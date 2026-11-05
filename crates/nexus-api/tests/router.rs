//! 路由骨架与 SSE 端点的行为层测试。
//!
//! 验证的是**真实 HTTP 响应** —— 端点真的能被访问，
//! SSE 帧真的按**字节**落在响应体里。

use std::collections::VecDeque;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nexus_agent::events::EventStream;
use nexus_api::middleware::{AuthenticatedIdentity, IdentityExtractor};
use nexus_api::router::{EventStreamProvider, RouterBuilder, SSE_DEMO_PATH};
use nexus_core::AppError;
use nexus_core::event::{Event, EventPayload};
use nexus_core::id::{RunId, UserId};
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

/// 一个事件脚本条目。
#[derive(Clone)]
enum ScriptItem {
    /// 正常事件。
    Event(Event),
}

/// 一个**产出固定几个事件**的数据源。
struct FixedEvents {
    items: Vec<ScriptItem>,
}

impl FixedEvents {
    fn new(items: Vec<ScriptItem>) -> Self {
        Self { items }
    }
}

impl EventStreamProvider for FixedEvents {
    fn open(
        &self,
        _identity: Option<&AuthenticatedIdentity>,
        _cancel: CancellationToken,
    ) -> EventStream {
        let mut queue: VecDeque<Result<Event, nexus_agent::events::EventStreamError>> = self
            .items
            .iter()
            .map(|item| match item {
                ScriptItem::Event(event) => Ok(event.clone()),
            })
            .collect();
        Box::pin(futures::stream::poll_fn(
            move |_: &mut Context<'_>| -> Poll<Option<_>> { Poll::Ready(queue.pop_front()) },
        ))
    }
}

/// 一个**永不产出事件**的数据源 —— 用于测心跳。
struct NeverProduces;

impl EventStreamProvider for NeverProduces {
    fn open(
        &self,
        _identity: Option<&AuthenticatedIdentity>,
        _cancel: CancellationToken,
    ) -> EventStream {
        Box::pin(futures::stream::pending())
    }
}

/// 一个**记录收到过身份**的数据源。
struct RecordsIdentity(Arc<std::sync::Mutex<Option<String>>>);

impl EventStreamProvider for RecordsIdentity {
    fn open(
        &self,
        identity: Option<&AuthenticatedIdentity>,
        _cancel: CancellationToken,
    ) -> EventStream {
        *self.0.lock().unwrap() = identity.map(|id| id.subject.clone());
        Box::pin(futures::stream::pending())
    }
}

/// 一个**总是放行**的提取器。
struct AllowAll;

impl IdentityExtractor for AllowAll {
    fn extract(&self, _headers: &axum::http::HeaderMap) -> Result<AuthenticatedIdentity, AppError> {
        Ok(AuthenticatedIdentity::new(Some(UserId::new(9)), "carol"))
    }
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

/// 一个 done 事件。
fn done_event() -> Event {
    Event {
        run_id: RunId::new(1),
        seq: 2,
        payload: EventPayload::Done {
            finish_reason: nexus_core::event::FinishReason::Stop,
        },
    }
}

#[tokio::test]
async fn the_sse_demo_endpoint_is_reachable_and_streams_frames() {
    // AC #5：交付**一个**真实可跑的 SSE 端点。
    //
    // 断言的是**响应体里的字节** —— 不是「调用了某个函数」。
    let provider = Arc::new(FixedEvents::new(vec![
        ScriptItem::Event(delta_event("你好")),
        ScriptItem::Event(done_event()),
    ]));
    let app = RouterBuilder::new()
        .with_sse_provider(provider)
        .register_sse_demo(SSE_DEMO_PATH)
        .build();

    let req = Request::builder()
        .uri(SSE_DEMO_PATH)
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

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();

    assert!(
        text.starts_with("event: delta\ndata: {"),
        "第一帧应是 delta，实际：{text}"
    );
    assert!(text.contains("event: done\n"), "应含 done 帧：{text}");
    assert!(text.ends_with("\n\n"), "最后一帧也要有分隔符：{text}");
}

#[tokio::test]
async fn the_sse_demo_endpoint_is_not_registered_without_a_provider() {
    // 没有数据源 → **不注册**端点。
    //
    // 端点没有数据源就是**空壳** —— 而「空壳比没有更糟」
    // （它看起来完成了）。所以宁可不注册。
    //
    // ⚠️ 断言的是 **404**（不是「不是 500」）。
    //
    // 变异的常见形态是「没数据源时用一个空 provider 顶上」——
    // 那会返回 200（连接建立后什么也不写）。
    // 只断言「不是 500」抓不住它；断言 404 才抓得住。
    let app = RouterBuilder::new()
        .register_sse_demo(SSE_DEMO_PATH)
        .build();

    let req = Request::builder()
        .uri(SSE_DEMO_PATH)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "没有数据源时不应注册该端点"
    );
}

