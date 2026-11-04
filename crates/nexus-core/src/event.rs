//! 事件负载与单写者事件流。
//!
//! 架构文档 §2.2 —— 事件流是**类型化的 Stream**。
//!
//! # 事件负载：类型化 enum 而非 `Value`（§2.2.4）
//!
//! 参考实现用 `map[string]any` 作负载（`internal/agent/event.go`）。本设计**不用 `Value` 兜底** ——
//! 原则 3 说「不变量优先由类型保证」。
//!
//! 收益：
//!
//! - 构造事件时字段名写错 → **编译错误**（`Value` 版本不报错）。
//! - `ToolStatus` / `FinishReason` 是 enum，不是字符串 —— 消费端的 `match` 穷尽检查。
//! - 序列化后仍是 `{"type": "...", ...}` 的**扁平 JSON**，与前端协议兼容。
//!
//! 代价：新增事件类型要改 enum。这是**想要**的成本 —— 事件协议是前后端契约，不该随便变。
//!
//! # 事件名是契约
//!
//! 事件名是 SSE `event:` 字段的值，**前端按它分支**。少一个或改一个就是破坏性变更。
//! §2.2.4.1 的 12 个事件名由 [`EVENT_NAMES`] 固化，并由契约测试逐条比对。
//!
//! # 单写者保证（§2.2.3）
//!
//! 事件流的**关键不变量**不是「它是流」，而是：
//!
//! > 序号必须严格单调、无重复、无空洞，且与写入顺序一致。
//!
//! `mpsc::Sender::send()` 是异步的 —— 如果在多个地方并发发送，**谁先被 poll 到是不确定的**，
//! 序号分配与写入顺序就会脱节。
//!
//! 因此**不给生产者发 `Sender`**，只给一个受控的写入接口 [`EventSink`]：
//!
//! - `EventSink` **不可 `Clone`** —— 物理上杜绝多写者。
//! - `emit(&mut self, ...)` —— 写入需要独占借用，编译期串行化。
//! - 序号分配与写入在同一临界区 —— 顺序天然一致。
//!
//! # 通道语义（§3.2）
//!
//! §2.2.4.1 的「载荷通道」列把事件分成两类：
//!
//! - **可丢**：只有 `delta` / `reasoning` —— 高频增量，丢了不影响正确性。
//! - **必达**：其余全部 —— 丢了就是语义丢失（工具调用丢了 = 动作没发生；`done` 丢了 = 客户端永远转圈）。
//!
//! 这个区分由 [`EventChannel`] 的**类型**承载，不是运行时约定。

use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::id::{AgentId, RunId, SessionId, ToolCallId};

/// 结束原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// 正常结束。
    Stop,
    /// 因工具调用而结束。
    ToolCalls,
    /// 因长度上限而结束。
    Length,
}

/// 工具执行状态。
///
/// 注意带数据变体的序列化形状（附录 C.1 陷阱）：
/// `Failed { code }` → `{"failed": {"code": 70002}}`，**不是** `{"code": 70002}`。
/// 契约测试固化实际形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    /// 执行成功。
    Ok,
    /// 执行失败，携带业务码。
    Failed {
        /// 业务错误码（与 `nexus-core::error::ErrorCode` 同一套码表）。
        code: i32,
    },
}

/// 多智能体事件的子类型。
///
/// 参考实现用**第二个** `type` 字段表达（`"delegate"` / `"progress"` / `"merge"`），
/// 本设计改名 `kind` —— 避免与外层 `type` 混淆。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    /// 委派给专家。
    Delegate,
    /// 进度汇报。
    Progress,
    /// 结果合并。
    Merge,
}

