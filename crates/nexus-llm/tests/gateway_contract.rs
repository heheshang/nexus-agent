//! 网关的**行为层**测试 —— 回退边界（架构文档 §3.3）。
//!
//! # 检验的设计条款
//!
//! | 测试 | 对应不变量 |
//! |---|---|
//! | `fallback_is_used_when_the_primary_fails` | P3-I06 |
//! | `no_fallback_when_the_provider_is_the_same` | P3-I08 |
//! | `the_error_mentions_both_providers` | P3-I09 |
//! | `the_stream_fails_before_the_first_event_can_fall_back` | P3-I06 |
//! | `the_stream_cannot_fall_back_after_the_first_event` | P3-I07 |

mod support;

use futures::StreamExt;
use nexus_llm::gateway::StreamOutcome;
use nexus_llm::types::{ChatRequest, Message, ModelDescriptor, Role};
use nexus_llm::{CallTarget, Gateway, LlmError, OpenAiProvider};
use support::mock_server::{MockBehavior, MockServer};

use tokio_util::sync::CancellationToken;

/// 构造一个指向 mock server 的描述符。
fn descriptor(base_url: &str, provider_code: &str) -> ModelDescriptor {
    let mut desc = ModelDescriptor::new(provider_code, base_url, "sk-test", "test-model");
    desc.context_window = 4096;
    desc.max_output_tokens = 1024;
    desc.max_tokens = 1024;
    desc
}

/// 一个简单的请求。
fn request() -> ChatRequest {
    ChatRequest {
        messages: vec![Message::new(Role::User, "hi")],
        tools: vec![],
        temperature: 0.7,
        top_p: 1.0,
        max_tokens: 0,
        stream: false,
    }
}

