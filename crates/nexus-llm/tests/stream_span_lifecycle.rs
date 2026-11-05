//! 网关的**流式 span 绑定流**测试 —— 架构文档 §2.4 关键不变量 2。
//!
//! # 为什么这条测试**单独一个二进制**
//!
//! `tracing` 把「这个 callsite 要不要记录」**全局缓存**在 callsite 注册表里。
//! 它在 callsite **第一次**被注册时算一次，之后就不再重算。
//!
//! 后果：如果**同一个测试二进制**里有别的测试先跑过 `chat_stream`，
//! 那时**还没有** subscriber —— callsite 的 interest 被缓存成 `never`，
//! 之后本测试装的 subscriber **看不到**任何 span，
//! `on_new_span` / `on_close` **永远不触发** —— 测试**随机失败**。
//!
//! `rebuild_interest_cache` 救不了：它只对**已注册的全局 dispatcher** 生效，
//! 而线程局部的 `set_default` 不在其中。
//!
//! 所以本测试：
//!
//! 1. **独占**一个测试二进制 —— 没有任何别的测试会先注册 callsite；
//! 2. 在**任何**业务调用**之前**装上**全局** subscriber ——
//!    callsite 注册时看到的就已经是它。
//!
//! # 为什么必须用**全局** subscriber 而不用 `set_default`
//!
//! `set_default` 设的是**线程局部** dispatcher —— 只有当前线程能看见。
//!
//! 而本测试里：
//!
//! 1. `tokio::spawn` 起的 mock server 任务在**别的线程**；
//! 2. span 的最后一个句柄（流被 drop 时）可能在**别的线程**被释放 ——
//!    而 `on_close` 是在**释放的那个线程**上触发的。
//!
//! 线程局部 subscriber 在那些线程上**看不见** —— 于是回调随机不触发。
//!
//! 全局 subscriber 没有这个问题。

// ⚠️ `support` 模块里的 `Json` 变体与 `received_bodies` 在本二进制里**用不到** ——
//
// 支撑模块被**每个**测试二进制**单独编译**一份。
// 本二进制只测流式，所以 `Json` 分支是死的 ——
// 加 `allow(dead_code)` 是因为「另一个二进制用它」**不构成**本二进制的使用。
#[allow(dead_code)]
mod support;

use std::sync::{Arc, Mutex};

use futures::StreamExt;
use nexus_llm::gateway::StreamOutcome;
use nexus_llm::types::{ChatRequest, Message, ModelDescriptor, Role};
use nexus_llm::{Gateway, OpenAiProvider};
use support::mock_server::{MockBehavior, MockServer};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

/// 记录 span 关闭事件的 layer。
#[derive(Debug, Clone, Default)]
struct CloseRecorder(Arc<Mutex<Vec<String>>>);

impl<S> Layer<S> for CloseRecorder
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(span.name().to_string());
        }
    }
}

/// 构造一个指向 mock server 的描述符。
fn descriptor(base_url: &str, provider_code: &str) -> ModelDescriptor {
    let mut desc = ModelDescriptor::new(provider_code, base_url, "sk-test", "test-model");
    desc.context_window = 4096;
    desc.max_output_tokens = 1024;
    desc.max_tokens = 1024;
    desc
}

/// 构造一个最小的请求。
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

/// 一个完整的 SSE 帧。
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

/// 网关的 span **随流结束**，而不是随「拿到流」结束。
///
/// # 检验的不变量
///
/// 架构文档 §2.4「关键不变量」第 2 条：
///
/// > **span 生命周期绑定流**：`spanOnClose` 把 span 的结束推迟到事件流关闭。
/// > 注释原文：「若放进 `runWithGuard`，拿到通道就返回，span 只有几毫秒，等于没测」。
///
/// # 为什么这条断言在**网关层**
///
/// `nexus-observability` 的 `wrap` 消耗 `self` —— 那是**类型**保证。
///
/// 但真正的风险在**网关怎么用**它：如果网关在别处**另外留一份** span 克隆，
/// 引用计数就降不到 0 —— span 永远不会关闭，
/// 而 `nexus-observability` 的单元测试**照样绿**。
///
/// 所以这条测试必须从**网关的公开 API** 出发。
#[tokio::test]
async fn the_gateway_span_closes_when_the_stream_is_consumed() {
    let recorder = CloseRecorder::default();
    let subscriber = tracing_subscriber::registry().with(recorder.clone());

    // ⚠️ 必须在**任何**业务调用**之前**装上 ——
    // 否则 callsite 的 interest 已经被缓存成 `never`（见模块文档）。
    let _ = tracing::subscriber::set_global_default(subscriber);

    let server = MockServer::start(MockBehavior::Sse {
        status: 200,
        frames: vec![sse_frame("hello")],
    })
    .await;
    let gateway = Gateway::with_defaults(OpenAiProvider::with_defaults().unwrap());

    let outcome = gateway
        .chat_stream(&descriptor(&server.base_url, "primary"), &request())
        .await;

    let StreamOutcome::Stream(handle) = outcome else {
        panic!("应成功建流");
    };

    // 建流之后 —— span **不应**已经关闭。
    // 那样 span 只覆盖了「建流」的几毫秒，等于没测。
    let closed = recorder.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(
        !closed.iter().any(|n| n == "llm.stream"),
        "建流后 span 不应关闭，实际关闭的：{closed:?}"
    );

    // 消费完整个流。
    let mut stream = handle.stream;
    while let Some(event) = stream.next().await {
        let _ = event.expect("不应是错误");
    }
    drop(stream);

    // 流消费完 —— span 应关闭。
    let closed = recorder.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(
        closed.iter().any(|n| n == "llm.stream"),
        "流消费完后 span 应关闭，实际关闭的：{closed:?}"
    );
}
