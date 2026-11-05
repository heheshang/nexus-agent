//! SSE 桥接 —— 事件流 → HTTP 响应体。
//!
//! # 决策 D8：手写 `Body::from_stream`，**不用** axum 的 `Sse`
//!
//! 参考架构 §1.5 **已定稿**：
//!
//! > axum 的 `Sse` 会用 `Event::default().data()` 自动转义，与 Go 版手写
//! > `event: / data:` 的字节输出不完全一致。**建议 Rust 版手写 `Body::from_stream`**。
//!
//! 前端 `lib/sse.ts` 是**按字节解析**的 —— 格式差异会表现为
//! 「前端收不到事件」这类**极难排查**的问题。
//!
//! # 事件名是契约
//!
//! SSE 的 `event:` 字段的值 = `nexus_core::EventPayload` 的事件名
//! （架构文档 §2.2.4.1 **已定稿**的 12 个）。
//!
//! ⚠️ **不得**加前缀、不得改大小写、不得用 Rust 变体名
//! （`ToolCall` ≠ `tool_call`）。改一个名字，前端那个分支就**静默失效**了。
//!
//! # `data:` 里放什么
//!
//! **负载本身**（`EventPayload` 的扁平 JSON），**不含** `run_id` 与 `seq`。
//!
//! 理由：
//!
//! 1. `run_id` / `seq` 是**执行记录**的标识 —— 前端按 `event:` 分支，
//!    不按执行记录标识分支。参考实现的 SSE `data` 同样只有负载字段。
//! 2. `EventPayload` 的 serde 内部标签已经产出 `type` 字段，
//!    所以 `data` 里**自带**事件名 —— 前端既能按 `event:` 分支，
//!    也能按 `data.type` 分支，两者一致。
//!
//! # 取消语义
//!
//! **客户端断开 = 取消。**
//!
//! 响应体被 drop 时，桥接的流随之被 drop —— 那一刻触发
//! [`CancellationToken`]。这是 P4 的取消语义在协议层的落点：
//! P4 的 `EventSink::cancel_token()` 与这里的断开检测**接在一起**。

use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::http::header::CONTENT_TYPE;
use axum::response::Response;
use futures::StreamExt;
use nexus_agent::events::EventStream;
use nexus_core::ErrorCode;
use nexus_core::event::{Event, EventPayload};
use tokio_util::sync::CancellationToken;

use crate::sse::encode_frame;

/// SSE 响应的 Content-Type。
///
/// # ⚠️ **必须显式设置**
///
/// `Body::from_stream` **不会**设置任何响应头 —— 它只包一个字节流。
///
/// 不设 `Content-Type: text/event-stream` 的话，
/// 前端的 `fetch` + `ReadableStream` 解析器**不会**把它当 SSE 处理
/// （`EventSource` 更是直接拒绝非 `text/event-stream` 的 MIME）。
///
/// 这是本模块实现时**真实踩到**的坑：
/// 初版只返回 `Body`，测试断言 content-type 时才发现响应头**完全缺失**。
pub const SSE_CONTENT_TYPE: &str = "text/event-stream";

/// 事件名 —— SSE `event:` 字段的值。
///
/// **穷尽匹配** —— `EventPayload` 新增变体时**编译失败**，
/// 强制补齐映射。这比「用 serde 的 `type` 字段」更严：
/// 后者会让新变体**自动**有名字，但**没人会注意到**它进了 SSE 协议。
///
/// # `Extension` 逃生舱
///
/// 架构文档 §2.2.4.1 定稿的是 **12 个**事件名 ——
/// `Extension` **不在**其中（它是**逃生舱**，不是契约的一部分）。
///
/// 但它**必须**有事件名 —— 否则它无法产出帧。
/// 这里给它 `"extension"`，与 serde 内部标签的 snake_case 形态一致。
///
/// 前端**不按**它分支（没有这个契约），
/// 但收到它不会**解析失败** —— 这正是逃生舱的意义。
pub fn event_name(payload: &EventPayload) -> &'static str {
    match payload {
        EventPayload::Meta { .. } => "meta",
        EventPayload::Delta { .. } => "delta",
        EventPayload::Reasoning { .. } => "reasoning",
        EventPayload::Plan { .. } => "plan",
        EventPayload::ToolCall { .. } => "tool_call",
        EventPayload::ToolResult { .. } => "tool_result",
        EventPayload::AgentEvent { .. } => "agent_event",
        EventPayload::Refs { .. } => "refs",
        EventPayload::Usage { .. } => "usage",
        EventPayload::Approval { .. } => "approval",
        EventPayload::Done { .. } => "done",
        EventPayload::Error { .. } => "error",
        EventPayload::Extension { .. } => "extension",
    }
}

