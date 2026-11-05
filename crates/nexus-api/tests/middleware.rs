//! 协议层中间件的行为层测试。
//!
//! 验证的是**真实 HTTP 响应** —— 状态码与 body 一起看。
//!
//! 理由：中间件的正确性在「客户端收到什么」这个**边界**上才成立。
//! 只测内部函数，测的是「函数返回了我期望的东西」，
//! 而「响应真的是 401 且 body 是统一响应包」是**另一回事**。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::Extension;
use axum::extract::FromRequestParts;
use axum::extract::Request as AxumRequest;
use axum::http::{HeaderMap, Request as HttpRequest, StatusCode};
use axum::routing::get;
use nexus_api::middleware::{
    AuthenticatedIdentity, IdentityExtractor, REQUEST_ID_HEADER, RequestId, RequestIdExtractor,
    auth_rejection,
};
use nexus_core::AppError;
use nexus_core::id::UserId;
use serde_json::Value;
use tower::util::ServiceExt;

/// 一个**总是成功**的提取器。
struct AllowAll;

impl IdentityExtractor for AllowAll {
    fn extract(&self, _headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError> {
        Ok(AuthenticatedIdentity::new(Some(UserId::new(7)), "alice"))
    }
}

/// 一个**总是失败**的提取器 —— 失败时返回指定的业务码。
struct RejectWith(nexus_core::ErrorCode);

impl IdentityExtractor for RejectWith {
    fn extract(&self, _headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError> {
        Err(AppError::new(self.0, "凭证无效"))
    }
}

/// 一个**按请求头**决定成败的提取器 —— 用来验证「透传」路径。
struct RequireToken;

impl IdentityExtractor for RequireToken {
    fn extract(&self, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError> {
        match headers.get("authorization") {
            Some(value) if value.as_bytes() == b"Bearer good" => {
                Ok(AuthenticatedIdentity::new(Some(UserId::new(42)), "bob"))
            }
            _ => Err(AppError::new(
                nexus_core::ErrorCode::Unauthorized,
                "缺少凭证",
            )),
        }
    }
}

/// 造一个受保护的端点 —— handler 把身份**回显**出来。
fn protected_app(extractor: Arc<dyn IdentityExtractor>) -> Router {
    Router::new()
        .route(
            "/secret",
            get(
                |Extension(identity): Extension<AuthenticatedIdentity>| async move {
                    axum::Json(serde_json::json!({
                        "subject": identity.subject,
                        "user_id": identity.user_id.map(|id| id.get()),
                    }))
                },
            )
            .layer(axum::middleware::from_fn_with_state(
                extractor,
                identity_middleware_dyn,
            )),
        )
        .with_state(())
}

/// 接受**擦除类型**提取器的鉴权中间件。
///
/// # 为什么需要它
///
/// `from_fn_with_state` 的泛型 `E` 在这里**推断不出来**
/// —— 它需要**具体**类型。
///
/// # 逻辑**不在**这里重复实现
///
/// 委托给 [`nexus_api::middleware::authenticate`] ——
/// 逻辑**只有一处**，两处各写一遍必然分叉。
async fn identity_middleware_dyn(
    axum::extract::State(extractor): axum::extract::State<Arc<dyn IdentityExtractor>>,
    mut request: AxumRequest,
    next: axum::middleware::Next,
) -> axum::response::Response {
    match nexus_api::middleware::authenticate(&*extractor, &mut request) {
        None => next.run(request).await,
        Some(rejection) => rejection,
    }
}

/// 发一个请求并取回 (状态码, body JSON)。
async fn call(app: Router, path: &str) -> (StatusCode, Value) {
    let req = HttpRequest::builder()
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn a_failed_identity_extraction_is_rejected() {
    // 提取失败 → 401，handler **不会**被执行。
    let app = protected_app(Arc::new(RejectWith(nexus_core::ErrorCode::Unauthorized)));
    let (status, _) = call(app, "/secret").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_rejection_uses_the_unified_response_shape() {
    // 拒绝响应的 body 必须是**统一响应包** —— 与成功响应形状一致。
    let app = protected_app(Arc::new(RejectWith(nexus_core::ErrorCode::Unauthorized)));
    let (status, body) = call(app, "/secret").await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], serde_json::json!(20101));
    assert_eq!(body["message"], serde_json::json!("凭证无效"));

    // 失败响应**只有** code 与 message —— 没有 data，没有 cause。
    let obj = body.as_object().unwrap();
    assert_eq!(obj.len(), 2, "失败响应应只有两个字段，实际：{body}");
    assert!(!body.to_string().contains("cause"), "不得外泄内部原因");
}

#[tokio::test]
async fn a_forbidden_extraction_maps_to_403() {
    // 「已认证但无权访问」是 **403**，不是 401。
    let app = protected_app(Arc::new(RejectWith(nexus_core::ErrorCode::Forbidden)));
    let (status, body) = call(app, "/secret").await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], serde_json::json!(20201));
}

#[tokio::test]
async fn the_identity_reaches_the_handler() {
    // 提取成功 → 身份进 `Extension` → handler 能取用。
    let app = protected_app(Arc::new(AllowAll));
    let (status, body) = call(app, "/secret").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["subject"], serde_json::json!("alice"));
    assert_eq!(body["user_id"], serde_json::json!(7));
}

