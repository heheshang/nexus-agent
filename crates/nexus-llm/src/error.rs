//! 领域错误：模型调用失败的原因。
//!
//! # 为什么不直接用 `nexus-core::ModelError`
//!
//! `nexus-core` 的 `ModelError` 是**示例**领域错误（架构文档 §2.6.2 的注释原文：
//! 「这是**示例**领域错误 …… 具体领域的错误 enum 随各 crate 落地时再定，
//! 但**模式**由本例确立」）。
//!
//! `nexus-llm` 的错误比它更细 —— 多出「熔断打开」「预算超限」「流已开始」
//! 这些**网关层**特有的失败。但**业务码不新增**：
//! 一律复用 `nexus-core` 已定稿的 42 个码（§6.3）。
//!
//! # 失败分类**不重写**
//!
//! `retry_policy()` **委托**给 `nexus-core::ErrorCode::retry_policy()` ——
//! 分类是**码的属性**，不是错误对象的属性。
//!
//! 这条很关键：参考实现的 `IsRetryable` 把 504（超时，可能已执行）
//! 与 502（明确拒绝）**一视同仁** —— 504 下重试支付类工具会重复扣费。
//! `nexus-core` 的 `RetryPolicy` 区分二者（§2.6.3），
//! 而 `nexus-llm` **不另写一套判断**。

use nexus_core::error::{AppError, ErrorCode};
use std::time::Duration;

/// 模型网关的领域错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    /// 上游超时。
    ///
    /// 映射到 `ErrorCode::ModelTimeout` → `RetryPolicy::UnknownOutcome` ——
    /// 请求**可能已到达上游并产生副作用**，不能自动重试。
    #[error("上游超时")]
    Timeout,

    /// 上游限流。
    #[error("上游限流")]
    RateLimited {
        /// 建议重试等待时长。
        retry_after: Option<Duration>,
    },

    /// 上游返回错误状态。
    ///
    /// # 消息里**必须带状态码**
    ///
    /// 502 与 503 的处置完全不同（一个可能是上游临时抖动，一个可能是过载）——
    /// 消息里没有状态码，运维只能看到一句「上游返回错误」，等于没信息。
    #[error("上游返回错误状态 {status}")]
    Upstream {
        /// HTTP 状态码。
        status: u16,
    },

    /// 响应格式非法。
    ///
    /// 映射到 `ErrorCode::Internal` —— **不是** `ModelUnavailable`。
    /// 后者是 `Retryable`，会让「解析失败」被自动重试 —— 重试同样的坏响应没有意义。
    #[error("响应格式非法")]
    Malformed(String),

    /// 传输层 IO 失败。
    ///
    /// # 为什么存**字符串**而不是 `std::io::Error`
    ///
    /// `std::io::Error` **不实现 `Clone`** —— 而网关需要在重试的闭包里
    /// 留一份原始错误（`AppError` 会丢掉上游状态码）。
    /// 存 `Display` 文本即可：IO 错误只需要给人看，不需要结构化处理。
    #[error("上游 IO 失败：{0}")]
    UpstreamIo(String),

    /// 熔断打开 —— 该供应商已被熔断器拒绝。
    ///
    /// 映射到 `ErrorCode::ModelUnavailable`（`Retryable`）——
    /// 换一个供应商（回退）是有意义的。
    #[error("熔断已打开")]
    CircuitOpen,

    /// 预算超限 —— 请求在**调用前**被拒。
    ///
    /// 映射到 `ErrorCode::BudgetExceeded` → `RetryPolicy::Terminal` ——
    /// 重试同样的请求同样会超预算。
    #[error("超出上下文预算")]
    BudgetExceeded,

    /// 供应商不存在。
    #[error("供应商不存在")]
    ProviderNotFound,

    /// 流已开始 —— **不可回退**。
    ///
    /// 架构文档 §3.3：流式响应一旦开始向客户端发送，就不能回退到另一个模型 ——
    /// 客户端已经收到了一半内容，切换模型会产生内容拼接错乱。
    ///
    /// 映射到 `ErrorCode::ModelUnavailable`（`Retryable`）——
    /// **对当前流**不可重试，但**对下一次调用**可以。
    #[error("流已开始，不能回退")]
    StreamAlreadyStarted,

    /// 调用被取消。
    ///
    /// 映射到 `ErrorCode::ServiceUnavailable`。
    ///
    /// ⚠️ **取消不是失败** —— 它**不计入**熔断样本（AC #10 of NEX-18）。
    #[error("调用已取消")]
    Canceled,

    /// 两边都失败 —— primary 与 fallback 都试过了。
    ///
    /// # 为什么要有这个变体
    ///
    /// 参考实现要求「两边都失败时，错误信息**同时含** primary 与 fallback 的信息」。
    /// 如果只报其中一个，运维会以为另一个没配 —— 那是**误导**。
    ///
    /// 消息里带的是**供应商代号 + 各自的原因**，
    /// **不**带 `base_url` / 响应体等内部细节（§2.6.5）。
    #[error("主供应商与备用供应商均失败：{primary}；{fallback}")]
    BothFailed {
        /// 主供应商的失败信息。
        primary: String,
        /// 备用供应商的失败信息。
        fallback: String,
    },
}

