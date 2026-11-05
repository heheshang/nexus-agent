//! OpenAI 兼容供应商客户端。
//!
//! # 为什么只做 OpenAI 兼容协议
//!
//! deepseek / openai / qwen / zhipu / moonshot / doubao 都是同一套协议；
//! 本地 ollama 也走 `/v1` 兼容层。引入第二套协议形状（ollama 原生 `/api/chat`）
//! 在只有一个供应商实现时没有收益 —— 见 `docs/specs/p3/contracts/invariants.json` 决策 D2。
//!
//! # 客户端**不查库**
//!
//! `ModelDescriptor`（含解密后的 `api_key`）由调用方传入。
//! Rust 版用 **Cargo 依赖图**保证这一点：`nexus-llm` 的 `Cargo.toml` 不出现 `nexus-repo`。
//!
//! # 凭据纪律
//!
//! `api_key` 是 [`secrecy::SecretString`] —— 只在**真正要发请求**时
//! [`ModelDescriptor::expose_api_key`] 取用，绝不打印、绝不入错误消息。
//!
//! # 错误分类
//!
//! - HTTP 5xx / 429 → 可回退（`ModelUnavailable` / `RateLimited`）
//! - HTTP 4xx（非 429）→ **不回退**（上游明确拒绝，重试无意义）
//! - 响应体解析失败 → `Malformed` → `ErrorCode::Internal`（**Terminal**，不重试）
//! - 超时 → `ModelTimeout` → `UnknownOutcome`（**结果未知**，不自动重试）

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;

use crate::convert::build_request_body;
use crate::error::LlmError;
use crate::protocol::OpenAiChatResponse;
use crate::sse;
use crate::types::{ChatRequest, ChatResponse, ModelDescriptor, StreamEvent};

/// 供应商客户端配置。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderConfig {
    /// 单次请求超时。
    pub timeout: Duration,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
        }
    }
}

/// 供应商事件流 —— 上游 SSE 翻译后的流。
pub type ProviderStream = futures::stream::BoxStream<'static, Result<StreamEvent, LlmError>>;

/// OpenAI 兼容客户端。
///
/// # 线程安全
///
/// `reqwest::Client` 内部是连接池，**克隆共享同一份连接池** —— 这正是想要的语义。
#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    config: ProviderConfig,
}

impl OpenAiProvider {
    /// 创建客户端。
    pub fn new(config: ProviderConfig) -> Result<Self, LlmError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| LlmError::UpstreamIo(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// 用默认配置创建客户端。
    pub fn with_defaults() -> Result<Self, LlmError> {
        Self::new(ProviderConfig::default())
    }

    /// 客户端配置。
    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// 非流式对话。
    ///
    /// # 参数
    ///
    /// - `max_tokens` —— **已裁剪过的**输出上限（来自预算裁剪）。
    ///
    /// # 契约影响五问
    ///
    /// 本方法**不新增**任何业务码 —— 全部复用 `nexus-core` 已定稿的码。
    pub async fn chat(
        &self,
        desc: &ModelDescriptor,
        req: &ChatRequest,
        max_tokens: i32,
    ) -> Result<ChatResponse, LlmError> {
        let url = chat_completions_url(&desc.base_url)?;
        let body = build_request_body(desc, req, false, max_tokens);

        let response = self
            .client
            .post(url)
            .bearer_auth(desc.expose_api_key())
            .json(&body)
            .send()
            .await
            .map_err(map_send_error)?;

        let status = response.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16()));
        }

        let raw: OpenAiChatResponse = response.json().await.map_err(map_parse_error)?;
        raw.into_chat_response()
    }

    /// 流式对话 —— 返回 SSE 事件流。
    ///
    /// # 返回的是**流本身**，不是收集好的 `Vec`
    ///
    /// 流式回退边界的核心是「**首事件之前**」这个时间点 ——
    /// 一旦把流收集成 `Vec`，边界就消失了。
    ///
    /// 本方法只负责「**把连接建起来**」。建流失败（连接失败、非 2xx）
    /// 返回 `Err` —— 那时**还没有任何事件发出**，回退是允许的。
    /// 建流之后流中途的失败，由调用方（网关）处理。
    pub async fn chat_stream(
        &self,
        desc: &ModelDescriptor,
        req: &ChatRequest,
        max_tokens: i32,
    ) -> Result<ProviderStream, LlmError> {
        let url = chat_completions_url(&desc.base_url)?;
        let body = build_request_body(desc, req, true, max_tokens);

        let response = self
            .client
            .post(url)
            .bearer_auth(desc.expose_api_key())
            .json(&body)
            .send()
            .await
            .map_err(map_send_error)?;

        let status = response.status();
        if !status.is_success() {
            return Err(map_status(status.as_u16()));
        }

        // 心跳/空帧（`Ok(None)`）被**过滤掉** —— 它们不是事件。
        let stream = response
            .bytes_stream()
            .eventsource()
            .filter_map(|event| async move {
                match event {
                    Ok(e) => match sse::parse(&e) {
                        Ok(Some(event)) => Some(Ok(event)),
                        Ok(None) => None,
                        Err(err) => Some(Err(err)),
                    },
                    Err(err) => Some(Err(map_eventsource_error(err))),
                }
            })
            .boxed();

        Ok(stream)
    }
}

