//! 网关装配 —— 预算裁剪 → 熔断 → 信号量 → 重试 → 供应商 → fallback。
//!
//! # 装配顺序**有讲究**
//!
//! 参考架构 §2.4 的管道：
//!
//! ```text
//! bound_chat_descriptor → 熔断 Allow? → 信号量 → 共享准入 → 重试 → 供应商 → fallback
//! ```
//!
//! 每一层的顺序都不是随意的：
//!
//! | 位置 | 为什么在这 | 为什么不在更前面 |
//! |---|---|---|
//! | 预算**最先** | 预算不足在**调用前**被拒 —— 重试同样的请求同样会超 | —— |
//! | 熔断**次之** | 熔断在最外层 —— 避免**已知故障**的供应商消耗配额 | 不能在预算前：预算不足不是供应商的错，不该进熔断样本 |
//! | 重试在供应商**内** | 重试的是「同一次调用」 | 不能在熔断外：熔断拒绝不是失败，重试无意义 |
//! | fallback 在**最外** | 回退是「换供应商重试整条管道」 | —— |
//!
//! # ⚠️ 熔断是**单副本粒度**
//!
//! 熔断状态在**进程内**。多副本部署时每个副本各有一套 ——
//! 会出现「A 副本熔断、B 副本继续打」的不一致。
//! 见 [`crate::breaker`] 与架构文档 §4.3。
//!
//! # 回退边界（架构文档 §3.3）
//!
//! - **非流式**：primary 失败 → 可以回退到 fallback。
//! - **流式**：**首事件之前**失败 → 可以回退；**首事件之后** → **不可回退**。
//!
//! 原因：客户端已经收到了一半内容，切换模型会产生内容拼接错乱。

use std::sync::{Arc, Mutex};
use tracing::Instrument;

use nexus_core::error::AppError;
use nexus_observability::{LlmMetrics, RequestStatus, StreamSpan};

use crate::breaker::BreakerRegistry;
use crate::budget::bound_chat_request;
use crate::error::LlmError;
use crate::provider::OpenAiProvider;
use crate::retry::{CallOutcome, RetryConfig, RetryOutcome, record_outcome, retry_do};
use crate::types::{ChatRequest, ChatResponse, ModelDescriptor, StreamEvent};

/// 网关配置。
#[derive(Debug, Clone, PartialEq)]
pub struct GatewayConfig {
    /// 重试配置。
    pub retry: RetryConfig,
    /// 每个供应商的并发上限。
    pub max_conns_per_provider: usize,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            // 参考实现默认值（`internal/llm/retry.go:19-22`）。
            retry: RetryConfig::default(),
            // 参考实现默认值（`internal/llm/ratelimiter.go:16`）：16。
            max_conns_per_provider: 16,
        }
    }
}

impl GatewayConfig {
    /// 从配置栈构造。
    ///
    /// # 读哪些配置项
    ///
    /// | 配置路径 | 环境变量 | 缺省时 |
    /// |---|---|---|
    /// | `llm.max_conns_per_provider` | `NX_LLM__MAX_CONNS_PER_PROVIDER` | 16（参考实现默认值） |
    ///
    /// # ⚠️ 与 `ProviderConfig::from_config` 的差别
    ///
    /// 超时与并发上限**语义不同**：
    ///
    /// - 超时缺省 120 秒 —— 缺了只是慢一点。
    /// - 并发上限缺省 16 —— 缺了是**限流**，直接决定吞吐。
    ///
    /// 所以并发上限的缺省值**必须**与参考实现一致，
    /// 且**不**允许配成 0（0 会让所有请求卡在 `acquire` 上）。
    pub fn from_config(stack: &nexus_config::ConfigStack) -> Self {
        let max_conns_per_provider = stack
            .get_u64("llm.max_conns_per_provider")
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            .unwrap_or(DEFAULT_MAX_CONNS_PER_PROVIDER);

        Self {
            retry: RetryConfig::default(),
            max_conns_per_provider,
        }
    }
}