#[tokio::test]
async fn the_extractor_receives_the_request_headers() {
    // 提取器拿到的是**真实的请求头** —— 否则它没法校验凭证。
    let app = protected_app(Arc::new(RequireToken));

    // 没有 authorization 头 → 拒绝。
    let (status, _) = call(app.clone(), "/secret").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 带正确的凭证 → 放行。
    let req = HttpRequest::builder()
        .uri("/secret")
        .header("authorization", "Bearer good")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["subject"], serde_json::json!("bob"));
}

#[tokio::test]
async fn a_non_auth_error_code_is_rejected_with_500() {
    // 提取器返回非鉴权段的码 —— 那是**协议层自身**的内部问题。
    // 映射成 500（而不是 401）—— 否则客户端会盲目重试。
    let app = protected_app(Arc::new(RejectWith(nexus_core::ErrorCode::Internal)));
    let (status, body) = call(app, "/secret").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], serde_json::json!(90001));
}

#[tokio::test]
async fn the_rejection_response_has_a_json_content_type() {
    let resp = auth_rejection(AppError::new(nexus_core::ErrorCode::Unauthorized, "未认证"));
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        ct.starts_with("application/json"),
        "content-type 应为 application/json，实际：{ct}"
    );
}

// ═══════════════════════════════════════════════════════════
// 请求 ID
// ═══════════════════════════════════════════════════════════

/// 造一个回显请求 ID 的端点。
fn request_id_app() -> Router {
    Router::new()
        .route(
            "/echo",
            get(|req_id: RequestIdExtractor| async move {
                req_id
                    .0
                    .map(|id| id.as_str().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            }),
        )
        .layer(axum::middleware::from_fn(
            nexus_api::middleware::request_id_middleware,
        ))
        .with_state(())
}

/// 发一个请求并取回 (状态码, 响应头中的请求 ID, body 文本)。
async fn call_with_request_id(
    app: Router,
    path: &str,
    inbound: Option<&str>,
) -> (StatusCode, Option<String>, String) {
    let mut builder = HttpRequest::builder().uri(path);
    if let Some(value) = inbound {
        builder = builder.header(REQUEST_ID_HEADER, value);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let header_id = resp
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        header_id,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

#[tokio::test]
async fn a_request_id_is_generated_when_none_is_inbound() {
    let (status, header_id, body) = call_with_request_id(request_id_app(), "/echo", None).await;

    assert_eq!(status, StatusCode::OK);
    let id = header_id.expect("响应头应带请求 ID");
    assert_eq!(body, id, "handler 拿到的应与响应头**同一个** ID");
    assert_eq!(id.len(), 36, "生成的应是 UUID v4");
}

#[tokio::test]
async fn an_inbound_request_id_is_passed_through() {
    let (status, header_id, body) =
        call_with_request_id(request_id_app(), "/echo", Some("trace-abc-123")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        header_id.as_deref(),
        Some("trace-abc-123"),
        "上游的请求 ID 应被透传"
    );
    assert_eq!(body, "trace-abc-123");
}

#[tokio::test]
async fn a_malformed_inbound_request_id_is_replaced() {
    // ⚠️ 上游传来的头是**不可信输入** —— 它会被回显到响应头。
    // 含 CR / LF 的值会造成**响应拆分**。
    let (status, header_id, body) =
        call_with_request_id(request_id_app(), "/echo", Some("a b")).await;

    assert_eq!(status, StatusCode::OK);
    let id = header_id.expect("响应头应带请求 ID");
    assert_ne!(id, "a b", "非法的透传值必须被丢弃");
    assert_eq!(body, id, "handler 拿到的应与响应头**同一个** ID");
    assert_eq!(id.len(), 36, "非法值应被换成新生成的 UUID");
}

#[tokio::test]
async fn the_request_id_is_absent_when_the_middleware_is_not_mounted() {
    // 没挂请求 ID 中间件 → 提取器给出 `None`，**不**报错。
    let app = Router::new()
        .route(
            "/echo",
            get(|req_id: RequestIdExtractor| async move { req_id.0.is_none().to_string() }),
        )
        .with_state(());

    let (status, header_id, body) = call_with_request_id(app, "/echo", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(header_id.is_none(), "没挂中间件时响应头不应有请求 ID");
    assert_eq!(body, "true");
}

#[tokio::test]
async fn the_request_id_header_name_is_stable() {
    // 头名是**契约** —— 客户端与网关都依赖它。
    assert_eq!(REQUEST_ID_HEADER, "x-request-id");
}

#[tokio::test]
async fn the_request_id_extractor_reads_the_extension() {
    // 直接构造请求，验证提取器读的是 `Extension` 里的值。
    let (mut parts, _body) = HttpRequest::builder()
        .uri("/")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    parts.extensions.insert(RequestId::new("ext-id"));

    let extractor = RequestIdExtractor::from_request_parts(&mut parts, &())
        .await
        .expect("提取不应失败");
    assert_eq!(
        extractor.0.map(|id| id.as_str().to_string()),
        Some("ext-id".to_string())
    );
}
