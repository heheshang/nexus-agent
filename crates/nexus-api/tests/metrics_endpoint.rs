//! `/metrics` 端点的**行为层**测试。
//!
//! 这些测试断言的是**外部可见的行为**：
//! 请求 `/metrics` 能拿到可解析的 Prometheus 文本，且不含凭据。

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nexus_api::{MetricsState, metrics_router};
use nexus_observability::{LlmMetrics, RequestStatus};
use tower::ServiceExt;

/// 发一次 `GET /metrics`，返回状态码、Content-Type 与响应体。
async fn get_metrics() -> (StatusCode, String, String) {
    let router = metrics_router(MetricsState::new());
    let response = router
        .oneshot(
            Request::builder()
                // ⚠️ 断言**字面量**路径，不引用 `METRICS_PATH` ——
                // 引用常量就是**同义反复**：改了常量，两边同时变，测试永远绿。
                // 故障注入实测过：把路径改成 `/metrics2` 后，
                // 引用常量的断言仍然通过（没拦住）。
                .uri("/metrics")
                .body(Body::empty())
                .expect("构造请求不应失败"),
        )
        .await
        .expect("路由调用不应失败");

    let status = response.status();
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("读取响应体不应失败");

    (
        status,
        content_type,
        String::from_utf8(body.to_vec()).expect("响应体应是 UTF-8"),
    )
}

/// 校验一段文本是**可解析的** Prometheus 文本格式。
///
/// `prometheus` crate 只提供**编码器**，没有解码器 —— 所以这里手写一个最小校验器。
///
/// 它检查三件事：
/// 1. `# HELP` / `# TYPE` 行必须形如 `# TYPE <name> <type>`；
/// 2. 样本行必须是 `<name>{labels} <number>` 或 `<name> <number>`；
/// 3. 样本行的指标名必须**有**对应的 `# TYPE` 声明。
fn assert_parseable_prometheus_text(text: &str) {
    let mut declared: Vec<&str> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("# TYPE ") {
            let name = rest
                .split_whitespace()
                .next()
                .unwrap_or_else(|| panic!("# TYPE 行缺少指标名: {line}"));
            assert!(
                rest.split_whitespace().count() == 2,
                "# TYPE 行必须有名字与类型两个字段: {line}"
            );
            declared.push(name);
            continue;
        }

        if line.starts_with('#') {
            // `# HELP` 与其它注释行 —— 不做结构校验。
            continue;
        }

        // 样本行：`<name>{labels} <value>` 或 `<name> <value>`
        let (name, raw_value) = match line.split_once('{') {
            Some((name, rest)) => {
                let (labels, value) = rest
                    .split_once('}')
                    .unwrap_or_else(|| panic!("样本行的标签未闭合: {line}"));
                assert!(!labels.is_empty(), "样本行的标签集不应为空: {line}");
                (name, value)
            }
            None => {
                let (name, value) = line
                    .split_once(' ')
                    .unwrap_or_else(|| panic!("样本行缺少取值: {line}"));
                (name, value)
            }
        };

        let value = raw_value.trim();
        assert!(
            value.parse::<f64>().is_ok(),
            "样本行的取值应是数字，实际是 {value:?}: {line}"
        );
        assert!(
            declared.contains(&name),
            "样本行的指标 {name} 没有对应的 # TYPE 声明: {line}"
        );
    }
}