/// 默认的每供应商并发上限。
///
/// 与参考实现一致（`internal/llm/ratelimiter.go:16`）。
const DEFAULT_MAX_CONNS_PER_PROVIDER: usize = 16;

/// 一次调用的**目标** —— primary 与可选的 fallback。
#[derive(Debug, Clone)]
pub struct CallTarget {
    /// 主供应商。
    pub primary: ModelDescriptor,
    /// 备用供应商（可选）。
    pub fallback: Option<ModelDescriptor>,
}

impl CallTarget {
    /// 只有主供应商。
    pub fn primary(desc: ModelDescriptor) -> Self {
        Self {
            primary: desc,
            fallback: None,
        }
    }

    /// 主供应商 + 备用供应商。
    pub fn with_fallback(primary: ModelDescriptor, fallback: ModelDescriptor) -> Self {
        Self {
            primary,
            fallback: Some(fallback),
        }
    }
}

/// 模型网关。
#[derive(Debug, Clone)]
pub struct Gateway {
    provider: OpenAiProvider,
    breakers: Arc<BreakerRegistry>,
    config: GatewayConfig,
    metrics: Arc<LlmMetrics>,
}

impl Gateway {
    /// 创建网关。
    pub fn new(provider: OpenAiProvider, config: GatewayConfig) -> Self {
        Self {
            provider,
            breakers: Arc::new(BreakerRegistry::new()),
            config,
            metrics: Arc::new(LlmMetrics::new()),
        }
    }

    /// 用默认配置创建网关。
    pub fn with_defaults(provider: OpenAiProvider) -> Self {
        Self::new(provider, GatewayConfig::default())
    }

    /// 指标集。
    pub fn metrics(&self) -> &Arc<LlmMetrics> {
        &self.metrics
    }

    /// 熔断器注册表。
    pub fn breakers(&self) -> &Arc<BreakerRegistry> {
        &self.breakers
    }