/// 成功的非流式响应体。
fn ok_body() -> String {
    serde_json::json!({
        "model": "test-model",
        "choices": [{
            "message": {"role": "assistant", "content": "hello"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 3, "completion_tokens": 5}
    })
    .to_string()
}

#[tokio::test]
async fn a_successful_call_returns_the_response() {
    let server = MockServer::start(MockBehavior::Json {
        status: 200,
        body: ok_body(),
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());
    let target = CallTarget::primary(descriptor(&server.base_url, "primary"));
    let cancel = CancellationToken::new();

    let response = gateway.chat(&target, &request(), &cancel).await.unwrap();

    assert_eq!(response.content, "hello");
    assert_eq!(response.usage.prompt_tokens, 3);
    assert_eq!(response.usage.completion_tokens, 5);
}

#[tokio::test]
async fn fallback_is_used_when_the_primary_fails() {
    let bad = MockServer::start(MockBehavior::Json {
        status: 500,
        body: "boom".to_string(),
    })
    .await;
    let good = MockServer::start(MockBehavior::Json {
        status: 200,
        body: ok_body(),
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());
    let target = CallTarget::with_fallback(
        descriptor(&bad.base_url, "primary"),
        descriptor(&good.base_url, "fallback"),
    );
    let cancel = CancellationToken::new();

    let response = gateway.chat(&target, &request(), &cancel).await.unwrap();

    assert_eq!(response.content, "hello");
    assert_eq!(
        bad.received_bodies().len(),
        3,
        "primary 应按重试配置尝试 3 次"
    );
    assert_eq!(good.received_bodies().len(), 1, "fallback 应被调用一次");
}

#[tokio::test]
async fn no_fallback_when_the_provider_is_the_same() {
    // 同一个供应商切两次没有意义 —— 失败原因不会变。
    let server = MockServer::start(MockBehavior::Json {
        status: 500,
        body: "boom".to_string(),
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());
    let target = CallTarget::with_fallback(
        descriptor(&server.base_url, "same"),
        descriptor(&server.base_url, "same"),
    );
    let cancel = CancellationToken::new();

    let result = gateway.chat(&target, &request(), &cancel).await;

    assert!(result.is_err(), "同供应商失败应直接报错");
    assert_eq!(
        server.received_bodies().len(),
        3,
        "同供应商不应触发第二轮 —— 只应有 primary 的 3 次重试"
    );
}

#[tokio::test]
async fn the_error_mentions_both_providers() {
    let primary = MockServer::start(MockBehavior::Json {
        status: 500,
        body: "boom".to_string(),
    })
    .await;
    let fallback = MockServer::start(MockBehavior::Json {
        status: 503,
        body: "boom".to_string(),
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());
    let target = CallTarget::with_fallback(
        descriptor(&primary.base_url, "primary-provider"),
        descriptor(&fallback.base_url, "fallback-provider"),
    );
    let cancel = CancellationToken::new();

    let err = gateway
        .chat(&target, &request(), &cancel)
        .await
        .unwrap_err();

    let LlmError::BothFailed { primary, fallback } = err else {
        panic!("应返回 BothFailed，实际：{err:?}");
    };
    assert!(
        primary.contains("primary-provider"),
        "错误信息应含 primary：{primary}"
    );
    assert!(
        fallback.contains("fallback-provider"),
        "错误信息应含 fallback：{fallback}"
    );
    // 两边各自的**失败原因**也必须在内 —— 只报代号等于没报。
    assert!(
        primary.contains("500"),
        "primary 的失败原因应在内：{primary}"
    );
    assert!(
        fallback.contains("503"),
        "fallback 的失败原因应在内：{fallback}"
    );
}

#[tokio::test]
async fn the_stream_fails_before_the_first_event_can_fall_back() {
    // 建流失败（连接建立阶段就 500）→ 还没有任何事件发出 → 允许回退。
    let bad = MockServer::start(MockBehavior::Json {
        status: 500,
        body: "boom".to_string(),
    })
    .await;
    let good = MockServer::start(MockBehavior::Sse {
        status: 200,
        frames: vec![sse_frame("hello")],
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());

    let outcome = gateway
        .chat_stream(&descriptor(&bad.base_url, "primary"), &request())
        .await;

    assert!(
        matches!(outcome, StreamOutcome::FailedBeforeStart(_)),
        "建流失败应返回 FailedBeforeStart —— 此时可回退"
    );

    // 回退：换供应商重新建流。
    let outcome = gateway
        .chat_stream(&descriptor(&good.base_url, "fallback"), &request())
        .await;

    let StreamOutcome::Stream(handle) = outcome else {
        panic!("fallback 应成功建流");
    };
    assert_eq!(handle.provider_code, "fallback");

    let mut stream = handle.stream;
    let first = stream
        .next()
        .await
        .expect("应有一个事件")
        .expect("不应是错误");
    assert_eq!(first.content, "hello");
}

#[tokio::test]
async fn the_stream_cannot_fall_back_after_the_first_event() {
    // 流已建立并发出首事件 → **不可回退**。
    let server = MockServer::start(MockBehavior::Sse {
        status: 200,
        frames: vec![sse_frame("first"), sse_frame("second")],
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());

    let outcome = gateway
        .chat_stream(&descriptor(&server.base_url, "primary"), &request())
        .await;

    let StreamOutcome::Stream(handle) = outcome else {
        panic!("应成功建流");
    };

    let mut stream = handle.stream;
    // 消费首事件 —— 这是「流已开始」的时刻。
    let first = stream
        .next()
        .await
        .expect("应有一个事件")
        .expect("不应是错误");
    assert_eq!(first.content, "first");

    // 首事件之后：流只能继续，不能换成另一个供应商。
    // 客户端已收到一半内容，切换模型会产生内容拼接错乱。
    let second = stream
        .next()
        .await
        .expect("应有两个事件")
        .expect("不应是错误");
    assert_eq!(second.content, "second");
}

#[tokio::test]
async fn the_budget_is_recomputed_on_every_attempt() {
    // 每次尝试都重跑预算裁剪 —— 参考实现的明确不变量。
    let server = MockServer::start(MockBehavior::Json {
        status: 200,
        body: ok_body(),
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());
    let target = CallTarget::primary(descriptor(&server.base_url, "primary"));
    let cancel = CancellationToken::new();

    let _ = gateway.chat(&target, &request(), &cancel).await.unwrap();

    let bodies = server.received_bodies();
    assert_eq!(bodies.len(), 1);
    let body: serde_json::Value = serde_json::from_str(&bodies[0]).expect("body 应是合法 JSON");
    assert_eq!(body["max_tokens"], 1024, "裁剪后的 max_tokens 应写入请求体");
}

/// 构造一个 SSE 帧。
fn sse_frame(content: &str) -> String {
    let payload = serde_json::json!({
        "model": "test-model",
        "choices": [{
            "delta": {"content": content},
            "finish_reason": null
        }]
    });
    format!("data: {payload}\n\n")
}
