//! 重试：指数退避 + 抖动。
//!
//! 参考实现 `internal/llm/retry.go:24`。
//!
//! # 抖动是**必需**的
//!
//! 没有抖动，多个并发请求会在**同一时刻**重试，形成同步风暴 ——
//! 上游刚恢复就被自己人的重试再打垮。
//!
//! # 重试判据**不重写**
//!
//! 判据一律取 `nexus-core` 的 `RetryPolicy`（架构文档 §2.6.3）——
//! `nexus-llm` **不另写一套**「这个错误能不能重试」的判断。
//!
//! | 策略 | 含义 | 网关的行为 |
//! |---|---|---|
//! | `Retryable` | 明确失败，重试安全 | ✅ 重试 |
//! | `UnknownOutcome` | **结果未知** —— 请求可能已到达上游并产生副作用 | ❌ **不重试** |
//! | `Terminal` | 明确失败，重试无意义 | ❌ 不重试 |
//!
//! `UnknownOutcome` 的例子是**超时**（`ErrorCode::ModelTimeout`）——
//! 请求可能已被上游接收并计费，重试会**重复扣费**。
//!
//! 参考实现的 `IsRetryable` 把 504（超时）与 502（明确拒绝）**一视同仁** ——
//! 那是本设计要修正的具体缺陷（架构文档 §2.6.3）。

use std::future::Future;
use std::time::Duration;

use nexus_core::error::{AppError, RetryPolicy};

/// 重试配置。
#[derive(Debug, Clone, PartialEq)]
pub struct RetryConfig {
    /// 最大尝试次数（含第一次）。
    pub max_attempts: u32,
    /// 基础延迟。
    pub base_delay: Duration,
    /// 延迟上限。
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    /// 参考实现的默认值（`internal/llm/retry.go:19-22`）：
    /// 3 次尝试、100ms 基础延迟、2s 上限。
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
        }
    }
}

impl RetryConfig {
    /// 第 `attempt` 次重试（从 0 开始）应等待的**基础**延迟。
    ///
    /// 指数增长并截断到 [`RetryConfig::max_delay`]。
    pub fn base_delay_for(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        let delay = self.base_delay.saturating_mul(factor);
        delay.min(self.max_delay)
    }

    /// 第 `attempt` 次重试应等待的延迟，**含抖动**。
    ///
    /// 抖动范围是 `[0, delay/4)` —— 与参考实现一致
    /// （`internal/llm/retry.go` 用 `rand.Int63n(delay/4)`）。
    ///
    /// 抖动是**必需的** —— 没有它，并发请求会在同一时刻重试，形成同步风暴。
    pub fn delay_for(&self, attempt: u32) -> Duration {
        let delay = self.base_delay_for(attempt);
        let jitter_span = delay / 4;
        let jitter = if jitter_span.is_zero() {
            Duration::ZERO
        } else {
            // `rand::random_range` 在 `RngExt` trait 上（rand 0.10 的 API 形状）。
            let nanos = rand::random_range(0..jitter_span.as_nanos() as u64);
            Duration::from_nanos(nanos)
        };
        delay + jitter
    }
}

/// 一次调用的**结果分类** —— 决定它是否进入熔断器的样本窗口。
///
/// # 取消**不计入**熔断样本
///
/// 这是熔断正确性的关键（参考实现 `internal/llm/gateway.go:346-349`）：
/// 用户取消**不是**供应商的过错 —— 把它记为失败会误伤供应商。
///
/// 同理，**熔断打开**导致的拒绝也不算失败 ——
/// 那是我们**主动**拒绝，不是供应商失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    /// 调用成功。
    Success,
    /// 调用失败 —— **计入**熔断样本。
    Failure,
    /// 调用被取消 —— **不计入**熔断样本。
    Canceled,
    /// 调用被熔断器拒绝 —— **不计入**熔断样本（是我们主动拒绝的）。
    Rejected,
}

impl CallOutcome {
    /// 是否**计入**熔断样本。
    pub fn counts_towards_breaker(self) -> bool {
        matches!(self, Self::Failure)
    }
}

/// 把调用结果记录到熔断器。
///
/// 取消与熔断拒绝**不记录** —— 见 [`CallOutcome`] 的文档。
pub fn record_outcome(breaker: &crate::breaker::CircuitBreaker, outcome: CallOutcome) {
    match outcome {
        CallOutcome::Success => breaker.record_success(),
        CallOutcome::Failure => breaker.record_failure(),
        // 取消与拒绝**不污染**错误率。
        CallOutcome::Canceled | CallOutcome::Rejected => {}
    }
}

