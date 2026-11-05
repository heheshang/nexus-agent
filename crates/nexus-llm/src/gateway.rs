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

use nexus_core::error::AppError;

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
}

impl Gateway {
    /// 创建网关。
    pub fn new(provider: OpenAiProvider, config: GatewayConfig) -> Self {
        Self {
            provider,
            breakers: Arc::new(BreakerRegistry::new()),
            config,
        }
    }

    /// 用默认配置创建网关。
    pub fn with_defaults(provider: OpenAiProvider) -> Self {
        Self::new(provider, GatewayConfig::default())
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

        // ── 4. 重试（内含供应商调用）──
        //
        // `retry_do` 的错误类型是 `AppError`（重试判据取自
        // `AppError::retry_policy()`）—— 但 `AppError` **丢失了上游状态码**。
        // 所以这里**额外留一份**原始的 `LlmError`，供失败时精确汇报。
        let last_error: Arc<Mutex<Option<LlmError>>> = Arc::new(Mutex::new(None));

        let outcome = retry_do(&self.config.retry, cancel, || {
            let last_error = last_error.clone();
            async move {
                // 预算在**每次尝试**都重跑。
                let bounded = bound_chat_request(desc, req).map_err(app_to_llm)?;
                let max_tokens = bounded.max_tokens;

                let result = self.provider.chat(desc, req, max_tokens).await;

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
            Ok(stream) => StreamOutcome::Stream(StreamHandle {
                stream,
                provider_code: desc.provider_code.clone(),
            }),
            Err(err) => {
                record_outcome(&breaker, CallOutcome::Failure);
                StreamOutcome::FailedBeforeStart(err)
            }
        }
    }
}