#[tokio::test]
async fn the_heartbeat_is_written_on_an_idle_connection() {
    // AC #9：路由层注入**心跳** —— 空闲连接按配置间隔写出 `: ping\n\n`。
    //
    // 用**短间隔**（50ms）让测试跑得快 —— 间隔是**配置项**，
    // 行为与间隔长度无关。
    let provider = Arc::new(NeverProduces);
    let app = RouterBuilder::new()
        .with_sse_provider(provider)
        .with_heartbeat(Some(Duration::from_millis(50)))
        .register_sse_demo(SSE_DEMO_PATH)
        .build();

    let req = Request::builder()
        .uri(SSE_DEMO_PATH)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let mut body = resp.into_body().into_data_stream();

    // 读**一个** chunk —— 它应该是心跳（数据源永不产出事件）。
    let chunk = tokio::time::timeout(Duration::from_secs(5), futures::StreamExt::next(&mut body))
        .await
        .expect("心跳应在超时前到达")
        .expect("应有数据")
        .expect("chunk 不应是错误");

    assert_eq!(
        chunk,
        axum::body::Bytes::from_static(b": ping\n\n"),
        "空闲连接应写出 `: ping\\n\\n` 注释帧"
    );
}

#[tokio::test]
async fn the_heartbeat_is_disabled_when_the_interval_is_absent() {
    // 心跳**可关闭** —— 关掉后空闲连接**不**写任何东西。
    //
    // ⚠️ 为什么用**短**超时窗口
    //
    // 默认心跳间隔是 15 秒。如果这里等 15 秒才断言「没写心跳」，
    // 那「心跳被禁用」与「心跳间隔恰好很长」在这个窗口里**表现一样** ——
    // 断言**无法区分**两者，也就**抓不住**「禁用被忽略」这类变异。
    //
    // 所以这里用 **300ms** 的窗口 —— 它**远小于**任何合理的默认间隔。
    // 变异「把 None 也变成一个很短的心跳」会让心跳在 300ms 内到达 → 断言变红。
    //
    // （这条是第 3 轮故障注入**实际发现**的：
    //  最初这里等 300ms 而变异把 None 变成默认间隔（15s），
    //  断言**未变红** —— 说明它抓不住那类变异。
    //  换成「None → 50ms」的变异后才变红。）
    let provider = Arc::new(NeverProduces);
    let app = RouterBuilder::new()
        .with_sse_provider(provider)
        .with_heartbeat(None)
        .register_sse_demo(SSE_DEMO_PATH)
        .build();

    let req = Request::builder()
        .uri(SSE_DEMO_PATH)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let mut body = resp.into_body().into_data_stream();

    let result = tokio::time::timeout(
        Duration::from_millis(300),
        futures::StreamExt::next(&mut body),
    )
    .await;

    assert!(result.is_err(), "心跳关闭后不应写出任何字节");
}

#[tokio::test]
async fn the_sse_endpoint_receives_the_authenticated_identity() {
    // 受保护的端点 → 数据源能拿到身份。
    //
    // 这验证了「身份 → `Extension` → handler」这条链路是**通的**。
    let seen = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(RecordsIdentity(seen.clone()));

    let app = RouterBuilder::new()
        .with_identity_extractor(Arc::new(AllowAll))
        .with_sse_provider(provider)
        .register_sse_demo_protected(SSE_DEMO_PATH)
        .build();

    let req = Request::builder()
        .uri(SSE_DEMO_PATH)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let recorded = seen.lock().unwrap().clone();
    assert_eq!(
        recorded.as_deref(),
        Some("carol"),
        "数据源应拿到已认证的身份"
    );
}

#[tokio::test]
async fn the_router_builder_registers_routes_incrementally() {
    // AC #4：可**增量注册**的路由构建器。
    //
    // 注册三条 → 三条都应可达（后注册的**不影响**先注册的）。
    let app = RouterBuilder::new()
        .register("/a", axum::routing::get(|| async { "a" }))
        .register("/b", axum::routing::get(|| async { "b" }))
        .register("/c", axum::routing::get(|| async { "c" }))
        .build();

    for (path, expected) in [("/a", "a"), ("/b", "b"), ("/c", "c")] {
        let req = Request::builder().uri(path).body(Body::empty()).unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{path} 应可达");
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), expected);
    }
}

#[tokio::test]
async fn the_request_id_is_present_on_every_route() {
    // 请求 ID 中间件**全局挂** —— 每条路由都应带上。
    let app = RouterBuilder::new()
        .register("/a", axum::routing::get(|| async { "a" }))
        .build();

    let req = Request::builder().uri("/a").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert!(
        resp.headers()
            .get(nexus_api::middleware::REQUEST_ID_HEADER)
            .is_some(),
        "每个响应都应带请求 ID"
    );
}

#[tokio::test]
async fn a_protected_route_is_rejected_without_credentials() {
    // 受保护的路由在提取失败时 → 401。
    struct RejectAll;
    impl IdentityExtractor for RejectAll {
        fn extract(
            &self,
            _headers: &axum::http::HeaderMap,
        ) -> Result<AuthenticatedIdentity, AppError> {
            Err(AppError::new(
                nexus_core::ErrorCode::Unauthorized,
                "缺少凭证",
            ))
        }
    }

    let app = RouterBuilder::new()
        .with_identity_extractor(Arc::new(RejectAll))
        .register_protected("/secret", axum::routing::get(|| async { "secret" }))
        .build();

    let req = Request::builder()
        .uri("/secret")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], serde_json::json!(20101));
}