impl LlmError {
    /// 映射到应用错误（带业务码）。
    ///
    /// 按**值**消费 —— 内部原因要求 `'static` 生命周期
    /// （`AppError::cause` 是 `Arc<dyn Error + 'static>`）。
    pub fn to_app_error(self) -> AppError {
        match self {
            // 超时 → **结果未知**，不能自动重试
            Self::Timeout => AppError::new(ErrorCode::ModelTimeout, "模型上游超时"),
            Self::RateLimited { .. } => {
                AppError::new(ErrorCode::RateLimited, "模型请求过于频繁，请稍后再试")
            }
            Self::Upstream { status } => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
                    .with_cause_msg(format!("upstream status {status}"))
            }
            // 解析失败 → Internal（Terminal）—— 重试同样的坏响应没有意义
            Self::Malformed(s) => {
                AppError::new(ErrorCode::Internal, "系统内部错误").with_cause_msg(s)
            }
            Self::UpstreamIo(msg) => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
                    .with_cause_msg(msg)
            }
            Self::CircuitOpen => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
            }
            Self::BudgetExceeded => AppError::new(ErrorCode::BudgetExceeded, "请求超出上下文预算"),
            Self::ProviderNotFound => {
                AppError::new(ErrorCode::ProviderNotFound, "模型供应商不存在")
            }
            Self::StreamAlreadyStarted => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
            }
            Self::Canceled => AppError::new(ErrorCode::ServiceUnavailable, "请求已取消"),
            Self::BothFailed { primary, fallback } => {
                // 两边都失败 → 对外只说「暂不可用」，
                // 双方明细进 `cause`（内部原因不外泄，§2.6.5）。
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
                    .with_cause_msg(format!("primary: {primary}; fallback: {fallback}"))
            }
        }
    }

    /// 是否**因调用方取消**而产生。
    ///
    /// 取消**不计入**熔断样本 —— 这是熔断正确性的关键：
    /// 否则用户取消会误伤供应商（参考实现 `internal/llm/gateway.go:346-349`）。
    pub fn is_canceled(&self) -> bool {
        matches!(self, Self::Canceled)
    }
}

impl From<LlmError> for AppError {
    fn from(e: LlmError) -> Self {
        e.to_app_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::RetryPolicy;

    #[test]
    fn timeout_is_unknown_outcome_not_retryable() {
        // 这是本设计相对参考实现的**关键改进**（架构文档 §2.6.3）。
        let app = LlmError::Timeout.to_app_error();

        assert_eq!(app.retry_policy(), RetryPolicy::UnknownOutcome);
    }

    #[test]
    fn upstream_error_is_retryable() {
        let app = LlmError::Upstream { status: 502 }.to_app_error();

        assert_eq!(app.retry_policy(), RetryPolicy::Retryable);
    }

    #[test]
    fn malformed_response_is_terminal_not_retryable() {
        // 解析失败重试同样的坏响应没有意义 —— 不能映射成 ModelUnavailable。
        let app = LlmError::Malformed("bad json".into()).to_app_error();

        assert_eq!(app.retry_policy(), RetryPolicy::Terminal);
    }

    #[test]
    fn budget_exceeded_is_terminal() {
        let app = LlmError::BudgetExceeded.to_app_error();

        assert_eq!(app.retry_policy(), RetryPolicy::Terminal);
    }

    #[test]
    fn circuit_open_is_retryable_so_fallback_can_happen() {
        let app = LlmError::CircuitOpen.to_app_error();

        assert_eq!(app.retry_policy(), RetryPolicy::Retryable);
    }

    #[test]
    fn canceled_is_recognized() {
        assert!(LlmError::Canceled.is_canceled());
        assert!(!LlmError::Timeout.is_canceled());
    }

    #[test]
    fn error_uses_existing_business_codes_only() {
        // 不新增码值 —— 全部复用 nexus-core 已定稿的 42 个码。
        assert_eq!(
            LlmError::Timeout.to_app_error().facing.code,
            ErrorCode::ModelTimeout
        );
        assert_eq!(
            LlmError::BudgetExceeded.to_app_error().facing.code,
            ErrorCode::BudgetExceeded
        );
        assert_eq!(
            LlmError::ProviderNotFound.to_app_error().facing.code,
            ErrorCode::ProviderNotFound
        );
        assert_eq!(
            LlmError::RateLimited { retry_after: None }
                .to_app_error()
                .facing
                .code,
            ErrorCode::RateLimited
        );
    }

    #[test]
    fn internal_reason_never_reaches_the_user_facing_part() {
        let app = LlmError::Malformed("内部细节：字段 x 缺失".into()).to_app_error();

        assert_eq!(app.facing.message, "系统内部错误");
        assert!(!app.facing.message.contains("内部细节"));
    }
}