/// 事件负载。
///
/// 用 `#[serde(tag = "type")]` 做**内部标签** —— 序列化后 `type` 与其他字段**同级**。
///
/// **12 个事件类型**与架构文档 §2.2.4.1 的契约表一一对应，字段名**逐字一致**。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    /// 会话元信息。
    Meta {
        /// 会话 ID。
        session_id: SessionId,
        /// Agent 定义 ID。
        agent_id: AgentId,
    },
    /// 增量文本。**可丢**。
    Delta {
        /// 增量内容。
        content: String,
    },
    /// 推理内容。**可丢**。
    Reasoning {
        /// 推理内容。
        content: String,
    },
    /// 执行计划。
    Plan {
        /// 计划步骤。
        steps: Vec<String>,
        /// 当前进行到的步骤序号。
        current: usize,
    },
    /// 工具调用请求。
    ToolCall {
        /// 工具调用标识。
        ///
        /// 刻意**不叫** `id` —— `id` 在事件里语义过泛（是 run 的 id？session 的 id？）。
        /// 这是一处**协议破坏性变更**（§2.2.4.1），前端必须同步改。
        call_id: ToolCallId,
        /// 工具名。
        name: String,
        /// 调用参数。
        arguments: Value,
    },
    /// 工具执行结果。
    ToolResult {
        /// 对应的工具调用标识。
        call_id: ToolCallId,
        /// 结果内容。
        result: String,
        /// 耗时（毫秒）。
        ///
        /// 刻意**不叫** `ms` —— 单位后缀显式化，`ms` 单独出现时读者要靠约定才知道单位。
        /// 这是一处**协议破坏性变更**（§2.2.4.1）。
        elapsed_ms: u64,
        /// 执行状态。
        status: ToolStatus,
    },
    /// 多智能体事件。
    AgentEvent {
        /// 子类型。参考实现用第二个 `type` 字段，本设计改名 `kind`。
        kind: AgentEventKind,
        /// 涉及的专家。
        expert: String,
        /// 细节描述。
        detail: String,
    },
    /// 引用（检索结果等）。
    Refs {
        /// 引用条目。
        items: Vec<Value>,
    },
    /// Token 用量与成本。
    Usage {
        /// 输入 token 数。
        prompt_tokens: u64,
        /// 输出 token 数。
        completion_tokens: u64,
        /// 成本。
        ///
        /// 用**十进制字符串**而不是 `f64` —— 浮点在跨系统传递中会丢精度，
        /// 而成本是要**累加**并对账的数字。
        cost: Option<String>,
        /// 模型标识。
        model: String,
    },
    /// 人工审批请求。
    Approval {
        /// 审批 ID。
        approval_id: String,
        /// 审批种类。
        kind: String,
        /// 审批负载。
        payload: Value,
    },
    /// 正常终止。
    Done {
        /// 结束原因。
        finish_reason: FinishReason,
    },
    /// 错误终止。
    Error {
        /// 业务错误码。
        code: i32,
        /// 面向用户的错误消息。
        message: String,
    },
    /// 逃生舱：插件式扩展。
    ///
    /// 注意 `flatten` 在**变体内部的 struct 字段**上，
    /// **不能**标注在 enum 的变体上（内部 tag 与 enum 级 flatten 不兼容 —— 附录 C.1 陷阱）。
    Extension {
        /// 扩展名。
        name: String,
        /// 扩展携带的任意字段。
        #[serde(flatten)]
        data: Value,
    },
}

/// 统一事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 所属执行记录。
    pub run_id: RunId,
    /// 序号。从 1 开始，严格单调递增。
    pub seq: u64,
    /// 事件负载。
    #[serde(flatten)]
    pub payload: EventPayload,
}

/// 事件写入器。
///
/// **不可 `Clone`** —— 物理上杜绝多写者。
/// 写入需要 `&mut self` —— 编译期串行化。
///
/// 这两个性质合起来保证 §2.2.3 的不变量：**序号严格单调、无重复、无空洞，
/// 且与写入顺序一致**。
#[derive(Debug)]
pub struct EventSink {
    run_id: RunId,
    seq: u64,
}

impl EventSink {
    /// 为一次执行创建写入器。
    pub fn new(run_id: RunId) -> Self {
        Self { run_id, seq: 0 }
    }

