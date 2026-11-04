//! 三层错误模型与业务码。
//!
//! 架构文档 §2.6。
//!
//! # 三层错误（§2.6.1）
//!
//! 参考实现只有一个 `errcode.Error`（HTTP 状态 + 业务码 + 消息 + cause）。
//! 本设计分三层，因为它们的**处理位置不同**：
//!
//! | 层 | 类型 | 谁处理 | 例子 |
//! |---|---|---|---|
//! | 领域错误 | 各领域的 enum | 领域内部 | [`ModelError`] |
//! | 应用错误 | [`AppError`] | 用例层 → 协议层 | [`ErrorCode`] |
//! | 传输错误 | `axum` 的 `IntoResponse` | 协议层 | **P5，不在本 crate** |
//!
//! # 领域错误是 enum，不是字符串（§2.6.2）
//!
//! 收益：
//!
//! - `match` 穷尽 —— 新增错误变体时，编译器列出所有需要处理的地方。
//! - `#[from]` 自动转换 —— 传输错误 → 领域错误。
//! - 可以携带**结构化**信息（`retry_after`），字符串做不到。
//!
//! # 失败分类是第一公民（§2.6.3）
//!
//! 参考实现有 `IsRetryable` 的概念，判据是 **429 / 5xx / 网络错误**。
//! 本设计把它提升为类型上的分类，**并区分它没有区分的一种情况**：
//!
//! | 策略 | 重试 | 落库 | 告警 |
//! |---|---|---|---|
//! | [`RetryPolicy::Retryable`] | ✅ 自动重试（带退避） | 失败终态 | 否 |
//! | [`RetryPolicy::UnknownOutcome`] | ❌ 人工介入 | 标记 `needs_attention` | ✅ |
//! | [`RetryPolicy::Terminal`] | ❌ | 失败终态 | 否 |
//!
//! **`UnknownOutcome` 是本设计相对参考实现的强化**。
//!
//! 场景：调用支付工具时 HTTP 超时。请求**可能已经到达**并扣了款。
//! - 当成「失败」→ 重试 → 扣两次款。
//! - 当成「成功」→ 数据错。
//! - 正确做法：标记「结果未知」，让人来看。
//!
//! 参考实现的具体缺陷：它把 504（超时）与 502（上游错误）**一视同仁**地判为可重试。
//! 但二者的语义完全不同 —— 502 说明上游明确拒绝了（没执行），
//! 504 说明上游**可能已经执行了**。支付类工具在 504 下重试会重复扣费。
//!
//! # 业务码：保留分段，但类型化（§2.6.4）
//!
//! 参考实现的数字码段（`1xxxx` 参数 / `2xxxx` 认证 / ... / `9xxxx` 系统）**保留** ——
//! 它承载了「前端按段通用处理」的语义。
//!
//! # 内部原因绝不外泄（§2.6.5）
//!
//! [`UserFacing`] **在类型上**就无法携带内部原因 —— 要输出给客户端，只能给它。

