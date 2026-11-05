//! 统一响应包的行为层测试。
//!
//! 验证的是**真实 HTTP 响应** —— 状态码与 body 一起看。
//!
//! 理由：响应包的正确性在「客户端收到什么」这个**边界**上才成立。

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use nexus_api::error::ApiError;
use nexus_api::response::{ApiResponse, SUCCESS_CODE};
use nexus_core::ErrorCode;
use serde_json::Value;
use tower::util::ServiceExt;

/// 发一个请求并取回 (状态码, body JSON)。
async fn call(app: Router, path: &str) -> (StatusCode, Value) {
    let req = Request::builder().uri(path).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// 造一个成功端点。
fn success_app() -> Router {
    Router::new().route(
        "/ok",
        get(|| async { axum::Json(ApiResponse::ok_with(serde_json::json!({"id": 7}))) }),
    )
}

/// 造一个失败端点。
fn failure_app() -> Router {
    Router::new().route(
        "/fail",
        get(|| async {
            Err::<axum::Json<Value>, ApiError>(ApiError::business(
                ErrorCode::Unauthorized,
                "未认证",
            ))
        }),
    )
}

#[tokio::test]
async fn a_success_response_carries_the_numeric_zero_code() {
    let (status, body) = call(success_app(), "/ok").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], serde_json::json!(0));
    assert_eq!(body["code"].as_i64().unwrap(), SUCCESS_CODE as i64);
    assert_eq!(body["message"], "ok");
}

#[tokio::test]
async fn a_failure_response_carries_the_business_code() {
    let (status, body) = call(failure_app(), "/fail").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], serde_json::json!(20101));
    assert_eq!(body["message"], "未认证");
}

#[tokio::test]
async fn the_http_status_matches_the_business_code() {
    // 逐个核对：状态码必须与业务码的 `http_status()` 一致。
    let cases = [
        (ErrorCode::InvalidParam, StatusCode::BAD_REQUEST),
        (ErrorCode::Unauthorized, StatusCode::UNAUTHORIZED),
        (ErrorCode::Forbidden, StatusCode::FORBIDDEN),
        (ErrorCode::RateLimited, StatusCode::TOO_MANY_REQUESTS),
        (ErrorCode::Internal, StatusCode::INTERNAL_SERVER_ERROR),
        (ErrorCode::NotImplemented, StatusCode::NOT_IMPLEMENTED),
        (
            ErrorCode::ServiceUnavailable,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ];

    for (code, expected) in cases {
        let app = Router::new().route(
            "/fail",
            get(move || async move {
                Err::<axum::Json<Value>, ApiError>(ApiError::business(code, "x"))
            }),
        );
        let (status, body) = call(app, "/fail").await;
        assert_eq!(status, expected, "业务码 {code:?} 的 HTTP 状态不对");
        assert_eq!(body["code"], serde_json::json!(code.code()));
    }
}

#[tokio::test]
async fn the_response_body_carries_no_internal_cause() {
    let app = Router::new().route(
        "/fail",
        get(|| async {
            Err::<axum::Json<Value>, ApiError>(ApiError::internal("PG 连接串 password=hunter2"))
        }),
    );
    let (status, body) = call(app, "/fail").await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    let text = body.to_string();
    assert!(!text.contains("hunter2"), "内部原因不得外泄：{text}");
    assert!(!text.contains("password"), "内部原因不得外泄：{text}");
    assert!(!text.contains("cause"), "内部原因不得外泄：{text}");
    // 但**业务码**在
    assert_eq!(body["code"], serde_json::json!(90001));
}

#[tokio::test]
async fn an_absent_data_field_is_omitted() {
    let app = Router::new().route("/ok", get(|| async { axum::Json(ApiResponse::ok()) }));
    let (status, body) = call(app, "/ok").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.get("data").is_none(),
        "data 缺省时应省略，实际：{body}"
    );
}

#[tokio::test]
async fn the_error_body_has_exactly_two_fields() {
    // 失败响应**只有** code 与 message —— 没有 data，没有 cause。
    let app = Router::new().route(
        "/fail",
        get(|| async {
            Err::<axum::Json<Value>, ApiError>(ApiError::business(ErrorCode::Internal, "x"))
        }),
    );
    let (_, body) = call(app, "/fail").await;
    let obj = body.as_object().unwrap();
    assert_eq!(obj.len(), 2, "失败响应应只有两个字段，实际：{body}");
    assert!(obj.contains_key("code"));
    assert!(obj.contains_key("message"));
}

#[tokio::test]
async fn the_content_type_is_json() {
    let app = Router::new().route(
        "/fail",
        get(|| async {
            Err::<axum::Json<Value>, ApiError>(ApiError::business(ErrorCode::Internal, "x"))
        }),
    );
    let req = Request::builder().uri("/fail").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
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