    /// 写入一个事件并分配序号。
    ///
    /// 序号从 **1** 开始。
    pub fn emit(&mut self, payload: EventPayload) -> Event {
        self.seq += 1;
        Event {
            run_id: self.run_id,
            seq: self.seq,
            payload,
        }
    }

    /// 已发出的事件数。
    pub fn emitted(&self) -> u64 {
        self.seq
    }

    /// 所属的执行记录。
    pub fn run_id(&self) -> RunId {
        self.run_id
    }
}

/// 事件载荷的通道语义（§3.2）。
///
/// **可丢**与**必达**的区分由**类型**承载，不是运行时约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventChannel {
    /// 可丢：高频增量，丢了不影响正确性（下一段会补上）。
    Lossy,
    /// 必达：丢了就是语义丢失。
    Guaranteed,
}

impl EventPayload {
    /// 该事件属于哪条通道。
    ///
    /// 契约：只有 `delta` / `reasoning` 是**可丢**，其余全部**必达**。
    /// 这个映射由契约测试固化 —— 改错会导致终态事件被丢，客户端永远转圈。
    pub fn channel(&self) -> EventChannel {
        match self {
            EventPayload::Delta { .. } | EventPayload::Reasoning { .. } => EventChannel::Lossy,
            _ => EventChannel::Guaranteed,
        }
    }
}

/// 事件流。
///
/// 消费侧用 `Stream` 而不是 `mpsc::Receiver`：
///
/// 1. `Stream` 是 Rust 生态的通用异步流抽象 —— 与 `futures::StreamExt`、`tokio::select!`、
///    `axum::body::Body` 天然组合。
/// 2. `ReceiverStream` 是 `tokio-stream` 提供的适配器，**已经有现成实现**，不需要自己写。
/// 3. `Stream<Item = Result<..>>` 让**错误成为流的一部分** —— 与「流可以以错误结束」的
///    自然语义一致，而 `Receiver` 只能靠 channel 关闭来表示结束。
pub type EventStream = Pin<Box<dyn Stream<Item = Result<Event, EventStreamError>> + Send>>;

/// 事件流中的错误。
///
/// 事件流本身**不携带**应用错误码（那是 `AppError` 的职责，属于 NEX-6）——
/// 这里只表示「流本身出了故障」，例如来源通道已关闭。
#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
    /// 来源通道已关闭，无法继续读取。
    #[error("事件来源已关闭")]
    SourceClosed,
}