use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 业务错误码。
///
/// 码段沿用参考实现：1=参数 2=认证 3=会话 4=限流 5=LLM 6=RAG 7=工具 8=协议 9=系统。
///
/// **全部 42 个业务码** —— 与架构文档 §2.6.4.1 的契约表一一对应。
/// 码值是前后端契约，不可自行增删或改号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    // ── 1xxxx 参数 ──
    /// 参数非法。
    InvalidParam,
    /// 缺少必填参数。
    MissingParam,
    /// 文件过大。
    FileTooLarge,
    /// 文件类型不支持。
    FileType,
    // ── 2xxxx 认证 ──
    /// 未认证。
    Unauthorized,
    /// 令牌过期。
    TokenExpired,
    /// 令牌无效。
    TokenInvalid,
    /// 无权限。
    Forbidden,
    /// 登录失败。
    LoginFailed,
    /// 账号被锁定。
    UserLocked,
    /// 账号已存在。
    UserExists,
    // ── 3xxxx Agent/会话 ──
    /// Agent 不存在。
    AgentNotFound,
    /// 超出最大迭代次数。
    MaxIteration,
    /// 会话不存在。
    SessionNotFound,
    /// 执行记录不存在。
    RunNotFound,
    /// 工作流 DSL 非法。
    WorkflowDsl,
    /// 超出预算。
    BudgetExceeded,
    /// 执行冲突。
    RunConflict,
    /// 事件游标已过期。
    EventCursorExpired,
    // ── 4xxxx 限流 ──
    /// 被限流。
    RateLimited,
    // ── 5xxxx LLM 上游 ──
    /// 模型不可用。
    ModelUnavailable,
    /// 模型超时。
    ModelTimeout,
    /// 供应商不存在。
    ProviderNotFound,
    /// 向量化失败。
    EmbeddingFailed,
    // ── 6xxxx RAG ──
    /// 解析失败。
    ParseFailed,
    /// 知识库不存在。
    KbNotFound,
    /// 文档不存在。
    DocumentNotFound,
    /// 记忆不存在。
    MemoryNotFound,
    /// 存储不可用。
    StorageUnavailable,
    // ── 7xxxx 工具/沙箱 ──
    /// 工具不存在。
    ToolNotFound,
    /// 工具执行失败。
    ToolFailed,
    /// 工具超时。
    ToolTimeout,
    /// 沙箱超时。
    SandboxTimeout,
    /// SQL 被禁止。
    SqlForbidden,
    /// 域名不被允许。
    DomainNotAllowed,
    // ── 8xxxx MCP/A2A ──
    /// MCP 连接失败。
    McpConnectFailed,
    /// MCP 工具调用失败。
    McpToolFailed,
    /// A2A 任务失败。
    A2ATaskFailed,
    /// A2A 卡片获取失败。
    A2ACardFetchFailed,
    // ── 9xxxx 系统 ──
    /// 系统内部错误。
    Internal,
    /// 未实现。
    NotImplemented,
    /// 服务不可用。
    ServiceUnavailable,
}

/// 全部业务码 —— 用于契约测试。
pub const ALL_CODES: &[ErrorCode] = &[
    ErrorCode::InvalidParam,
    ErrorCode::MissingParam,
    ErrorCode::FileTooLarge,
    ErrorCode::FileType,
    ErrorCode::Unauthorized,
    ErrorCode::TokenExpired,
    ErrorCode::TokenInvalid,
    ErrorCode::Forbidden,
    ErrorCode::LoginFailed,
    ErrorCode::UserLocked,
    ErrorCode::UserExists,
    ErrorCode::AgentNotFound,
    ErrorCode::MaxIteration,
    ErrorCode::SessionNotFound,
    ErrorCode::RunNotFound,
    ErrorCode::WorkflowDsl,
    ErrorCode::BudgetExceeded,
    ErrorCode::RunConflict,
    ErrorCode::EventCursorExpired,
    ErrorCode::RateLimited,
    ErrorCode::ModelUnavailable,
    ErrorCode::ModelTimeout,
    ErrorCode::ProviderNotFound,
    ErrorCode::EmbeddingFailed,
    ErrorCode::ParseFailed,
    ErrorCode::KbNotFound,
    ErrorCode::DocumentNotFound,
    ErrorCode::MemoryNotFound,
    ErrorCode::StorageUnavailable,
    ErrorCode::ToolNotFound,
    ErrorCode::ToolFailed,
    ErrorCode::ToolTimeout,
    ErrorCode::SandboxTimeout,
    ErrorCode::SqlForbidden,
    ErrorCode::DomainNotAllowed,
    ErrorCode::McpConnectFailed,
    ErrorCode::McpToolFailed,
    ErrorCode::A2ATaskFailed,
    ErrorCode::A2ACardFetchFailed,
    ErrorCode::Internal,
    ErrorCode::NotImplemented,
    ErrorCode::ServiceUnavailable,
];