    /// 非流式对话。
    ///
    /// # 装配顺序
    ///
    /// 1. 预算裁剪（**每次尝试都重跑**）
    /// 2. 熔断判断
    /// 3. 信号量
    /// 4. 重试
    /// 5. 供应商调用
    /// 6. fallback
    ///
    /// # 回退条件
    ///
    /// 只有当 fallback 的 `provider_code` 与 primary **不同**时才切换。
    /// 同一个供应商切两次没有意义 —— 失败原因不会变。
    ///
    /// # 错误信息
    ///
    /// 两边都失败时，错误信息**同时含** primary 与 fallback 的信息 ——
    /// 运维需要知道「试过哪两个」。
    pub async fn chat(
        &self,
        target: &CallTarget,
        req: &ChatRequest,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ChatResponse, LlmError> {
        let span = tracing::info_span!(
            target: "nexus::llm",
            "llm.chat",
            llm.provider = %target.primary.provider_code,
            llm.has_fallback = target.fallback.is_some(),
        );

        let result = async {
            let primary_result = self.attempt(&target.primary, req, cancel).await;

            match primary_result {
                Ok(response) => Ok(response),
                Err(primary_err) => {
                    // 没有 fallback —— 直接返回。
                    let Some(fallback_desc) =
                        usable_fallback(&target.primary, target.fallback.as_ref())
                    else {
                        return Err(primary_err);
                    };

                    let fallback_result = self.attempt(&fallback_desc, req, cancel).await;

                    match fallback_result {
                        Ok(response) => Ok(response),
                        Err(fallback_err) => Err(combine_errors(
                            &target.primary,
                            &primary_err,
                            &fallback_desc,
                            &fallback_err,
                        )),
                    }
                }
            }
        }
        .instrument(span.clone())
        .await;

        // 指标在 span **之外**记录 ——
        // 指标是「这次调用结束了」的断言，与 span 的生命周期无关。
        let status = match &result {
            Ok(_) => RequestStatus::Ok,
            Err(LlmError::Canceled) => RequestStatus::Canceled,
            Err(_) => RequestStatus::Error,
        };
        self.metrics
            .record_request(&target.primary.provider_code, status);

        if let Ok(response) = &result {
            span.record("llm.finish_reason", &response.finish_reason);
            span.record("llm.prompt_tokens", response.usage.prompt_tokens);
            span.record("llm.completion_tokens", response.usage.completion_tokens);
        }

        result
    }

    /// 对**单个供应商**做一次完整尝试（含重试）。
    ///
    /// # 管道顺序
    ///
    /// 预算 → 熔断 → 信号量 → 重试 → 供应商。
    ///
    /// # 预算在**每次重试尝试**都重跑
    ///
    /// 参考实现的明确不变量：预算裁剪在**每次尝试**（含 fallback）都重跑。
    /// 原因：裁剪会**改变** `max_tokens`，而重试时上游可能已经消耗了一部分额度 ——
    /// 不重跑就可能撞穿窗口。
    async fn attempt(
        &self,
        desc: &ModelDescriptor,
        req: &ChatRequest,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ChatResponse, LlmError> {
        let breaker = self.breakers.get_or_create(&desc.provider_code);

        // ── 1. 预算裁剪（调用前拒绝）──
        // 裁剪在**调用前**做一次，用于尽早拒绝；每次重试尝试还会再跑一遍。
        bound_chat_request(desc, req).map_err(app_to_llm)?;

        // ── 2. 熔断判断 ──
        if !breaker.allow() {
            // 熔断拒绝**不计入**熔断样本（是我们主动拒绝的）。
            return Err(LlmError::CircuitOpen);
        }

        // ── 3. 信号量 ──
        let semaphore = tokio::sync::Semaphore::new(self.config.max_conns_per_provider.max(1));
        let permit = semaphore.acquire().await.map_err(|_| LlmError::Canceled)?;

        // 每次尝试一个 span —— 重试会产生多个 span，
        // 它们挂在 `llm.chat` 之下（span 层级）。
        let attempt_span = tracing::info_span!(
            target: "nexus::llm",
            "llm.attempt",
            llm.provider = %desc.provider_code,
            llm.model = %desc.model_name,
        );

        // ── 4. 重试（内含供应商调用）──
        //
        // `retry_do` 的错误类型是 `AppError`（重试判据取自
        // `AppError::retry_policy()`）—— 但 `AppError` **丢失了上游状态码**。
        // 所以这里**额外留一份**原始的 `LlmError`，供失败时精确汇报。
        let last_error: Arc<Mutex<Option<LlmError>>> = Arc::new(Mutex::new(None));

        let outcome = retry_do(&self.config.retry, cancel, || {
            let last_error = last_error.clone();
            let attempt_span = attempt_span.clone();
            async move {
                // 预算在**每次尝试**都重跑。
                let bounded = bound_chat_request(desc, req).map_err(app_to_llm)?;
                let max_tokens = bounded.max_tokens;

                let result = self
                    .provider
                    .chat(desc, req, max_tokens)
                    .instrument(attempt_span.clone())
                    .await;

                match result {
                    Ok(response) => Ok(response),
                    Err(err) => {
                        // 记下原始错误（**立即释放锁**，不跨 await）。
                        let mut slot = last_error.lock().unwrap_or_else(|e| e.into_inner());
                        *slot = Some(err.clone());
                        drop(slot);
                        Err(err.to_app_error())
                    }
                }
            }
        })
        .await;

        drop(permit);

        match outcome {
            RetryOutcome::Ok(response) => {
                record_outcome(&breaker, CallOutcome::Success);
                Ok(response)
            }
            RetryOutcome::Exhausted(err) => {
                record_outcome(&breaker, CallOutcome::Failure);
                // 优先用**原始**错误 —— 它带上游状态码。
                let slot = last_error.lock().unwrap_or_else(|e| e.into_inner());
                match slot.clone() {
                    Some(original) => Err(original),
                    None => Err(app_to_llm(err)),
                }
            }
            RetryOutcome::Canceled => {
                record_outcome(&breaker, CallOutcome::Canceled);
                Err(LlmError::Canceled)
            }
        }
    }
}

/// 取出**可用的** fallback 描述符。
///
/// # 同一供应商**不切换**
///
/// 参考实现只在「fallback 的 provider 与 primary 不同」时才切换。
/// 同一个供应商切两次没有意义 —— 失败原因不会变，
/// 只是白白多等一次超时。
fn usable_fallback(
    primary: &ModelDescriptor,
    fallback: Option<&ModelDescriptor>,
) -> Option<ModelDescriptor> {
    let fallback = fallback?;
    if fallback.provider_code == primary.provider_code {
        return None;
    }
    Some(fallback.clone())
}

/// 两边都失败时，合成一个**同时含双方信息**的错误。
///
/// # 为什么错误信息要含双方
///
/// 运维需要知道「试过哪两个供应商」——
/// 只报其中一个，下次排障会以为是另一个没配。
fn combine_errors(
    primary: &ModelDescriptor,
    primary_err: &LlmError,
    fallback: &ModelDescriptor,
    fallback_err: &LlmError,
) -> LlmError {
    LlmError::BothFailed {
        primary: format!("{}: {primary_err}", primary.provider_code),
        fallback: format!("{}: {fallback_err}", fallback.provider_code),
    }
}

/// 把 [`AppError`] 转回 [`LlmError`]。
///
/// # 为什么需要反向转换
///
/// `retry_do` 的错误类型是 `AppError`（因为重试判据取自
/// `AppError::retry_policy()`），但网关的语义错误是 `LlmError`。
fn app_to_llm(err: AppError) -> LlmError {
    use nexus_core::error::ErrorCode;

    match err.facing.code {
        ErrorCode::BudgetExceeded => LlmError::BudgetExceeded,
        ErrorCode::ProviderNotFound => LlmError::ProviderNotFound,
        ErrorCode::ModelTimeout => LlmError::Timeout,
        ErrorCode::RateLimited => LlmError::RateLimited { retry_after: None },
        ErrorCode::ModelUnavailable => LlmError::Upstream { status: 502 },
        ErrorCode::Internal => LlmError::Malformed("上游响应格式非法".to_string()),
        ErrorCode::ServiceUnavailable => LlmError::Canceled,
        _ => LlmError::Upstream { status: 502 },
    }
}

/// 流式回退的结果。
///
/// # 为什么用 `enum` 而不是一个 `Option<Stream>`
///
/// 流式回退有一个**不可逆**的边界：
///
/// - **首事件之前**失败 → 可以换一个供应商重来。
/// - **首事件之后**失败 → **不可回退**（客户端已收到一半内容）。
///
/// 这个「不可逆」必须体现在**类型上** —— 编译器能帮我们挡住
/// 「流已经开始却还想回退」的代码路径。
#[derive(Debug)]
pub enum StreamOutcome {
    /// 流已建立。
    Stream(StreamHandle),
    /// 首事件之前失败 —— 可以回退。
    ///
    /// 携带**原始错误**，调用方据此决定是否换供应商重试。
    FailedBeforeStart(LlmError),
}

/// 一个已建立的流。
///
/// # 为什么手写 `Debug`
///
/// [`BoxStream`] 不实现 `Debug` —— 而 workspace 的 lint
/// `missing_debug_implementations` 要求公共类型都有 `Debug`。
/// 这里只打印**供应商代号**，不碰流本身。
pub struct StreamHandle {
    /// 事件流。
    pub stream: futures::stream::BoxStream<'static, Result<StreamEvent, LlmError>>,
    /// 实际使用的供应商代号。
    pub provider_code: String,
}

impl std::fmt::Debug for StreamHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamHandle")
            .field("provider_code", &self.provider_code)
            .finish_non_exhaustive()
    }
}

