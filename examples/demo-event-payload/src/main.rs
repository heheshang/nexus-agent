//! demo-event-payload
//!
//! 验证设计文档 §2.2.4：事件负载用**类型化 enum** 而非 `serde_json::Value`，
//! 且序列化后是**扁平 JSON**（`type` 与其他字段同级），与前端协议兼容。
//!
//! 同时验证 §2.2.3 的「单写者 + 显式序号」：`EventSink` 不可 `Clone`，
//! 写入需要 `&mut self`，因此序号与写入顺序天然一致。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 结束原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
}

/// 工具执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Ok,
    Failed { code: i32 },
}

/// 多智能体事件的子类型。
///
/// 参考实现用第二个 `type` 字段表达（`"delegate"` / `"progress"` / `"merge"`），
/// 本设计改名 `kind` —— 避免与外层 `type` 混淆。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventKind {
    Delegate,
    Progress,
    Merge,
}

/// 事件负载。
///
/// 用 `#[serde(tag = "type")]` 做**内部标签** —— 序列化后 `type` 与其他字段同级。
///
/// **完整 12 个事件类型** —— 与设计文档 §2.2.4.1 的契约表一一对应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    Meta {
        session_id: String,
        agent_id: i64,
    },
    Delta {
        content: String,
    },
    Reasoning {
        content: String,
    },
    Plan {
        steps: Vec<String>,
        current: usize,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        result: String,
        elapsed_ms: u64,
        status: ToolStatus,
    },
    AgentEvent {
        kind: AgentEventKind,
        expert: String,
        detail: String,
    },
    Refs {
        items: Vec<Value>,
    },
    Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
        cost: Option<String>,
        model: String,
    },
    Approval {
        approval_id: String,
        kind: String,
        payload: Value,
    },
    Done {
        finish_reason: FinishReason,
    },
    Error {
        code: i32,
        message: String,
    },
    /// 逃生舱：插件式扩展。
    ///
    /// 注意 `flatten` 在**变体内部的 struct 字段**上，
    /// 不能标注在 enum 的变体上（内部 tag 与 enum 级 flatten 不兼容）。
    Extension {
        name: String,
        #[serde(flatten)]
        data: Value,
    },
}

/// 统一事件。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub run_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub payload: EventPayload,
}

/// 事件写入器。
///
/// **不可 `Clone`** —— 物理上杜绝多写者。
/// 写入需要 `&mut self` —— 编译期串行化。
#[derive(Debug)]
pub struct EventSink {
    run_id: String,
    seq: u64,
}

impl EventSink {
    pub fn new(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            seq: 0,
        }
    }

    /// 写入一个事件并分配序号。
    pub fn emit(&mut self, payload: EventPayload) -> Event {
        self.seq += 1;
        Event {
            run_id: self.run_id.clone(),
            seq: self.seq,
            payload,
        }
    }

    /// 已发出的事件数。
    pub fn emitted(&self) -> u64 {
        self.seq
    }
}