impl ErrorCode {
    /// 完整业务码。
    pub const fn code(self) -> i32 {
        match self {
            // 1xxxx 参数
            Self::InvalidParam => 10001,
            Self::MissingParam => 10002,
            Self::FileTooLarge => 10003,
            Self::FileType => 10004,
            // 2xxxx 认证
            Self::Unauthorized => 20101,
            Self::TokenExpired => 20102,
            Self::TokenInvalid => 20103,
            Self::Forbidden => 20201,
            Self::LoginFailed => 20202,
            Self::UserLocked => 20203,
            Self::UserExists => 20204,
            // 3xxxx Agent/会话
            Self::AgentNotFound => 30001,
            Self::MaxIteration => 30002,
            Self::SessionNotFound => 30003,
            Self::RunNotFound => 30004,
            Self::WorkflowDsl => 30005,
            Self::BudgetExceeded => 30006,
            Self::RunConflict => 30007,
            Self::EventCursorExpired => 30008,
            // 4xxxx 限流
            Self::RateLimited => 40001,
            // 5xxxx LLM 上游
            Self::ModelUnavailable => 50001,
            Self::ModelTimeout => 50002,
            Self::ProviderNotFound => 50003,
            Self::EmbeddingFailed => 50004,
            // 6xxxx RAG
            // 注意：码段 6 从 60002 起 —— 参考实现的原样，不是笔误。
            Self::ParseFailed => 60002,
            Self::KbNotFound => 60003,
            Self::DocumentNotFound => 60004,
            Self::MemoryNotFound => 60005,
            Self::StorageUnavailable => 60006,
            // 7xxxx 工具/沙箱
            Self::ToolNotFound => 70001,
            Self::ToolFailed => 70002,
            Self::ToolTimeout => 70003,
            Self::SandboxTimeout => 70004,
            Self::SqlForbidden => 70005,
            Self::DomainNotAllowed => 70006,
            // 8xxxx MCP/A2A
            Self::McpConnectFailed => 80001,
            Self::McpToolFailed => 80002,
            Self::A2ATaskFailed => 80003,
            Self::A2ACardFetchFailed => 80004,
            // 9xxxx 系统
            // 注意：90002 在 90003 之前 —— 参考实现的原样，不是笔误。
            Self::Internal => 90001,
            Self::NotImplemented => 90002,
            Self::ServiceUnavailable => 90003,
        }
    }

    /// 码段：1=参数 2=认证 3=会话 4=限流 5=LLM 6=RAG 7=工具 8=协议 9=系统。
    pub const fn segment(self) -> u32 {
        (self.code() / 10000) as u32
    }

    /// HTTP 状态码。
    pub const fn http_status(self) -> u16 {
        match self {
            Self::InvalidParam | Self::MissingParam | Self::FileTooLarge | Self::FileType => 400,
            Self::WorkflowDsl => 400,
            Self::Unauthorized | Self::TokenExpired | Self::TokenInvalid | Self::LoginFailed => 401,
            Self::Forbidden | Self::SqlForbidden | Self::DomainNotAllowed => 403,
            Self::AgentNotFound
            | Self::SessionNotFound
            | Self::RunNotFound
            | Self::ProviderNotFound
            | Self::KbNotFound
            | Self::DocumentNotFound
            | Self::MemoryNotFound
            | Self::ToolNotFound => 404,
            Self::EventCursorExpired => 410,
            Self::UserExists | Self::RunConflict => 409,
            Self::UserLocked | Self::RateLimited => 429,
            Self::MaxIteration
            | Self::BudgetExceeded
            | Self::ParseFailed
            | Self::ToolFailed
            | Self::Internal => 500,
            Self::NotImplemented => 501,
            Self::ModelUnavailable
            | Self::EmbeddingFailed
            | Self::McpConnectFailed
            | Self::McpToolFailed
            | Self::A2ATaskFailed
            | Self::A2ACardFetchFailed => 502,
            Self::StorageUnavailable | Self::ServiceUnavailable => 503,
            Self::ModelTimeout | Self::ToolTimeout | Self::SandboxTimeout => 504,
        }
    }