/// 全部 12 个事件名 —— **契约**。
///
/// 顺序与架构文档 §2.2.4.1 的表一致。
///
/// 事件名是 SSE `event:` 字段的值，**前端按它分支** —— 少一个或改一个就是破坏性变更。
pub const EVENT_NAMES: &[&str] = &[
    "meta",
    "delta",
    "reasoning",
    "plan",
    "tool_call",
    "tool_result",
    "agent_event",
    "refs",
    "usage",
    "approval",
    "done",
    "error",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 12 个事件的样例负载 —— 契约测试用。
    fn samples() -> Vec<EventPayload> {
        vec![
            EventPayload::Meta {
                session_id: SessionId::new_v4(),
                agent_id: AgentId::new(7),
            },
            EventPayload::Delta {
                content: "x".into(),
            },
            EventPayload::Reasoning {
                content: "y".into(),
            },
            EventPayload::Plan {
                steps: vec!["a".into()],
                current: 0,
            },
            EventPayload::ToolCall {
                call_id: ToolCallId::new("c1"),
                name: "search".into(),
                arguments: json!({"q": "rust"}),
            },
            EventPayload::ToolResult {
                call_id: ToolCallId::new("c1"),
                result: "ok".into(),
                elapsed_ms: 12,
                status: ToolStatus::Ok,
            },
            EventPayload::AgentEvent {
                kind: AgentEventKind::Delegate,
                expert: "researcher".into(),
                detail: "d".into(),
            },
            EventPayload::Refs {
                items: vec![json!({"doc_id": 42})],
            },
            EventPayload::Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                cost: Some("0.0012".into()),
                model: "gpt-4".into(),
            },
            EventPayload::Approval {
                approval_id: "ap-1".into(),
                kind: "tool_write".into(),
                payload: json!({"tool": "write_file"}),
            },
            EventPayload::Done {
                finish_reason: FinishReason::ToolCalls,
            },
            EventPayload::Error {
                code: 90001,
                message: "boom".into(),
            },
        ]
    }

    /// 取事件名（测试辅助）。
    fn event_name(payload: &EventPayload) -> String {
        serde_json::to_value(payload).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// **契约测试** —— 事件名清单必须与架构文档 §2.2.4.1 的表一致。
    ///
    /// 事件名是 SSE `event:` 字段的值，前端按它分支 —— 少一个或改一个就是破坏性变更。
    #[test]
    fn event_name_list_matches_contract() {
        let actual: Vec<String> = samples().iter().map(event_name).collect();

        assert_eq!(actual, EVENT_NAMES, "事件名清单与契约不符");
        assert_eq!(EVENT_NAMES.len(), 12, "事件类型数量应为 12");
    }

    /// **契约测试** —— 字段名逐条比对。
    ///
    /// 重点是两处**刻意偏离**参考实现的名字：
    /// `call_id` 不是 `id`，`elapsed_ms` 不是 `ms`。
    #[test]
    fn field_names_match_contract() {
        // tool_call：call_id 而非 id
        let v = serde_json::to_value(EventPayload::ToolCall {
            call_id: ToolCallId::new("c1"),
            name: "search".into(),
            arguments: Value::Null,
        })
        .unwrap();
        assert!(v.get("call_id").is_some(), "缺 call_id");
        assert!(v.get("id").is_none(), "不应出现 id —— 刻意偏离参考实现");

        // tool_result：elapsed_ms 而非 ms
        let v = serde_json::to_value(EventPayload::ToolResult {
            call_id: ToolCallId::new("c1"),
            result: "ok".into(),
            elapsed_ms: 12,
            status: ToolStatus::Ok,
        })
        .unwrap();
        assert!(v.get("elapsed_ms").is_some(), "缺 elapsed_ms");
        assert!(v.get("ms").is_none(), "不应出现 ms —— 刻意偏离参考实现");

        // 其余字段名
        let expected: &[(&str, &[&str])] = &[
            ("meta", &["session_id", "agent_id"]),
            ("delta", &["content"]),
            ("reasoning", &["content"]),
            ("plan", &["steps", "current"]),
            ("tool_call", &["call_id", "name", "arguments"]),
            (
                "tool_result",
                &["call_id", "result", "elapsed_ms", "status"],
            ),
            ("agent_event", &["kind", "expert", "detail"]),
            ("refs", &["items"]),
            (
                "usage",
                &["prompt_tokens", "completion_tokens", "cost", "model"],
            ),
            ("approval", &["approval_id", "kind", "payload"]),
            ("done", &["finish_reason"]),
            ("error", &["code", "message"]),
        ];

        for (payload, (event_name, fields)) in samples().iter().zip(expected) {
            let v = serde_json::to_value(payload).unwrap();
            let obj = v.as_object().unwrap();
            for field in *fields {
                assert!(obj.contains_key(*field), "{event_name} 缺字段 {field}");
            }
        }
    }

    /// 负载序列化后是**扁平 JSON** —— `type` 与其他字段同级。
    #[test]
    fn serialization_is_flat_with_type_at_sibling_level() {
        let v = serde_json::to_value(EventPayload::Delta {
            content: "hi".into(),
        })
        .unwrap();

        assert_eq!(v["type"], "delta");
        assert_eq!(v["content"], "hi");
        // 不应出现嵌套的 payload 结构
        assert!(v.get("payload").is_none());
    }

    /// `agent_event` 的子类型用 `kind` 而非**第二个** `type`。
    #[test]
    fn agent_event_uses_kind_not_a_second_type() {
        let v = serde_json::to_value(EventPayload::AgentEvent {
            kind: AgentEventKind::Delegate,
            expert: "researcher".into(),
            detail: "d".into(),
        })
        .unwrap();

        assert_eq!(v["type"], "agent_event");
        assert_eq!(v["kind"], "delegate");

        // 只有**一个** type 字段
        let type_count = v
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.as_str() == "type")
            .count();
        assert_eq!(type_count, 1, "不应出现第二个 type 字段");
    }

    /// 逃生舱 `Extension` 携带任意字段（`flatten` 在变体内部的 struct 字段上）。
    #[test]
    fn extension_escape_hatch_carries_arbitrary_fields() {
        let v = serde_json::to_value(EventPayload::Extension {
            name: "custom".into(),
            data: json!({"foo": "bar"}),
        })
        .unwrap();

        assert_eq!(v["type"], "extension");
        assert_eq!(v["name"], "custom");
        assert_eq!(v["foo"], "bar");
    }

    /// 12 个变体全部往返序列化。
    #[test]
    fn all_event_types_roundtrip_serialization() {
        for sample in samples() {
            let json = serde_json::to_string(&sample).unwrap();
            let back: EventPayload = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_value(&back).unwrap(),
                serde_json::to_value(&sample).unwrap()
            );
        }
    }

    /// 带数据变体的形状被固化（附录 C.1 陷阱）。
    #[test]
    fn state_enum_serializes_to_lowercase_tag() {
        // 无数据变体 → 字符串 "ok"
        assert_eq!(serde_json::to_value(ToolStatus::Ok).unwrap(), json!("ok"));
        // 带数据变体 → { "failed": { "code": … } }
        assert_eq!(
            serde_json::to_value(ToolStatus::Failed { code: 70002 }).unwrap(),
            json!({"failed": {"code": 70002}})
        );

        assert_eq!(
            serde_json::to_value(FinishReason::ToolCalls).unwrap(),
            json!("tool_calls")
        );
        assert_eq!(
            serde_json::to_value(AgentEventKind::Merge).unwrap(),
            json!("merge")
        );
    }

    /// 序号从 1 开始、严格递增、与写入顺序一致。
    #[test]
    fn sequence_starts_at_1_and_strictly_increases() {
        let mut sink = EventSink::new(RunId::new(1));

        assert_eq!(sink.emitted(), 0);
        assert_eq!(
            sink.emit(EventPayload::Delta {
                content: "a".into()
            })
            .seq,
            1
        );
        assert_eq!(
            sink.emit(EventPayload::Done {
                finish_reason: FinishReason::Stop
            })
            .seq,
            2
        );
        assert_eq!(sink.emitted(), 2);
    }

    /// `EventSink` 的序号与写入顺序**严格一致** —— 单写者保证的实质。
    #[test]
    fn sequence_matches_write_order() {
        let mut sink = EventSink::new(RunId::new(1));

        for i in 1..=10 {
            let event = sink.emit(EventPayload::Delta {
                content: format!("chunk-{i}"),
            });
            assert_eq!(event.seq, i);
        }
    }

    /// 通道语义：只有 `delta` / `reasoning` 可丢，其余必达。
    ///
    /// 改错会导致终态事件被丢 —— 客户端永远转圈。
    #[test]
    fn only_delta_and_reasoning_are_lossy() {
        for sample in samples() {
            let name = event_name(&sample);
            let expected = if name == "delta" || name == "reasoning" {
                EventChannel::Lossy
            } else {
                EventChannel::Guaranteed
            };
            assert_eq!(sample.channel(), expected, "{name} 的通道语义与契约不符");
        }
    }

    /// `Event` 的序列化是扁平的：`run_id` / `seq` 与负载字段同级。
    #[test]
    fn event_serializes_flat() {
        let mut sink = EventSink::new(RunId::new(42));
        let event = sink.emit(EventPayload::Delta {
            content: "hi".into(),
        });

        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["run_id"], 42);
        assert_eq!(v["seq"], 1);
        assert_eq!(v["type"], "delta");
        assert_eq!(v["content"], "hi");
    }
}