fn main() {
    let mut sink = EventSink::new("run-1");

    let events = [
        EventPayload::Meta {
            session_id: "s-1".into(),
            agent_id: 7,
        },
        EventPayload::Delta {
            content: "你好".into(),
        },
        EventPayload::Reasoning {
            content: "思考中".into(),
        },
        EventPayload::Plan {
            steps: vec!["检索".into(), "回答".into()],
            current: 0,
        },
        EventPayload::ToolCall {
            call_id: "c1".into(),
            name: "search".into(),
            arguments: serde_json::json!({ "q": "rust" }),
        },
        EventPayload::ToolResult {
            call_id: "c1".into(),
            result: "ok".into(),
            elapsed_ms: 12,
            status: ToolStatus::Ok,
        },
        EventPayload::AgentEvent {
            kind: AgentEventKind::Delegate,
            expert: "researcher".into(),
            detail: "委派检索任务".into(),
        },
        EventPayload::Refs {
            items: vec![serde_json::json!({"doc_id": 42, "title": "Rust 所有权"})],
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
            payload: serde_json::json!({"tool": "write_file"}),
        },
        EventPayload::Done {
            finish_reason: FinishReason::ToolCalls,
        },
    ];

    for p in events {
        let ev = sink.emit(p);
        println!(
            "seq={} {}",
            ev.seq,
            serde_json::to_string(&ev.payload).unwrap()
        );
    }

    // ── 验证扁平化 ──
    let json = serde_json::to_value(EventPayload::ToolResult {
        call_id: "c1".into(),
        result: "ok".into(),
        elapsed_ms: 12,
        status: ToolStatus::Ok,
    })
    .unwrap();
    assert_eq!(json["type"], "tool_result");
    assert_eq!(json["call_id"], "c1");
    println!("\n✓ 扁平化验证：{json}");
    println!("✓ 序号连续且与写入顺序一致：1..={}", sink.emitted());
    println!("✓ EventSink 不可 Clone —— 单写者由类型保证");
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn state_enum_serializes_to_lowercase_tag() {
        // 无数据变体 → 字符串 "ok"
        assert_eq!(
            serde_json::to_value(ToolStatus::Ok).unwrap(),
            serde_json::json!("ok")
        );
        // 带数据变体 → { "failed": { ... } }
        assert_eq!(
            serde_json::to_value(ToolStatus::Failed { code: 70002 }).unwrap(),
            serde_json::json!({"failed": {"code": 70002}})
        );
    }

    #[test]
    fn extension_escape_hatch_carries_arbitrary_fields() {
        let v = serde_json::to_value(EventPayload::Extension {
            name: "custom".into(),
            data: serde_json::json!({"foo": "bar"}),
        })
        .unwrap();
        assert_eq!(v["type"], "extension");
        assert_eq!(v["name"], "custom");
        assert_eq!(v["foo"], "bar");
    }

    #[test]
    fn sequence_starts_at_1_and_strictly_increases() {
        let mut sink = EventSink::new("r");
        assert_eq!(
            sink.emit(EventPayload::Meta {
                session_id: "s".into(),
                agent_id: 1,
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

    #[test]
    fn all_event_types_roundtrip_serialization() {
        let samples = [
            EventPayload::Meta {
                session_id: "s".into(),
                agent_id: 1,
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
                call_id: "c".into(),
                name: "n".into(),
                arguments: Value::Null,
            },
            EventPayload::ToolResult {
                call_id: "c".into(),
                result: "r".into(),
                elapsed_ms: 1,
                status: ToolStatus::Ok,
            },
            EventPayload::AgentEvent {
                kind: AgentEventKind::Merge,
                expert: "e".into(),
                detail: "d".into(),
            },
            EventPayload::Refs {
                items: vec![Value::Null],
            },
            EventPayload::Usage {
                prompt_tokens: 1,
                completion_tokens: 2,
                cost: None,
                model: "m".into(),
            },
            EventPayload::Approval {
                approval_id: "a".into(),
                kind: "k".into(),
                payload: Value::Null,
            },
            EventPayload::Done {
                finish_reason: FinishReason::Length,
            },
            EventPayload::Error {
                code: 90001,
                message: "boom".into(),
            },
        ];
        for s in samples {
            let json = serde_json::to_string(&s).unwrap();
            let back: EventPayload = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_value(&back).unwrap(),
                serde_json::to_value(&s).unwrap()
            );
        }
    }

    /// **契约测试** —— 事件名清单必须与参考实现 `internal/agent/event.go` 一致。
    ///
    /// 事件名是 SSE `event:` 字段的值，前端按它分支 —— 少一个或改一个就是破坏性变更。
    /// 这份清单是**契约**，不是实现细节。
    #[test]
    fn event_name_list_matches_contract() {
        const CONTRACT: &[&str] = &[
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

        let samples = [
            EventPayload::Meta {
                session_id: "s".into(),
                agent_id: 1,
            },
            EventPayload::Delta {
                content: "x".into(),
            },
            EventPayload::Reasoning {
                content: "x".into(),
            },
            EventPayload::Plan {
                steps: vec![],
                current: 0,
            },
            EventPayload::ToolCall {
                call_id: "c".into(),
                name: "n".into(),
                arguments: Value::Null,
            },
            EventPayload::ToolResult {
                call_id: "c".into(),
                result: "r".into(),
                elapsed_ms: 1,
                status: ToolStatus::Ok,
            },
            EventPayload::AgentEvent {
                kind: AgentEventKind::Delegate,
                expert: "e".into(),
                detail: "d".into(),
            },
            EventPayload::Refs { items: vec![] },
            EventPayload::Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cost: None,
                model: "m".into(),
            },
            EventPayload::Approval {
                approval_id: "a".into(),
                kind: "k".into(),
                payload: Value::Null,
            },
            EventPayload::Done {
                finish_reason: FinishReason::Stop,
            },
            EventPayload::Error {
                code: 1,
                message: "m".into(),
            },
        ];

        let actual: Vec<String> = samples
            .iter()
            .map(|s| {
                serde_json::to_value(s).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();

        assert_eq!(actual, CONTRACT, "事件名清单与契约不符");
    }

    #[test]
    fn agent_event_subtypes_use_kind_not_second_type() {
        // 参考实现用第二个 "type" 字段，本设计改名 kind 避免混淆
        let v = serde_json::to_value(EventPayload::AgentEvent {
            kind: AgentEventKind::Delegate,
            expert: "researcher".into(),
            detail: "d".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "agent_event");
        assert_eq!(v["kind"], "delegate");
        assert!(v.get("type").is_some());
        // 只有**一个** type 字段
        assert_eq!(
            v.as_object()
                .unwrap()
                .keys()
                .filter(|k| k.as_str() == "type")
                .count(),
            1
        );
    }
}