/// 拼接 chat completions 的 URL。
///
/// # 兼容带 `/v1` 与不带 `/v1` 的 `base_url`
///
/// OpenAI 兼容供应商的 `base_url` 惯例是 `https://host/v1`。
/// 但也有配置写成 `https://host` —— 这时补上 `/v1`。
/// 已带 `/v1` 的**不重复追加**。
fn chat_completions_url(base_url: &str) -> Result<String, LlmError> {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(LlmError::Malformed("base_url 为空".to_string()));
    }

    let with_version = if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    };

    Ok(format!("{with_version}/chat/completions"))
}

/// 把 `reqwest` 的发送错误映射成 [`LlmError`]。
///
/// # 超时**必须**映射成 `Timeout`
///
/// `reqwest` 的超时错误与连接错误是同一个类型 ——
/// 但它们的**失败语义完全不同**：
///
/// - 超时 → `ModelTimeout` → `UnknownOutcome` → **不重试**
///   （请求可能已到达上游并计费，重试会重复扣费）
/// - 连接失败 → `ModelUnavailable` → `Retryable` → 可重试
///   （请求根本没发出去）
fn map_send_error(err: reqwest::Error) -> LlmError {
    if err.is_timeout() {
        LlmError::Timeout
    } else if err.is_connect() {
        LlmError::Upstream { status: 502 }
    } else {
        LlmError::UpstreamIo(err.to_string())
    }
}

/// 把 `eventsource-stream` 的错误映射成 [`LlmError`]。
///
/// # 传输层错误 vs 解析层错误
///
/// - `Transport` —— 下游 IO 失败 → `UpstreamIo`（可重试）。
/// - `Utf8` / `Parser` —— **响应体本身**坏了 → `Malformed`
///   （`ErrorCode::Internal`，Terminal —— 重试同样的坏响应没有意义）。
fn map_eventsource_error(err: eventsource_stream::EventStreamError<reqwest::Error>) -> LlmError {
    use eventsource_stream::EventStreamError;

    match err {
        EventStreamError::Transport(e) => map_send_error(e),
        EventStreamError::Utf8(e) => LlmError::Malformed(format!("响应体不是合法 UTF-8: {e}")),
        EventStreamError::Parser(e) => LlmError::Malformed(format!("SSE 分帧失败: {e}")),
    }
}

/// 把响应体解析错误映射成 [`LlmError::Malformed`]。
///
/// 解析失败 → `ErrorCode::Internal`（**Terminal**）——
/// 重试同样的坏响应没有意义。
fn map_parse_error(err: reqwest::Error) -> LlmError {
    LlmError::Malformed(format!("响应体解析失败: {err}"))
}

/// 把 HTTP 状态码映射成 [`LlmError`]。
///
/// # 分类规则
///
/// - 429 → `RateLimited`（可重试）
/// - 5xx → `Upstream`（可重试）
/// - 其他 4xx → **不回退**（上游明确拒绝，重试与回退都没有意义）
///
/// 「其他 4xx 不回退」是本实现相对参考实现的**收紧** ——
/// 参考实现只区分 504 与其他，把 400（请求非法）也当成可回退，
/// 结果是对着一个永远会拒绝的供应商反复重试。
fn map_status(status: u16) -> LlmError {
    match status {
        429 => LlmError::RateLimited { retry_after: None },
        500..=599 => LlmError::Upstream { status },
        _ => LlmError::Upstream { status },
    }
}