    /// 失败分类。
    ///
    /// **穷尽匹配** —— 新增错误码时编译器会强制在这里做出选择，
    /// 不会静默落入某个默认分支。这正是「失败分类是第一公民」的含义（§2.6.3）。
    ///
    /// 关键：**超时类错误属于 `UnknownOutcome`** —— 请求可能已到达上游并产生副作用，
    /// 自动重试会重复扣费 / 重复执行。
    pub const fn retry_policy(self) -> RetryPolicy {
        match self {
            // ── 明确失败，重试安全 ──
            Self::UserLocked
            | Self::RateLimited
            | Self::ModelUnavailable
            | Self::EmbeddingFailed
            | Self::StorageUnavailable
            | Self::McpConnectFailed
            | Self::A2ACardFetchFailed
            | Self::ServiceUnavailable => RetryPolicy::Retryable,

            // ── 结果未知，**不能**自动重试 ──
            Self::ModelTimeout
            | Self::ToolFailed
            | Self::ToolTimeout
            | Self::SandboxTimeout
            | Self::McpToolFailed
            | Self::A2ATaskFailed => RetryPolicy::UnknownOutcome,

            // ── 其余：重试无意义 ──
            Self::InvalidParam
            | Self::MissingParam
            | Self::FileTooLarge
            | Self::FileType
            | Self::Unauthorized
            | Self::TokenExpired
            | Self::TokenInvalid
            | Self::Forbidden
            | Self::LoginFailed
            | Self::UserExists
            | Self::AgentNotFound
            | Self::MaxIteration
            | Self::SessionNotFound
            | Self::RunNotFound
            | Self::WorkflowDsl
            | Self::BudgetExceeded
            | Self::RunConflict
            | Self::EventCursorExpired
            | Self::ProviderNotFound
            | Self::ParseFailed
            | Self::KbNotFound
            | Self::DocumentNotFound
            | Self::MemoryNotFound
            | Self::ToolNotFound
            | Self::SqlForbidden
            | Self::DomainNotAllowed
            | Self::Internal
            | Self::NotImplemented => RetryPolicy::Terminal,
        }
    }

    /// 由码值反查业务码。
    ///
    /// 用于反序列化与「外部传入码值」的校验 —— 未知码值返回 `None`。
    pub fn from_code(code: i32) -> Option<Self> {
        ALL_CODES.iter().copied().find(|c| c.code() == code)
    }
}

/// **手工**实现 `Serialize` —— 派生会输出变体名 `"ModelTimeout"` 而非数字码。
///
/// 这不是细节：前端依赖码段做通用分支处理（§2.6.4）。
impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i32(self.code())
    }
}

/// **手工**实现 `Deserialize` —— 与 `Serialize` 对称，读数字码。
///
/// 派生版本会期望变体名 `"ModelTimeout"`，那与序列化的形状不匹配。
impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let code = i32::deserialize(d)?;
        Self::from_code(code)
            .ok_or_else(|| serde::de::Error::custom(format!("未知的业务码：{code}")))
    }
}

/// 重试策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RetryPolicy {
    /// 明确失败，重试安全。
    Retryable,
    /// **结果未知** —— 请求可能已到达上游并产生副作用，重试可能重复扣费。
    UnknownOutcome,
    /// 明确失败，重试无意义。
    Terminal,
}

/// 用户可见的部分。
///
/// **只有** code 与 message —— 类型上就无法携带内部原因（§2.6.5）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserFacing {
    /// 业务码。
    pub code: ErrorCode,
    /// 面向用户的消息。
    pub message: Cow<'static, str>,
}

impl UserFacing {
    /// 构造。
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 完整错误（含内部原因）。
///
/// **不实现 `Serialize`** —— 要输出给客户端，只能给 [`UserFacing`]。
/// 这是刻意的：省掉一次「不小心把完整错误序列化出去」的机会。
#[derive(Debug)]
pub struct AppError {
    /// 用户可见的部分。
    pub facing: UserFacing,
    /// 内部原因。只进日志，**绝不**序列化。
    pub cause: Option<Arc<dyn Error + Send + Sync>>,
}

impl AppError {
    /// 构造（无内部原因）。
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            facing: UserFacing::new(code, message),
            cause: None,
        }
    }

    /// 附加内部原因。
    pub fn with_cause(mut self, cause: impl Error + Send + Sync + 'static) -> Self {
        self.cause = Some(Arc::new(cause));
        self
    }

    /// 附加字符串形式的内部原因。
    pub fn with_cause_msg(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(Arc::new(SimpleCause(cause.into())));
        self
    }

    /// 失败分类。
    ///
    /// 委托给 [`ErrorCode::retry_policy`] —— 分类是码的**属性**，
    /// 不是错误的属性。
    pub fn retry_policy(&self) -> RetryPolicy {
        self.facing.code.retry_policy()
    }

    /// 用户可见的部分。
    ///
    /// 显式提供这个方法，是为了让「只把 `facing` 交给协议层」成为**唯一**的取法。
    pub fn user_facing(&self) -> &UserFacing {
        &self.facing
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.facing.code.code(), self.facing.message)?;
        if let Some(c) = &self.cause {
            write!(f, ": {c}")?;
        }
        Ok(())
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|c| c.as_ref() as &(dyn Error + 'static))
    }
}

