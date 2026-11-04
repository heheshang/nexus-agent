//! 熔断器：环形缓冲 + 预热期。
//!
//! 参考实现 `internal/llm/fallback.go:13`。
//!
//! # ⚠️ 熔断是**单副本粒度**
//!
//! 熔断状态**只在本进程内维护**。多副本部署时每个副本各有一套 ——
//! 会出现「A 副本熔断、B 副本继续打」的不一致。
//!
//! 这是参考实现的已知局限（参考架构文档 §5.3 局限 #3）。
//! 本阶段**明确接受**并声明 —— 外置到 Redis/PG 属后续特性，不在 P3 范围。
//!
//! # 为什么用 `std::sync::Mutex` 而**不是** `tokio::sync::Mutex`
//!
//! 熔断操作是**纯同步**的、不跨 await：
//!
//! - `std::sync::Mutex` 没有 async 开销（`tokio::sync::Mutex` 的获取是 async 的）。
//! - 更重要的是：**编译器会强制你不在持锁时 await** ——
//!   `std::sync::MutexGuard` 不是 `Send`，跨 await 是编译错误。
//!   `tokio::sync::MutexGuard` 反而允许跨 await，那是**隐藏的** bug 来源。
//!
//! # 预热期是**必需的**
//!
//! 参考实现的注释原文：
//!
//! > 预热期：样本不足不评估（窗口零值 false 不代表真实失败）
//!
//! 窗口里初始都是 `false`（成功）—— 如果样本不足就评估错误率，
//! 一个**从未失败**的供应商会因为「窗口里的 false 太少」被误判为失败。
//! （换个角度看：初始值全 `false` 时错误率是 0，不会误判 ——
//! 但如果窗口初始值全是 `true` 就会。关键是**样本不足时根本不评估**，
//! 这与参考实现一致，也更安全。）

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 熔断配置。
#[derive(Debug, Clone, PartialEq)]
pub struct BreakerConfig {
    /// 窗口大小（保留最近 N 次调用结果）。
    pub window_size: usize,
    /// 错误率阈值（超过则打开）。
    ///
    /// 用**比例**（`0..=1`）而不是「失败次数」——
    /// 窗口大小变化时阈值语义不变。
    pub failure_threshold: f64,
    /// 熔断打开后的持续时长。
    pub open_duration: Duration,
}

impl Default for BreakerConfig {
    /// 参考实现的默认值（`internal/llm/fallback.go`）：窗口 20、打开 60s。
    ///
    /// 阈值取 `0.5` —— 超过一半的调用失败才熔断。
    fn default() -> Self {
        Self {
            window_size: 20,
            failure_threshold: 0.5,
            open_duration: Duration::from_secs(60),
        }
    }
}

/// 熔断器的内部状态。
#[derive(Debug)]
struct Inner {
    /// 环形缓冲：每个元素是一次调用的结果（`true` = 失败）。
    window: VecDeque<bool>,
    /// 已填充的样本数（`filled < window_size` 即**预热期**）。
    filled: usize,
    /// 熔断打开的截止时刻。
    open_until: Option<Instant>,
}

/// 熔断器。
///
/// # 线程安全
///
/// 用 `std::sync::Mutex` —— 熔断判断是**同步**的、不跨 await。
///
/// 状态装在 `Arc` 里 —— 因为**克隆必须共享同一份状态**：
/// 「同一个供应商共用一个熔断器」是这个类型的核心语义，
/// 克隆一份独立状态就不再是同一个熔断器了。
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    config: BreakerConfig,
    inner: Arc<Mutex<Inner>>,
}

impl CircuitBreaker {
    /// 创建熔断器。
    pub fn new(config: BreakerConfig) -> Self {
        let window_size = config.window_size;
        Self {
            config,
            inner: Arc::new(Mutex::new(Inner {
                window: VecDeque::with_capacity(window_size),
                filled: 0,
                open_until: None,
            })),
        }
    }