/// 该错误是否**值得**重试。
///
/// 委托给 `nexus-core` 的 [`RetryPolicy`] —— 分类是**码的属性**，不是错误对象的属性。
pub fn is_retryable(error: &AppError) -> bool {
    error.retry_policy() == RetryPolicy::Retryable
}

/// 重试执行的结果。
#[derive(Debug)]
pub enum RetryOutcome<T> {
    /// 成功。
    Ok(T),
    /// 重试耗尽（最后一次尝试失败）。
    Exhausted(AppError),
    /// **被取消** —— 调用方主动取消，不是失败。
    Canceled,
}

/// 带指数退避与抖动的重试。
///
/// # 取消语义
///
/// 取消**立即中止** —— 不等退避结束，也不算失败。
/// 取消**不计入**熔断样本（见 [`crate::breaker`]）。
///
/// # 为什么用 `select!` 且带 `biased`
///
/// `biased;` 保证**取消分支优先于**业务分支被检查 ——
/// 否则在高负载下取消可能被饿死（业务分支一直就绪，取消分支排不上）。
pub async fn retry_do<F, Fut, T>(
    cfg: &RetryConfig,
    cancel: &tokio_util::sync::CancellationToken,
    mut operation: F,
) -> RetryOutcome<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, AppError>>,
{
    let max_attempts = cfg.max_attempts.max(1);

    for attempt in 0..max_attempts {
        // 先看取消 —— 已经在取消状态下就不该发起新尝试。
        if cancel.is_cancelled() {
            return RetryOutcome::Canceled;
        }

        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return RetryOutcome::Canceled,
            r = operation() => r,
        };

        match result {
            Ok(value) => return RetryOutcome::Ok(value),
            Err(error) => {
                // 不可重试 —— 立即返回。
                if !is_retryable(&error) {
                    return RetryOutcome::Exhausted(error);
                }
                // 已经是最后一次尝试。
                if attempt + 1 >= max_attempts {
                    return RetryOutcome::Exhausted(error);
                }
            }
        }

        // 退避等待 —— 期间要能立即响应取消。
        let delay = cfg.delay_for(attempt);
        tokio::select! {
            biased;
            () = cancel.cancelled() => return RetryOutcome::Canceled,
            () = tokio::time::sleep(delay) => {}
        }
    }

    // 循环必然返回 —— `max_attempts >= 1` 保证至少执行一次。
    unreachable!("循环至少执行一次")
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn retryable_error() -> AppError {
        AppError::new(ErrorCode::ModelUnavailable, "上游不可用")
    }

    fn unknown_outcome_error() -> AppError {
        AppError::new(ErrorCode::ModelTimeout, "上游超时")
    }

    fn terminal_error() -> AppError {
        AppError::new(ErrorCode::BudgetExceeded, "超出预算")
    }

    #[test]
    fn default_config_matches_the_reference() {
        let cfg = RetryConfig::default();

        assert_eq!(cfg.max_attempts, 3);
        assert_eq!(cfg.base_delay, Duration::from_millis(100));
        assert_eq!(cfg.max_delay, Duration::from_secs(2));
    }

    #[test]
    fn backoff_grows_exponentially_and_is_capped() {
        let cfg = RetryConfig::default();

        assert_eq!(cfg.base_delay_for(0), Duration::from_millis(100));
        assert_eq!(cfg.base_delay_for(1), Duration::from_millis(200));
        assert_eq!(cfg.base_delay_for(2), Duration::from_millis(400));
        // 截断到上限
        assert_eq!(cfg.base_delay_for(10), Duration::from_secs(2));
    }

    #[test]
    fn jitter_is_present_but_bounded() {
        let cfg = RetryConfig::default();

        // 抖动范围是 [0, delay/4)。
        for attempt in 0..5 {
            let base = cfg.base_delay_for(attempt);
            for _ in 0..50 {
                let delay = cfg.delay_for(attempt);
                assert!(
                    delay >= base && delay < base + base / 4,
                    "抖动超出范围：{delay:?} 不在 [{base:?}, {:?})",
                    base + base / 4
                );
            }
        }
    }

    #[test]
    fn jitter_actually_varies() {
        let cfg = RetryConfig::default();

        let samples: Vec<_> = (0..50).map(|_| cfg.delay_for(2)).collect();

        // 全都相同意味着抖动没生效 —— 那会产生同步风暴。
        assert!(
            samples
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "抖动未生效：所有延迟相同"
        );
    }

    #[test]
    fn unknown_outcome_is_not_retryable() {
        // 这是本设计相对参考实现的**关键改进**。
        // 超时意味着请求可能已被上游接收并计费 —— 重试会重复扣费。
        assert!(!is_retryable(&unknown_outcome_error()));
    }

    #[test]
    fn terminal_is_not_retryable() {
        assert!(!is_retryable(&terminal_error()));
    }

    #[test]
    fn retryable_is_retryable() {
        assert!(is_retryable(&retryable_error()));
    }

    #[test]
    fn cancellation_does_not_count_towards_the_breaker() {
        // 用户取消不是供应商的过错 —— 记为失败会误伤供应商。
        let breaker = crate::breaker::CircuitBreaker::new(crate::breaker::BreakerConfig {
            window_size: 4,
            failure_threshold: 0.5,
            open_duration: Duration::from_secs(60),
        });

        record_outcome(&breaker, CallOutcome::Canceled);

        assert_eq!(breaker.filled(), 0, "取消不进入熔断样本窗口");
        assert!(breaker.allow(), "取消不得触发熔断");
    }

    #[test]
    fn a_rejected_call_does_not_count_towards_the_breaker() {
        // 熔断拒绝是我们**主动**拒绝的，不是供应商失败。
        let breaker = crate::breaker::CircuitBreaker::new(crate::breaker::BreakerConfig {
            window_size: 4,
            failure_threshold: 0.5,
            open_duration: Duration::from_secs(60),
        });

        record_outcome(&breaker, CallOutcome::Rejected);

        assert_eq!(breaker.filled(), 0);
        assert!(breaker.allow());
    }

    #[test]
    fn only_failures_count_towards_the_breaker() {
        assert!(CallOutcome::Failure.counts_towards_breaker());
        assert!(!CallOutcome::Success.counts_towards_breaker());
        assert!(!CallOutcome::Canceled.counts_towards_breaker());
        assert!(!CallOutcome::Rejected.counts_towards_breaker());
    }

    #[test]
    fn failures_are_recorded_and_successes_lower_the_error_rate() {
        // 窗口 2 —— 两次样本后窗口即满（否则处于预热期，不评估错误率）。
        let breaker = crate::breaker::CircuitBreaker::new(crate::breaker::BreakerConfig {
            window_size: 2,
            failure_threshold: 0.5,
            open_duration: Duration::from_secs(60),
        });

        record_outcome(&breaker, CallOutcome::Failure);
        record_outcome(&breaker, CallOutcome::Success);

        // 成功**也进窗口** —— 它是「一次成功调用」的样本，作用是拉低错误率。
        // 只有取消与熔断拒绝**完全不进窗口**。
        assert_eq!(breaker.filled(), 2);
        assert_eq!(breaker.error_rate(), 0.5, "一次失败 + 一次成功 = 50%");
    }

    #[tokio::test]
    async fn succeeds_on_the_first_attempt() {
        let cfg = RetryConfig::default();
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Ok(42u32) }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Ok(42)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retries_a_retryable_error_then_succeeds() {
        let cfg = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            let n = c.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err::<u32, AppError>(retryable_error())
                } else {
                    Ok(7u32)
                }
            }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Ok(7)));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn unknown_outcome_is_not_retried() {
        let cfg = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err::<u32, AppError>(unknown_outcome_error()) }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Exhausted(_)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "结果未知的错误只尝试一次 —— 重试会重复扣费"
        );
    }

    #[tokio::test]
    async fn terminal_error_is_not_retried() {
        let cfg = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err::<u32, AppError>(terminal_error()) }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Exhausted(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn exhausts_all_attempts_on_persistent_retryable_errors() {
        let cfg = RetryConfig {
            max_attempts: 3,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err::<u32, AppError>(retryable_error()) }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Exhausted(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn cancellation_before_the_first_attempt_prevents_the_call() {
        let cfg = RetryConfig::default();
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let outcome = retry_do(&cfg, &cancel, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Ok(1u32) }
        })
        .await;

        assert!(matches!(outcome, RetryOutcome::Canceled));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "已取消时不应发起调用");
    }

    #[tokio::test]
    async fn cancellation_during_backoff_aborts_immediately() {
        let cfg = RetryConfig {
            max_attempts: 5,
            // 很长的退避 —— 若取消不生效，测试会挂 30 秒。
            base_delay: Duration::from_secs(30),
            max_delay: Duration::from_secs(30),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let calls = Arc::new(AtomicU32::new(0));

        let c = calls.clone();
        let cancel_for_task = cancel.clone();
        let handle = tokio::spawn(async move {
            retry_do(&cfg, &cancel_for_task, || {
                c.fetch_add(1, Ordering::SeqCst);
                async { Err::<u32, AppError>(retryable_error()) }
            })
            .await
        });

        // 让第一次尝试失败并进入退避。
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let outcome = handle.await.expect("任务应结束");

        assert!(
            matches!(outcome, RetryOutcome::Canceled),
            "退避期间的取消应立即中止"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "取消后不应再发起新尝试");
    }
}