/// 简单的原因类型 —— 让字符串也能作为内部原因。
#[derive(Debug)]
struct SimpleCause(String);

impl fmt::Display for SimpleCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for SimpleCause {}

/// 领域错误：模型调用失败的原因。
///
/// 这是**示例**领域错误 —— 演示 §2.6.2 的三个要点：
/// enum 而非字符串、`#[from]` 自动转换、`match` 穷尽。
///
/// 具体领域的错误 enum 随各 crate 落地时再定，但**模式**由本例确立。
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// 上游超时。
    #[error("上游超时")]
    Timeout,
    /// 上游限流。
    #[error("上游限流")]
    RateLimited {
        /// 建议重试等待时长。
        retry_after: Option<Duration>,
    },
    /// 上游返回错误状态。
    #[error("上游返回错误")]
    Upstream {
        /// HTTP 状态码。
        status: u16,
    },
    /// 响应格式非法。
    #[error("响应格式非法")]
    Malformed(String),
    /// 传输层 IO 失败。
    ///
    /// 演示 `#[from]` 自动转换 —— 这里用 `std::io::Error` 代替传输库的
    /// 错误类型（如 `nexus-llm` 里的 `reqwest::Error`）。
    #[error("上游 IO 失败")]
    UpstreamIo(#[from] std::io::Error),
}