#[tokio::test]
async fn the_metrics_endpoint_returns_prometheus_text() {
    // 先产生一点流量 —— 指标**有样本**后才会出现在文本里。
    //
    // 为什么需要这一步：`Registry::gather()` 会**剪掉空 family**
    // （`prometheus` 的 `registry.rs` 里 `if mf.get_metric().is_empty() { continue }`）。
    // 标签是运行期才知道的（供应商代号来自配置），所以**没有流量时就没有样本**。
    let metrics = LlmMetrics::new();
    metrics.record_request("p-metrics", RequestStatus::Ok);
    metrics.record_tokens("p-metrics", 11, 22);

    let (status, content_type, body) = get_metrics().await;

    assert_eq!(status, StatusCode::OK, "/metrics 应返回 200");
    // ⚠️ 断言**字面量**，不引用 `METRICS_CONTENT_TYPE` ——
    // 引用常量就是**同义反复**：改了常量，两边同时变，测试永远绿。
    // 故障注入实测过：把常量改成 `text/html` 后，
    // 引用常量的断言仍然通过（FAILED ✗ —— 没拦住）。
    assert_eq!(
        content_type, "text/plain; version=0.0.4; charset=utf-8",
        "Content-Type 必须是 Prometheus 文本格式"
    );
    assert_parseable_prometheus_text(&body);
}

#[tokio::test]
async fn the_response_contains_both_metric_definitions() {
    let metrics = LlmMetrics::new();
    metrics.record_request("p-def", RequestStatus::Ok);
    metrics.record_tokens("p-def", 5, 7);

    let (_, _, body) = get_metrics().await;

    // AC #2：两个指标的**定义**都要在。
    assert!(
        body.contains("# TYPE llm_requests_total counter"),
        "缺少 llm_requests_total 的定义:\n{body}"
    );
    assert!(
        body.contains("# TYPE llm_tokens_total counter"),
        "缺少 llm_tokens_total 的定义:\n{body}"
    );
}

#[tokio::test]
async fn the_endpoint_requires_no_credentials() {
    // AC #3：不带任何 header / query / body，直接请求就能拿到 200。
    let (status, _, body) = get_metrics().await;

    assert_eq!(status, StatusCode::OK, "/metrics 不应要求任何凭据");
    assert!(!body.is_empty(), "响应体不应为空");
}

#[tokio::test]
async fn the_response_contains_no_credentials() {
    // AC #4：响应体**不得**出现凭据字段。
    let metrics = LlmMetrics::new();
    metrics.record_request("p-creds", RequestStatus::Ok);
    metrics.record_tokens("p-creds", 3, 4);

    let (_, _, body) = get_metrics().await;

    let lowered = body.to_lowercase();
    for forbidden in [
        "api_key",
        "apikey",
        "authorization",
        "bearer",
        "secret",
        "password",
    ] {
        assert!(
            !lowered.contains(forbidden),
            "响应体不应包含 {forbidden:?}:\n{body}"
        );
    }
}

#[tokio::test]
async fn the_cancelled_status_is_counted_separately() {
    // 端点必须反映 `nexus-observability` 的**全部**三个状态口径 ——
    // 取消单独计数是 P3 已定稿的口径（架构文档 §2.2.4.1）。
    let metrics = LlmMetrics::new();
    metrics.record_request("p-status-ok", RequestStatus::Ok);
    metrics.record_request("p-status-err", RequestStatus::Error);
    metrics.record_request("p-status-cancel", RequestStatus::Canceled);

    let (_, _, body) = get_metrics().await;

    assert!(body.contains(r#"status="ok""#), "缺少 ok 计数:\n{body}");
    assert!(
        body.contains(r#"status="error""#),
        "缺少 error 计数:\n{body}"
    );
    assert!(
        body.contains(r#"status="canceled""#),
        "缺少 canceled 计数:\n{body}"
    );
}

#[tokio::test]
async fn the_metrics_path_is_metrics() {
    // 端点路径是**契约** —— Prometheus 抓取配置里写死了它。
    // 断言字面量，不引用常量（同义反复的教训）。
    let router = metrics_router(MetricsState::new());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("构造请求不应失败"),
        )
        .await
        .expect("路由调用不应失败");

    assert_eq!(response.status(), StatusCode::OK, "路径 /metrics 必须可用");
}

#[tokio::test]
async fn an_unknown_path_returns_not_found() {
    let router = metrics_router(MetricsState::new());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/not-metrics")
                .body(Body::empty())
                .expect("构造请求不应失败"),
        )
        .await
        .expect("路由调用不应失败");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "未注册的路径应返回 404"
    );
}