    /// 当前是否**允许**调用。
    ///
    /// 熔断打开且冷却未结束时返回 `false`。
    /// 冷却结束后**允许**（进入半开试探）—— 参考实现的行为。
    pub fn allow(&self) -> bool {
        let inner = self.lock();

        match inner.open_until {
            // 未打开 —— 允许。
            None => true,
            Some(open_until) => {
                if Instant::now() >= open_until {
                    // 冷却结束 —— 进入半开，允许试探。
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 是否处于**熔断打开**状态（尚未冷却完毕）。
    pub fn is_open(&self) -> bool {
        !self.allow()
    }

    /// 记录一次**成功**。
    pub fn record_success(&self) {
        self.record(false);
    }

    /// 记录一次**失败**。
    pub fn record_failure(&self) {
        self.record(true);
    }

    /// 记录一次调用结果。
    fn record(&self, failed: bool) {
        let mut inner = self.lock();

        // 环形缓冲：满了就挤掉最老的。
        if inner.window.len() == self.config.window_size {
            inner.window.pop_front();
        }
        inner.window.push_back(failed);
        inner.filled += 1;

        // **预热期**：样本不足不评估错误率。
        if inner.filled < self.config.window_size {
            return;
        }

        let failures = inner.window.iter().filter(|f| **f).count();
        // 注意：**不能**在这里调用 `self.window_len()` ——
        // 那会再加一次锁，而 `std::sync::Mutex` **不可重入** → 死锁。
        let window_len = inner.window.len().max(1);
        let error_rate = failures as f64 / window_len as f64;

        if error_rate > self.config.failure_threshold {
            // 打开熔断，并**重置窗口**（参考实现：窗口置为全 true、filled = 0）。
            //
            // 重置的意义：冷却结束后从**干净的窗口**重新评估 ——
            // 否则旧的高错误率样本会让熔断立刻再次打开。
            inner.window.clear();
            for _ in 0..self.config.window_size {
                inner.window.push_back(true);
            }
            inner.filled = 0;
            inner.open_until = Some(Instant::now() + self.config.open_duration);
        }
    }

    /// 当前已填充的样本数。
    ///
    /// 供测试与指标使用 —— 暴露「预热期还差多少样本」。
    pub fn filled(&self) -> usize {
        self.lock().filled
    }

    /// 当前的错误率（`0.0..=1.0`）。
    ///
    /// 样本不足时返回 `0.0` —— **不评估**（预热期语义）。
    pub fn error_rate(&self) -> f64 {
        let inner = self.lock();
        if inner.filled < self.config.window_size || inner.window.is_empty() {
            return 0.0;
        }
        let failures = inner.window.iter().filter(|f| **f).count();
        failures as f64 / inner.window.len() as f64
    }

    /// 取锁。
    ///
    /// 锁中毒（poison）时**取回内部数据**而不是 panic ——
    /// 熔断器的状态丢失不值得让整个进程崩掉。
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 熔断器的**注册表** —— 按供应商标识分别维护熔断状态。
///
/// # ⚠️ 仍是**单副本粒度**
///
/// 整个注册表在一个进程内 —— 与单个熔断器同样的局限。
#[derive(Debug, Default)]
pub struct BreakerRegistry {
    breakers: Mutex<std::collections::HashMap<String, CircuitBreaker>>,
}

impl BreakerRegistry {
    /// 创建注册表。
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// 取得某个供应商的熔断器（不存在则用默认配置创建）。
    pub fn get_or_create(&self, provider_code: &str) -> CircuitBreaker {
        let mut map = self.breakers.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(provider_code.to_string())
            .or_insert_with(|| CircuitBreaker::new(BreakerConfig::default()))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker(window_size: usize, threshold: f64) -> CircuitBreaker {
        CircuitBreaker::new(BreakerConfig {
            window_size,
            failure_threshold: threshold,
            open_duration: Duration::from_secs(60),
        })
    }

    #[test]
    fn a_fresh_breaker_allows_calls() {
        let b = breaker(20, 0.5);

        assert!(b.allow());
        assert!(!b.is_open());
    }

    #[test]
    fn the_warmup_period_does_not_evaluate_the_error_rate() {
        // 窗口 4 —— 前 3 次失败**不得**触发熔断（样本不足）。
        let b = breaker(4, 0.5);

        b.record_failure();
        b.record_failure();
        b.record_failure();

        assert!(b.allow(), "预热期不得因样本不足就熔断");
        assert_eq!(b.filled(), 3);
        assert_eq!(
            b.error_rate(),
            0.0,
            "预热期不评估错误率 —— 样本不足时返回 0"
        );
    }

    #[test]
    fn the_error_rate_is_evaluated_once_the_window_is_full() {
        let b = breaker(4, 0.5);

        b.record_failure();
        b.record_failure();
        b.record_success();
        b.record_success();

        assert_eq!(b.error_rate(), 0.5);
        assert!(b.allow(), "0.5 不超过阈值 0.5 —— 不熔断");
    }

    #[test]
    fn exceeding_the_threshold_opens_the_circuit() {
        let b = breaker(4, 0.5);

        b.record_failure();
        b.record_failure();
        b.record_failure();
        b.record_success();

        assert!(!b.allow(), "错误率 0.75 > 0.5 —— 应熔断");
        assert!(b.is_open());
    }

    #[test]
    fn opening_the_circuit_resets_the_window() {
        let b = breaker(4, 0.5);

        b.record_failure();
        b.record_failure();
        b.record_failure();
        b.record_success();

        assert!(b.is_open());
        assert_eq!(
            b.filled(),
            0,
            "打开熔断时窗口被重置 —— 冷却后从干净窗口重新评估"
        );
    }

    #[test]
    fn the_circuit_allows_calls_again_after_the_cooldown() {
        let b = CircuitBreaker::new(BreakerConfig {
            window_size: 2,
            failure_threshold: 0.5,
            // 极短的冷却 —— 测试不等待真实时间。
            open_duration: Duration::from_millis(10),
        });

        b.record_failure();
        b.record_failure();
        assert!(b.is_open());

        std::thread::sleep(Duration::from_millis(30));

        assert!(b.allow(), "冷却结束后应允许试探（半开）");
    }

    #[test]
    fn successes_lower_the_error_rate() {
        let b = breaker(4, 0.5);

        b.record_failure();
        b.record_failure();
        b.record_success();
        b.record_success();
        assert_eq!(b.error_rate(), 0.5);

        b.record_success();
        b.record_success();
        assert_eq!(b.error_rate(), 0.0, "滚动窗口应挤掉旧的失败样本");
    }

    #[test]
    fn the_registry_gives_each_provider_its_own_breaker() {
        let registry = BreakerRegistry::new();

        let a = registry.get_or_create("deepseek");
        let b = registry.get_or_create("openai");

        a.record_failure();

        assert_eq!(a.filled(), 1);
        assert_eq!(b.filled(), 0, "不同供应商的熔断状态必须独立");
    }

    #[test]
    fn the_registry_returns_the_same_breaker_for_the_same_provider() {
        let registry = BreakerRegistry::new();

        let a = registry.get_or_create("deepseek");
        let b = registry.get_or_create("deepseek");

        a.record_failure();

        assert_eq!(b.filled(), 1, "同一供应商应共用一个熔断器");
    }

    #[test]
    fn a_cloned_breaker_shares_the_same_state() {
        // 这条钉死「克隆共享状态」——
        // 若克隆出的是**独立**窗口，「同一供应商共用一个熔断器」就不成立，
        // 熔断会**对同一个供应商各算各的**。
        let a = breaker(4, 0.5);
        let b = a.clone();

        a.record_failure();

        assert_eq!(b.filled(), 1, "克隆必须共享同一份窗口");
    }

    #[test]
    fn recording_from_multiple_threads_is_safe() {
        let b = breaker(100, 0.5);

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let b = b.clone();
                scope.spawn(move || {
                    for _ in 0..10 {
                        b.record_success();
                    }
                });
            }
        });

        assert_eq!(b.filled(), 80, "并发记录不应丢样本");
    }
}
