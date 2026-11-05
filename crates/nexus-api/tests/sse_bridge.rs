//! SSE 桥接的行为层测试。
//!
//! 验证的是**真实 HTTP 响应体** —— 不是内部函数返回值。
//!
//! 理由：桥接的正确性在「字节落到线上」这个**边界**上才成立。
//! 只测内部函数，测的是「函数返回了我期望的东西」，
//! 而「响应体真的是那些字节」是**另一回事**。

use std::collections::VecDeque;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use nexus_agent::events::EventStreamError;
use nexus_api::bridge::{encode_error_frame, encode_event, sse_body, sse_response};
use nexus_api::sse::FrameDecoder;
use nexus_core::event::{Event, EventPayload, FinishReason};
use nexus_core::id::RunId;
use std::task::{Context, Poll};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

/// 把响应体读成字节。
async fn read_body(body: Body) -> Vec<u8> {
    axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("读取响应体失败")
        .to_vec()
}

/// 造一个只含给定事件的流。
fn stream_of(events: Vec<Result<Event, EventStreamError>>) -> nexus_agent::events::EventStream {
    let mut queue: VecDeque<Result<Event, EventStreamError>> = events.into();
    let stream = futures::stream::poll_fn(move |_: &mut Context<'_>| -> Poll<Option<_>> {
        Poll::Ready(queue.pop_front())
    });
    Box::pin(stream)
}

/// 一个 delta 事件。
fn delta_event(content: &str) -> Event {
    Event {
        run_id: RunId::new(1),
        seq: 1,
        payload: EventPayload::Delta {
            content: content.to_string(),
        },
    }
}

#[tokio::test]
async fn every_frame_the_bridge_writes_can_be_decoded_back() {
    let stream = stream_of(vec![
        Ok(delta_event("第一段")),
        Ok(Event {
            run_id: RunId::new(1),
            seq: 2,
            payload: EventPayload::Done {
                finish_reason: FinishReason::Stop,
            },
        }),
    ]);

    let cancel = CancellationToken::new();
    let body = sse_body(stream, cancel.clone());
    let bytes = read_body(body).await;

    // 用 NEX-31 的解码器**完整解回** —— 证明输出符合帧格式。
    let mut decoder = FrameDecoder::new();
    let frames = decoder.push(&bytes).unwrap();

    assert_eq!(frames.len(), 2, "应有两帧");
    assert_eq!(frames[0].event, "delta");
    assert_eq!(frames[0].data, r#"{"type":"delta","content":"第一段"}"#);
    assert_eq!(frames[1].event, "done");
    assert_eq!(decoder.buffered(), 0, "缓冲区应清空");
}

#[tokio::test]
async fn the_stream_ends_without_extra_frames() {
    let stream = stream_of(vec![Ok(delta_event("x"))]);
    let cancel = CancellationToken::new();
    let body = sse_body(stream, cancel.clone());
    let bytes = read_body(body).await;

    // 逐字节等于「一帧 + 没有别的东西」
    let expected = encode_event(&Event {
        run_id: RunId::new(1),
        seq: 1,
        payload: EventPayload::Delta {
            content: "x".into(),
        },
    });
    assert_eq!(bytes, expected, "流结束不应补额外的帧");
}

#[tokio::test]
async fn a_stream_error_becomes_an_error_frame() {
    let stream = stream_of(vec![Err(EventStreamError::SourceClosed)]);
    let cancel = CancellationToken::new();
    let body = sse_body(stream, cancel.clone());
    let bytes = read_body(body).await;

    let mut decoder = FrameDecoder::new();
    let frames = decoder.push(&bytes).unwrap();

    assert_eq!(frames.len(), 1, "流出错应产出恰好一帧");
    assert_eq!(frames[0].event, "error");
    assert!(
        frames[0].data.contains("\"code\":90003"),
        "实际：{}",
        frames[0].data
    );
}

#[tokio::test]
async fn the_error_frame_carries_no_internal_cause() {
    let stream = stream_of(vec![Err(EventStreamError::SourceClosed)]);
    let cancel = CancellationToken::new();
    let body = sse_body(stream, cancel.clone());
    let bytes = read_body(body).await;

    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("cause"), "错误帧不得含 cause");
    assert!(
        !text.contains("source_closed") && !text.contains("SourceClosed"),
        "错误帧不得含内部错误类型名：{text}"
    );
}

#[tokio::test]
async fn the_error_frame_shape_is_stable() {
    // 断言**字节**形状 —— 不只是「有 error 事件」。
    let out = String::from_utf8(encode_error_frame(90003, "事件流中断")).unwrap();
    assert_eq!(
        out,
        "event: error\ndata: {\"type\":\"error\",\"code\":90003,\"message\":\"事件流中断\"}\n\n"
    );
}

#[tokio::test]
async fn a_client_disconnect_cancels_the_run() {
    // ⚠️ 这是「客户端断开 = 取消」的验证。
    //
    // 断开时 axum 会 **drop** 响应体 —— 那时没有代码路径会「正常地」
    // 走到取消点，所以必须靠 `Drop`。
    let cancel = CancellationToken::new();
    let stream = stream_of(vec![]);
    let body = sse_body(stream, cancel.clone());

    assert!(!cancel.is_cancelled(), "初始不应被取消");

    // 模拟客户端断开 —— 响应体被 drop。
    drop(body);

    assert!(
        cancel.is_cancelled(),
        "响应体被 drop 后，取消令牌必须被触发"
    );
}

#[tokio::test]
async fn the_sse_response_carries_the_documented_content_type() {
    // 组一个最小 Router，验证端到端的响应头。
    let app = Router::new().route(
        "/stream",
        axum::routing::get(|| async {
            let cancel = CancellationToken::new();
            let stream = stream_of(vec![Ok(delta_event("x"))]);
            sse_response(stream, cancel)
        }),
    );

    let req = Request::builder()
        .uri("/stream")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        ct.starts_with("text/event-stream"),
        "content-type 应为 text/event-stream，实际：{ct}"
    );
}

#[tokio::test]
async fn finishing_the_stream_normally_does_not_cancel() {
    // ⚠️ 回归：正常结束时响应体同样会被 drop，
    // 但那是「流走完了」，不是「客户端断开」。
    // 守卫若不解除，上层会把正常收尾当成用户取消。
    let cancel = CancellationToken::new();
    let stream = stream_of(vec![Ok(delta_event("x"))]);
    let body = sse_body(stream, cancel.clone());
    let _ = read_body(body).await;

    assert!(
        !cancel.is_cancelled(),
        "正常结束不是客户端断开 —— 不得触发取消"
    );
}

#[tokio::test]
async fn an_upstream_error_does_not_cancel() {
    // 上游出错是「来源通道故障」，不是客户端断开。
    let cancel = CancellationToken::new();
    let stream = stream_of(vec![Err(EventStreamError::SourceClosed)]);
    let body = sse_body(stream, cancel.clone());
    let _ = read_body(body).await;

    assert!(
        !cancel.is_cancelled(),
        "上游出错不是客户端断开 —— 不得触发取消"
    );
}