impl From<ModelError> for AppError {
    fn from(e: ModelError) -> Self {
        match e {
            // 超时 → **结果未知**，不能自动重试
            ModelError::Timeout => AppError::new(ErrorCode::ModelTimeout, "模型上游超时"),
            ModelError::RateLimited { .. } => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
            }
            ModelError::Upstream { status } => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试")
                    .with_cause_msg(format!("upstream status {status}"))
            }
            ModelError::Malformed(s) => {
                AppError::new(ErrorCode::Internal, "系统内部错误").with_cause_msg(s)
            }
            ModelError::UpstreamIo(e) => {
                AppError::new(ErrorCode::ModelUnavailable, "模型暂不可用，请稍后再试").with_cause(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **契约测试** —— 码值 + HTTP 状态必须与架构文档 §2.6.4.1 的表完全一致。
    ///
    /// 这份清单是**前后端契约**。改一个码值，已上线的前端分支逻辑就失效。
    #[test]
    fn error_code_table_matches_contract() {
        // (变体, 码值, HTTP 状态, 失败分类)
        const CONTRACT: &[(&str, i32, u16, RetryPolicy)] = &[
            // 1xxxx 参数
            ("InvalidParam", 10001, 400, RetryPolicy::Terminal),
            ("MissingParam", 10002, 400, RetryPolicy::Terminal),
            ("FileTooLarge", 10003, 400, RetryPolicy::Terminal),
            ("FileType", 10004, 400, RetryPolicy::Terminal),
            // 2xxxx 认证
            ("Unauthorized", 20101, 401, RetryPolicy::Terminal),
            ("TokenExpired", 20102, 401, RetryPolicy::Terminal),
            ("TokenInvalid", 20103, 401, RetryPolicy::Terminal),
            ("Forbidden", 20201, 403, RetryPolicy::Terminal),
            ("LoginFailed", 20202, 401, RetryPolicy::Terminal),
            ("UserLocked", 20203, 429, RetryPolicy::Retryable),
            ("UserExists", 20204, 409, RetryPolicy::Terminal),
            // 3xxxx Agent/会话
            ("AgentNotFound", 30001, 404, RetryPolicy::Terminal),
            ("MaxIteration", 30002, 500, RetryPolicy::Terminal),
            ("SessionNotFound", 30003, 404, RetryPolicy::Terminal),
            ("RunNotFound", 30004, 404, RetryPolicy::Terminal),
            ("WorkflowDsl", 30005, 400, RetryPolicy::Terminal),
            ("BudgetExceeded", 30006, 500, RetryPolicy::Terminal),
            ("RunConflict", 30007, 409, RetryPolicy::Terminal),
            ("EventCursorExpired", 30008, 410, RetryPolicy::Terminal),
            // 4xxxx 限流
            ("RateLimited", 40001, 429, RetryPolicy::Retryable),
            // 5xxxx LLM 上游
            ("ModelUnavailable", 50001, 502, RetryPolicy::Retryable),
            ("ModelTimeout", 50002, 504, RetryPolicy::UnknownOutcome),
            ("ProviderNotFound", 50003, 404, RetryPolicy::Terminal),
            ("EmbeddingFailed", 50004, 502, RetryPolicy::Retryable),
            // 6xxxx RAG
            ("ParseFailed", 60002, 500, RetryPolicy::Terminal),
            ("KbNotFound", 60003, 404, RetryPolicy::Terminal),
            ("DocumentNotFound", 60004, 404, RetryPolicy::Terminal),
            ("MemoryNotFound", 60005, 404, RetryPolicy::Terminal),
            ("StorageUnavailable", 60006, 503, RetryPolicy::Retryable),
            // 7xxxx 工具/沙箱
            ("ToolNotFound", 70001, 404, RetryPolicy::Terminal),
            ("ToolFailed", 70002, 500, RetryPolicy::UnknownOutcome),
            ("ToolTimeout", 70003, 504, RetryPolicy::UnknownOutcome),
            ("SandboxTimeout", 70004, 504, RetryPolicy::UnknownOutcome),
            ("SqlForbidden", 70005, 403, RetryPolicy::Terminal),
            ("DomainNotAllowed", 70006, 403, RetryPolicy::Terminal),
            // 8xxxx MCP/A2A
            ("McpConnectFailed", 80001, 502, RetryPolicy::Retryable),
            ("McpToolFailed", 80002, 502, RetryPolicy::UnknownOutcome),
            ("A2ATaskFailed", 80003, 502, RetryPolicy::UnknownOutcome),
            ("A2ACardFetchFailed", 80004, 502, RetryPolicy::Retryable),
            // 9xxxx 系统
            ("Internal", 90001, 500, RetryPolicy::Terminal),
            ("NotImplemented", 90002, 501, RetryPolicy::Terminal),
            ("ServiceUnavailable", 90003, 503, RetryPolicy::Retryable),
        ];

        assert_eq!(
            ALL_CODES.len(),
            CONTRACT.len(),
            "业务码数量与契约不符：实现 {} 个，契约 {} 个",
            ALL_CODES.len(),
            CONTRACT.len()
        );

        for (name, code, http, policy) in CONTRACT {
            let c = ALL_CODES
                .iter()
                .copied()
                .find(|c| format!("{c:?}") == *name)
                .unwrap_or_else(|| panic!("契约里的 {name} 在实现中不存在"));

            assert_eq!(c.code(), *code, "{name} 码值与契约不符");
            assert_eq!(c.http_status(), *http, "{name} HTTP 状态与契约不符");
            assert_eq!(c.retry_policy(), *policy, "{name} 失败分类与契约不符");
        }
    }

    /// 码值唯一。
    #[test]
    fn error_codes_are_unique() {
        let mut codes: Vec<i32> = ALL_CODES.iter().map(|c| c.code()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "业务码重复");
    }

    /// 码段与码值前两位一致，且落在 1..=9。
    #[test]
    fn each_code_range_is_partitioned_correctly() {
        for c in ALL_CODES {
            let seg = c.segment();
            assert!((1..=9).contains(&seg), "{c:?} 码段越界：{seg}");
            assert_eq!(c.code() / 10000, seg as i32, "{c:?} 码值与码段不一致");
        }
    }

    /// 两处「看着像笔误」的地方是参考实现的原样 —— 必须原样保留。
    #[test]
    fn quirky_codes_match_reference_implementation() {
        // 码段 6 从 60002 起（没有 60001）
        assert_eq!(ErrorCode::ParseFailed.code(), 60002);
        assert!(ALL_CODES.iter().all(|c| c.code() != 60001));

        // 码段 9 的 90002 在 90003 之前
        assert_eq!(ErrorCode::NotImplemented.code(), 90002);
        assert_eq!(ErrorCode::ServiceUnavailable.code(), 90003);
    }

    /// 业务码序列化是**数字码**，不是变体名。
    #[test]
    fn error_code_serializes_as_number_not_variant_name() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidParam).unwrap(),
            "10001"
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::ModelTimeout).unwrap(),
            "50002"
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::Internal).unwrap(),
            "90001"
        );
    }

    /// 反序列化也按数字码 —— 与序列化对称。
    #[test]
    fn error_code_deserializes_from_number() {
        let back: ErrorCode = serde_json::from_str("50002").unwrap();
        assert_eq!(back, ErrorCode::ModelTimeout);

        // 未知码值被拒
        assert!(serde_json::from_str::<ErrorCode>("12345").is_err());
    }

    /// 业务码往返序列化。
    #[test]
    fn error_codes_roundtrip() {
        for c in ALL_CODES {
            let json = serde_json::to_string(c).unwrap();
            let back: ErrorCode = serde_json::from_str(&json).unwrap();
            assert_eq!(*c, back);
        }
    }

    /// 码值反查。
    #[test]
    fn from_code_roundtrips() {
        for c in ALL_CODES {
            assert_eq!(ErrorCode::from_code(c.code()), Some(*c));
        }
        assert_eq!(ErrorCode::from_code(1), None);
        assert_eq!(ErrorCode::from_code(60001), None);
    }

    /// 超时类错误是**结果未知** —— 不能自动重试。
    ///
    /// 这是本设计相对参考实现的关键改进：
    /// 超时 → 请求可能已到达上游 → 自动重试会重复扣费。
    #[test]
    fn timeout_errors_map_to_unknown_outcome() {
        for c in [
            ErrorCode::ModelTimeout,
            ErrorCode::ToolFailed,
            ErrorCode::ToolTimeout,
            ErrorCode::SandboxTimeout,
            ErrorCode::McpToolFailed,
            ErrorCode::A2ATaskFailed,
        ] {
            assert_eq!(
                c.retry_policy(),
                RetryPolicy::UnknownOutcome,
                "{c:?} 应归为 UnknownOutcome"
            );
        }
    }

    /// 明确失败的错误可重试。
    #[test]
    fn definite_failure_errors_map_to_retryable() {
        for c in [
            ErrorCode::UserLocked,
            ErrorCode::RateLimited,
            ErrorCode::ModelUnavailable,
            ErrorCode::EmbeddingFailed,
            ErrorCode::StorageUnavailable,
            ErrorCode::McpConnectFailed,
            ErrorCode::A2ACardFetchFailed,
            ErrorCode::ServiceUnavailable,
        ] {
            assert_eq!(
                c.retry_policy(),
                RetryPolicy::Retryable,
                "{c:?} 应归为 Retryable"
            );
        }
    }

    /// 参数错误不重试。
    #[test]
    fn invalid_param_is_terminal() {
        assert_eq!(
            ErrorCode::InvalidParam.retry_policy(),
            RetryPolicy::Terminal
        );
    }

    /// 内部原因**绝不出现在**用户可见部分。
    #[test]
    fn internal_cause_never_appears_in_user_facing() {
        let err = AppError::new(ErrorCode::Internal, "系统内部错误")
            .with_cause(std::io::Error::other("secret=abc123"));

        let json = serde_json::to_string(err.user_facing()).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("abc123"));
    }

    /// `UserFacing` 只有两个字段 —— 类型上无法携带内部原因。
    #[test]
    fn user_facing_has_exactly_two_fields() {
        let facing = UserFacing::new(ErrorCode::Internal, "系统内部错误");
        let obj = serde_json::to_value(&facing).unwrap();
        let keys: Vec<&str> = obj
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(keys.len(), 2, "UserFacing 只能有 code 与 message 两个字段");
        assert!(keys.contains(&"code"));
        assert!(keys.contains(&"message"));
    }

    /// `AppError` **不实现** `Serialize` —— 只能把 `facing` 交给协议层。
    ///
    /// 这条用**编译失败**来保证更合适（见 `compile_fail/`），
    /// 这里的测试只保证 `facing` 可序列化。
    #[test]
    fn user_facing_is_serializable() {
        let err = AppError::new(ErrorCode::InvalidParam, "参数错误");
        let json = serde_json::to_value(err.user_facing()).unwrap();

        assert_eq!(json["code"], 10001);
        assert_eq!(json["message"], "参数错误");
    }

    /// `Display` 含内部原因 —— 供日志使用。
    #[test]
    fn display_includes_cause_for_logging() {
        let err = AppError::new(ErrorCode::Internal, "系统内部错误")
            .with_cause(std::io::Error::other("db password=hunter2"));

        let text = err.to_string();
        assert!(text.contains("90001"));
        assert!(text.contains("系统内部错误"));
        assert!(text.contains("hunter2"), "日志里应有内部原因");
    }

    /// `std::error::Error::source()` 暴露内部原因。
    #[test]
    fn error_source_returns_cause() {
        let err = AppError::new(ErrorCode::Internal, "系统内部错误").with_cause_msg("root cause");

        assert!(err.source().is_some());
        assert_eq!(err.source().unwrap().to_string(), "root cause");
    }

    /// `AppError` 的失败分类委托给码。
    #[test]
    fn app_error_retry_policy_delegates_to_code() {
        let timeout = AppError::new(ErrorCode::ModelTimeout, "超时");
        assert_eq!(timeout.retry_policy(), RetryPolicy::UnknownOutcome);

        let unavailable = AppError::new(ErrorCode::ModelUnavailable, "不可用");
        assert_eq!(unavailable.retry_policy(), RetryPolicy::Retryable);

        let invalid = AppError::new(ErrorCode::InvalidParam, "参数错误");
        assert_eq!(invalid.retry_policy(), RetryPolicy::Terminal);
    }

    /// 领域错误 → 应用错误的转换（§2.6.2 的 `#[from]` 与穷尽匹配模式）。
    #[test]
    fn domain_error_converts_to_app_error() {
        let timeout = AppError::from(ModelError::Timeout);
        assert_eq!(timeout.facing.code, ErrorCode::ModelTimeout);
        assert_eq!(timeout.retry_policy(), RetryPolicy::UnknownOutcome);

        let unavailable = AppError::from(ModelError::RateLimited { retry_after: None });
        assert_eq!(unavailable.facing.code, ErrorCode::ModelUnavailable);
        assert_eq!(unavailable.retry_policy(), RetryPolicy::Retryable);

        let upstream = AppError::from(ModelError::Upstream { status: 503 });
        assert_eq!(upstream.facing.code, ErrorCode::ModelUnavailable);
        assert!(upstream.cause.is_some(), "上游状态码应作为内部原因保留");

        let malformed = AppError::from(ModelError::Malformed("bad json".into()));
        assert_eq!(malformed.facing.code, ErrorCode::Internal);
        assert!(malformed.cause.is_some());
    }

    /// `#[from]` 自动转换 —— 传输层错误 → 领域错误。
    #[test]
    fn transport_error_converts_via_from_attribute() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset");
        let model_error = ModelError::from(io);
        assert!(matches!(model_error, ModelError::UpstreamIo(_)));
    }

    /// 领域错误是 enum —— `match` 穷尽检查。
    #[test]
    fn domain_error_is_exhaustively_matchable() {
        // 这个 match 若新增变体不处理，编译器会报错 —— 这就是价值。
        fn classify(e: &ModelError) -> &'static str {
            match e {
                ModelError::Timeout => "timeout",
                ModelError::RateLimited { .. } => "rate_limited",
                ModelError::Upstream { .. } => "upstream",
                ModelError::Malformed(_) => "malformed",
                ModelError::UpstreamIo(_) => "upstream_io",
            }
        }
        assert_eq!(classify(&ModelError::Timeout), "timeout");
    }
}