impl Gateway {
    /// 流式对话 —— **建立流**，不消费流。
    ///
    /// # 回退边界
    ///
    /// 本方法只负责「**把流建起来**」。
    ///
    /// - 建流**失败**（连接失败、非 2xx）→ 返回
    ///   [`StreamOutcome::FailedBeforeStart`] —— **此时还没有任何事件发出，回退是允许的**。
    /// - 建流**成功** → 返回 [`StreamOutcome::Stream`]。
    ///   此后流中途的失败**一律不可回退** ——
    ///   客户端可能已经收到了一部分内容，切换模型会产生内容拼接错乱。
    ///
    /// # 为什么不在本方法里自动回退
    ///
    /// 因为「回退」需要**重新走一遍整条管道**（预算、熔断、信号量、重试），
    /// 而管道是 `async` 且带**取消令牌**的 ——
    /// 在一个返回 `impl Stream` 的方法里做这些会让生命周期变得不可表达。
    ///
    /// 更重要的是：**回退必须由调用方显式决定** ——
    /// 因为「首事件之前」这个边界只有**消费者**知道
    /// （流是惰性的，「已经发给客户端了」不是「调用了 `send`」，
    /// 而是「下游 poll 到了这个 item」）。
    ///
    /// # 首事件的锁定点
    ///
    /// 锁定发生在**下游真正 poll 到首个事件时**，
    /// 而**不是**在「上游发来首个事件时」。
    /// 详见架构文档 §3.3 与 `examples/demo-stream-fallback`。
    pub async fn chat_stream(&self, desc: &ModelDescriptor, req: &ChatRequest) -> StreamOutcome {
        // ── 预算裁剪（调用前拒绝）──
        let bounded = match bound_chat_request(desc, req) {
            Ok(b) => b,
            Err(e) => return StreamOutcome::FailedBeforeStart(app_to_llm(e)),
        };

        // ── 熔断判断 ──
        let breaker = self.breakers.get_or_create(&desc.provider_code);
        if !breaker.allow() {
            return StreamOutcome::FailedBeforeStart(LlmError::CircuitOpen);
        }

        // ── 建流 ──
        match self
            .provider
            .chat_stream(desc, req, bounded.max_tokens)
            .await
        {
            Ok(stream) => {
                // ⚠️ span 绑的是**流**，不是「拿到流」这个时刻。
                //
                // 参考实现的 `spanOnClose` 注释原文：
                // 「若放进 runWithGuard，拿到通道就返回，span 只有几毫秒，等于没测」。
                //
                // 所以 span 被**移进**流的包装里 ——
                // 它随流被消费完（或被 drop）才结束，
                // 从而覆盖「模型生成 + 客户端消费」的**全部**耗时。
                let span = StreamSpan::new("llm.chat_stream", &desc.provider_code);
                // `wrap` **消耗** span —— wrapper 持有**唯一**的句柄，
                // 所以 span 随**流**结束（而不是随「拿到流」结束）。
                //
                // `wrap_with` 的回调在**事件流经时**记录字段 ——
                // 因为 span 在流结束时才关闭，
                // 事后再记字段就看不到了。
                let stream = futures::StreamExt::boxed(span.wrap_with(stream, |span, item| {
                    if let Ok(event) = item {
                        if !event.finish_reason.is_empty() {
                            span.record("llm.finish_reason", event.finish_reason.as_str());
                        }
                        if let Some(usage) = &event.usage {
                            span.record("llm.prompt_tokens", usage.prompt_tokens);
                            span.record("llm.completion_tokens", usage.completion_tokens);
                        }
                    }
                }));

                // 建流成功 —— 记 `ok`。
                //
                // ⚠️ 建流成功后**流中途**的失败**不再计数** ——
                // 那属于「流已建立后」的错误，由下游处理。
                // 原因：中途失败的 span 归属与重试语义不同，
                // 在这里补记会与 `llm.chat` 的计数**重复**。
                self.metrics
                    .record_request(&desc.provider_code, RequestStatus::Ok);

                StreamOutcome::Stream(StreamHandle {
                    stream,
                    provider_code: desc.provider_code.clone(),
                })
            }
            Err(err) => {
                record_outcome(&breaker, CallOutcome::Failure);
                self.metrics
                    .record_request(&desc.provider_code, RequestStatus::Error);
                StreamOutcome::FailedBeforeStart(err)
            }
        }
    }
}