/// 把一个事件编码为 SSE 帧。
///
/// **纯函数** —— 同样的事件得到同样的字节。
pub fn encode_event(event: &Event) -> Vec<u8> {
    match serde_json::to_value(&event.payload) {
        Ok(payload) => encode_frame(event_name(&event.payload), &payload),
        // 序列化失败是**内部**状态问题 —— 发 error 帧，不 panic。
        // 只给业务码与固定文案，不带内部原因。
        Err(_) => encode_error_frame(ErrorCode::Internal.code(), "事件负载序列化失败"),
    }
}

/// 把一个错误编码为 SSE `error` 帧。
///
/// ⚠️ **不含内部原因** —— 只有**业务码**与**面向用户的消息**
/// （架构文档 §2.6.5）。
pub fn encode_error_frame(code: i32, message: &str) -> Vec<u8> {
    encode_frame(
        "error",
        &serde_json::json!({
            "type": "error",
            "code": code,
            "message": message,
        }),
    )
}

/// 把事件流桥接成 HTTP 响应体。
///
/// # 行为
///
/// - 事件流**结束** → 流结束，**不补**额外的帧。
/// - 事件流**出错** → 发一个 `error` 帧，然后结束。
/// - **客户端断开** → `cancel` 令牌被触发。
///
/// # 参数
///
/// - `stream` —— 事件来源。
/// - `cancel` —— 断开时触发的取消令牌（**由调用方持有**）。
///
/// # 为什么 `cancel` 是**参数**而不是内部创建
///
/// 上层（service / agent）需要在**取消发生后**做清理
/// —— 释放共享额度、写终态、终止上游模型调用。
/// 令牌必须由**上层**创建并持有，桥接只是**触发**它。
pub fn sse_body(stream: EventStream, cancel: CancellationToken) -> Body {
    let guard = CancelOnDrop::new(cancel);

    let body_stream = futures::stream::unfold(
        Some((stream, guard)),
        |state: Option<(EventStream, CancelOnDrop)>| async move {
            let (mut stream, mut guard) = state?;

            match stream.next().await {
                Some(Ok(event)) => {
                    let bytes = Bytes::from(encode_event(&event));
                    Some((Ok::<Bytes, Infallible>(bytes), Some((stream, guard))))
                }
                Some(Err(_err)) => {
                    // ⚠️ 流出错 → 发一个 `error` 帧，然后结束。
                    //
                    // 用 `ServiceUnavailable`（90003）——
                    // 「来源通道已关闭」是**传输层**故障，不是业务失败。
                    // 只给业务码与固定文案 —— 不把内部 `Display` 拼进用户可见消息。
                    let bytes = Bytes::from(encode_error_frame(
                        ErrorCode::ServiceUnavailable.code(),
                        "事件流中断",
                    ));
                    // ⚠️ 上游出错**不是**「客户端断开」——解除取消。
                    // 否则上层会把一次正常收尾当成用户取消。
                    guard.disarm();
                    Some((Ok::<Bytes, Infallible>(bytes), None))
                }
                None => {
                    // ⚠️ 正常结束**不是**取消 —— 必须解除。
                    //
                    // 不解除的话，响应体的 drop（流结束时必然发生）
                    // 会触发取消令牌，与「客户端断开」不可区分。
                    guard.disarm();
                    None
                }
            }
        },
    );

    Body::from_stream(body_stream)
}

/// 把事件流桥接成**完整的 SSE 响应**（含响应头）。
///
/// # 为什么需要这一个
///
/// [`sse_body`] 返回的是**裸** `Body` —— 它**不带任何响应头**。
///
/// SSE 客户端要求 `Content-Type: text/event-stream`，
/// 所以对外暴露的应当是**这个**函数，不是 `sse_body`。
///
/// `sse_body` 保留为 `pub`，是为了让测试能拿到裸流做字节级断言。
pub fn sse_response(stream: EventStream, cancel: CancellationToken) -> Response {
    let body = sse_body(stream, cancel);
    match Response::builder()
        .header(CONTENT_TYPE, SSE_CONTENT_TYPE)
        .body(body)
    {
        Ok(resp) => resp,
        // 静态头 + 合法 body 构造不会失败；兜底也**不 panic** ——
        // 服务器里一个 `expect` 就是一次可用性事故。
        Err(_) => {
            let mut resp = Response::new(Body::empty());
            *resp.status_mut() = axum::http::StatusCode::INTERNAL_SERVER_ERROR;
            resp
        }
    }
}

/// 响应体被 drop 时触发取消。
///
/// # 为什么用 `Drop` 而不是显式调用
///
/// 客户端断开时，axum 会 **drop** 响应体 ——
/// 那时**没有任何代码路径**会「正常地」走到取消点。
///
/// `Drop` 是唯一可靠的钩子。
struct CancelOnDrop {
    cancel: CancellationToken,
    /// 是否仍应在 drop 时触发取消。
    ///
    /// 流**正常结束**或**上游出错结束**时必须解除 ——
    /// 那两种情况下响应体同样会被 drop，但它们**不是**「客户端断开」。
    armed: bool,
}

impl CancelOnDrop {
    fn new(cancel: CancellationToken) -> Self {
        Self {
            cancel,
            armed: true,
        }
    }

    /// 流已走完 —— 解除取消。
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::event::{AgentEventKind, EVENT_NAMES, EventPayload, FinishReason, ToolStatus};
    use nexus_core::id::{AgentId, SessionId, ToolCallId};

    /// 构造 13 种负载（12 个契约事件 + `Extension` 逃生舱）。
    fn samples() -> Vec<EventPayload> {
        vec![
            EventPayload::Meta {
                session_id: SessionId::new_v4(),
                agent_id: AgentId::new(1),
            },
            EventPayload::Delta {
                content: "x".into(),
            },
            EventPayload::Reasoning {
                content: "x".into(),
            },
            EventPayload::Plan {
                steps: vec!["a".into()],
                current: 0,
            },
            EventPayload::ToolCall {
                call_id: ToolCallId::new("c1"),
                name: "search".into(),
                arguments: serde_json::json!({"q": 1}),
            },
            EventPayload::ToolResult {
                call_id: ToolCallId::new("c1"),
                result: "ok".into(),
                elapsed_ms: 12,
                status: ToolStatus::Ok,
            },
            EventPayload::AgentEvent {
                kind: AgentEventKind::Delegate,
                expert: "e".into(),
                detail: "d".into(),
            },
            EventPayload::Refs {
                items: vec![serde_json::json!({"id": 1})],
            },
            EventPayload::Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                cost: Some("0.001".into()),
                model: "m".into(),
            },
            EventPayload::Approval {
                approval_id: "a1".into(),
                kind: "k".into(),
                payload: serde_json::json!({}),
            },
            EventPayload::Done {
                finish_reason: FinishReason::Stop,
            },
            EventPayload::Error {
                code: 90001,
                message: "boom".into(),
            },
            EventPayload::Extension {
                name: "custom".into(),
                data: serde_json::json!({"foo": "bar"}),
            },
        ]
    }

    #[test]
    fn all_twelve_event_names_map_correctly() {
        let mut names: Vec<&str> = samples().iter().map(event_name).collect();
        // 逃生舱不在 12 个契约事件名里。
        names.pop();

        assert_eq!(
            names, EVENT_NAMES,
            "12 个契约事件名必须与 nexus-core 的 EVENT_NAMES 逐条一致"
        );
        assert_eq!(names.len(), 12, "契约事件名应为 12 个");
    }

    #[test]
    fn the_extension_escape_hatch_has_an_event_name() {
        let ext = EventPayload::Extension {
            name: "custom".into(),
            data: serde_json::json!({}),
        };
        assert_eq!(event_name(&ext), "extension");
    }

    #[test]
    fn the_encoded_event_has_the_documented_bytes() {
        let event = Event {
            run_id: nexus_core::id::RunId::new(1),
            seq: 1,
            payload: EventPayload::Delta {
                content: "hi".into(),
            },
        };
        let out = String::from_utf8(encode_event(&event)).unwrap();
        assert!(
            out.starts_with("event: delta\ndata: {"),
            "实际输出：{out:?}"
        );
        assert!(out.ends_with("\n\n"), "帧必须以分隔符结尾");
        // data 里**不含** run_id / seq
        assert!(!out.contains("run_id"), "data 不应含 run_id");
        assert!(!out.contains("\"seq\""), "data 不应含 seq");
        // 但**含** type（serde 内部标签）
        assert!(out.contains("\"type\":\"delta\""), "data 应含 type");
    }

    #[test]
    fn the_error_frame_carries_no_internal_cause() {
        let out = String::from_utf8(encode_error_frame(90001, "boom")).unwrap();
        assert!(out.contains("\"code\":90001"));
        assert!(out.contains("\"message\":\"boom\""));
        assert!(!out.contains("cause"), "error 帧不得含 cause");
        assert!(!out.contains("source"), "error 帧不得含内部来源");
    }
}
