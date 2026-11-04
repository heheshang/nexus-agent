//! demo-error-model
//!
//! 验证设计文档 §2.6 的三层错误模型：
//!
//! 1. **领域错误**是 enum（§2.6.2）—— `match` 穷尽检查
//! 2. **失败分类**是第一公民（§2.6.3）—— `RetryPolicy::UnknownOutcome`
//!    表达「超时后上游可能已执行副作用」，**不能**自动重试
//! 3. **内部原因绝不外泄**（§2.6.5）—— `UserFacing` 在类型上就无法泄露
//! 4. **业务码保留数字分段**（§2.6.4）—— 但必须手工实现 `Serialize`

use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::sync::Arc;

/// 业务错误码。
///
/// 码段沿用参考实现：1=参数 2=认证 3=会话 4=限流 5=LLM 6=RAG 7=工具 8=协议 9=系统。
///
/// **完整 42 个业务码** —— 与设计文档 §2.6.4.1 的契约表一一对应。
/// 码值是前后端契约，不可自行增删或改号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // ── 1xxxx 参数 ──
    InvalidParam,
    MissingParam,
    FileTooLarge,
    FileType,
    // ── 2xxxx 认证 ──
    Unauthorized,
    TokenExpired,
    TokenInvalid,
    Forbidden,
    LoginFailed,
    UserLocked,
    UserExists,
    // ── 3xxxx Agent/会话 ──
    AgentNotFound,
    MaxIteration,
    SessionNotFound,
    RunNotFound,
    WorkflowDsl,
    BudgetExceeded,
    RunConflict,
    EventCursorExpired,
    // ── 4xxxx 限流 ──
    RateLimited,
    // ── 5xxxx LLM 上游 ──
    ModelUnavailable,
    ModelTimeout,
    ProviderNotFound,
    EmbeddingFailed,
    // ── 6xxxx RAG ──
    ParseFailed,
    KbNotFound,
    DocumentNotFound,
    MemoryNotFound,
    StorageUnavailable,
    // ── 7xxxx 工具/沙箱 ──
    ToolNotFound,
    ToolFailed,
    ToolTimeout,
    SandboxTimeout,
    SqlForbidden,
    DomainNotAllowed,
    // ── 8xxxx MCP/A2A ──
    McpConnectFailed,
    McpToolFailed,
    A2ATaskFailed,
    A2ACardFetchFailed,
    // ── 9xxxx 系统 ──
    Internal,
    NotImplemented,
    ServiceUnavailable,
}

/// **全部**业务码 —— 用于契约测试。
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
    pub fn code(&self) -> i32 {
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
            Self::Internal => 90001,
            Self::NotImplemented => 90002,
            Self::ServiceUnavailable => 90003,
        }
    }

    /// 码段。
    pub fn segment(&self) -> u32 {
        (self.code() / 10000) as u32
    }

    /// HTTP 状态码。
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidParam | Self::MissingParam | Self::FileTooLarge | Self::FileType => 400,
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
            Self::UserExists | Self::RunConflict => 409,
            Self::UserLocked | Self::RateLimited => 429,
            Self::MaxIteration | Self::BudgetExceeded | Self::ParseFailed | Self::ToolFailed => 500,
            Self::NotImplemented => 501,
            Self::ModelUnavailable
            | Self::EmbeddingFailed
            | Self::McpConnectFailed
            | Self::McpToolFailed
            | Self::A2ATaskFailed
            | Self::A2ACardFetchFailed => 502,
            Self::StorageUnavailable | Self::ServiceUnavailable => 503,
            Self::ModelTimeout | Self::ToolTimeout | Self::SandboxTimeout => 504,
            Self::EventCursorExpired => 410,
            Self::Internal => 500,
            Self::WorkflowDsl => 400,
        }
    }
}

/// **手工**实现 `Serialize` —— 派生会输出变体名 `"InvalidParam"` 而非数字码。
///
/// 这不是细节：前端依赖码段做通用分支处理。
impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_i32(self.code())
    }
}

/// 重试策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// **只有** code 与 message —— 类型上就无法携带内部原因。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserFacing {
    pub code: ErrorCode,
    pub message: Cow<'static, str>,
}

/// 完整错误（含内部原因）。
#[derive(Debug)]
pub struct AppError {
    pub facing: UserFacing,
    /// 内部原因。只进日志，**绝不**序列化。
    pub cause: Option<Arc<dyn std::error::Error + Send + Sync>>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            facing: UserFacing {
                code,
                message: message.into(),
            },
            cause: None,
        }
    }

    /// 附加内部原因。
    pub fn with_cause(mut self, cause: impl std::error::Error + Send + Sync + 'static) -> Self {
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
    /// **关键**：超时类错误属于 `UnknownOutcome` —— 请求可能已到达上游并产生副作用，
    /// 自动重试会重复扣费 / 重复执行。
    pub fn retry_policy(&self) -> RetryPolicy {
        match self.facing.code {
            // ── 明确失败，重试安全 ──
            ErrorCode::UserLocked
            | ErrorCode::RateLimited
            | ErrorCode::ModelUnavailable
            | ErrorCode::EmbeddingFailed
            | ErrorCode::StorageUnavailable
            | ErrorCode::McpConnectFailed
            | ErrorCode::A2ACardFetchFailed
            | ErrorCode::ServiceUnavailable => RetryPolicy::Retryable,

            // ── 结果未知，**不能**自动重试 ──
            ErrorCode::ModelTimeout
            | ErrorCode::ToolFailed
            | ErrorCode::ToolTimeout
            | ErrorCode::SandboxTimeout
            | ErrorCode::McpToolFailed
            | ErrorCode::A2ATaskFailed => RetryPolicy::UnknownOutcome,

            // ── 其余：重试无意义 ──
            _ => RetryPolicy::Terminal,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.facing.code.code(), self.facing.message)?;
        if let Some(c) = &self.cause {
            write!(f, ": {c}")?;
        }
        Ok(())
    }
}

/// 简单的原因类型 —— 让字符串也能作为内部原因。
#[derive(Debug)]
struct SimpleCause(String);

impl std::fmt::Display for SimpleCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SimpleCause {}

/// 领域错误：模型调用失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("上游超时")]
    Timeout,
    #[error("上游限流")]
    RateLimited {
        retry_after: Option<std::time::Duration>,
    },
    #[error("上游返回错误")]
    Upstream { status: u16 },
    #[error("响应格式非法")]
    Malformed(String),
}

impl From<ModelError> for AppError {
    fn from(e: ModelError) -> Self {
        match e {
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
        }
    }
}

fn main() {
    // ── 1. 数字码序列化 ──
    let json = serde_json::to_string(&ErrorCode::ModelTimeout).unwrap();
    println!("✓ 业务码序列化 = {json}（不是 \"ModelTimeout\"）");
    assert_eq!(json, "50002");

    // ── 2. 码段 ──
    for c in [
        ErrorCode::InvalidParam,
        ErrorCode::ModelTimeout,
        ErrorCode::Internal,
    ] {
        println!("  码 {} → 段 {}", c.code(), c.segment());
    }

    // ── 3. 内部原因不外泄 ──
    let err = AppError::new(ErrorCode::Internal, "系统内部错误")
        .with_cause(std::io::Error::other("db password=hunter2"));
    let facing_json = serde_json::to_string(&err.facing).unwrap();
    println!("\n✓ UserFacing = {facing_json}");
    assert!(!facing_json.contains("hunter2"), "内部原因泄露！");

    // Display 里才有 —— 用于日志
    println!("✓ 日志形态 = {err}");
    assert!(err.to_string().contains("hunter2"));

    // ── 4. 失败分类 ──
    println!("\n--- 失败分类 ---");
    let timeout = AppError::from(ModelError::Timeout);
    let unavailable = AppError::from(ModelError::RateLimited { retry_after: None });
    println!("  超时     → {:?}", timeout.retry_policy());
    println!("  限流     → {:?}", unavailable.retry_policy());
    println!(
        "  参数错误 → {:?}",
        AppError::new(ErrorCode::InvalidParam, "参数错误").retry_policy()
    );
    assert_eq!(timeout.retry_policy(), RetryPolicy::UnknownOutcome);
    assert_eq!(unavailable.retry_policy(), RetryPolicy::Retryable);
    println!("  → 超时与「明确失败」被区分开，不会自动重试");

    // ── 5. 全量码表统计 ──
    println!("\n--- 全量码表（{} 个）---", ALL_CODES.len());
    let mut by_segment: std::collections::BTreeMap<u32, Vec<i32>> = Default::default();
    for c in ALL_CODES {
        by_segment.entry(c.segment()).or_default().push(c.code());
    }
    for (seg, codes) in &by_segment {
        println!("  {seg}xxxx 段：{} 个", codes.len());
    }
    let retryable = ALL_CODES
        .iter()
        .filter(|c| AppError::new(**c, "").retry_policy() == RetryPolicy::Retryable)
        .count();
    let unknown = ALL_CODES
        .iter()
        .filter(|c| AppError::new(**c, "").retry_policy() == RetryPolicy::UnknownOutcome)
        .count();
    let terminal = ALL_CODES.len() - retryable - unknown;
    println!("  可重试 {retryable} 个 / 结果未知 {unknown} 个 / 终止 {terminal} 个");
    println!(
        "  → 结果未知的那 {} 个不会自动重试（避免重复扣费）",
        unknown
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 业务码序列化为数字而非变体名() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidParam).unwrap(),
            "10001"
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::Internal).unwrap(),
            "90001"
        );
    }

    #[test]
    fn 码段划分正确() {
        assert_eq!(ErrorCode::InvalidParam.segment(), 1);
        assert_eq!(ErrorCode::Unauthorized.segment(), 2);
        assert_eq!(ErrorCode::ModelTimeout.segment(), 5);
        assert_eq!(ErrorCode::ToolFailed.segment(), 7);
        assert_eq!(ErrorCode::Internal.segment(), 9);
    }

    #[test]
    fn 业务码互不重复() {
        let mut codes: Vec<i32> = ALL_CODES.iter().map(|c| c.code()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "业务码重复");
    }

    /// **契约测试** —— 码值 + HTTP 状态必须与参考实现
    /// `internal/pkg/errcode/errcode.go` 完全一致。
    ///
    /// 这份清单是**前后端契约**。改一个码值，已上线的前端分支逻辑就失效。
    #[test]
    fn 业务码表与契约一致() {
        // (变体, 码值, HTTP 状态)
        const CONTRACT: &[(&str, i32, u16)] = &[
            ("InvalidParam", 10001, 400),
            ("MissingParam", 10002, 400),
            ("FileTooLarge", 10003, 400),
            ("FileType", 10004, 400),
            ("Unauthorized", 20101, 401),
            ("TokenExpired", 20102, 401),
            ("TokenInvalid", 20103, 401),
            ("Forbidden", 20201, 403),
            ("LoginFailed", 20202, 401),
            ("UserLocked", 20203, 429),
            ("UserExists", 20204, 409),
            ("AgentNotFound", 30001, 404),
            ("MaxIteration", 30002, 500),
            ("SessionNotFound", 30003, 404),
            ("RunNotFound", 30004, 404),
            ("WorkflowDsl", 30005, 400),
            ("BudgetExceeded", 30006, 500),
            ("RunConflict", 30007, 409),
            ("EventCursorExpired", 30008, 410),
            ("RateLimited", 40001, 429),
            ("ModelUnavailable", 50001, 502),
            ("ModelTimeout", 50002, 504),
            ("ProviderNotFound", 50003, 404),
            ("EmbeddingFailed", 50004, 502),
            ("ParseFailed", 60002, 500),
            ("KbNotFound", 60003, 404),
            ("DocumentNotFound", 60004, 404),
            ("MemoryNotFound", 60005, 404),
            ("StorageUnavailable", 60006, 503),
            ("ToolNotFound", 70001, 404),
            ("ToolFailed", 70002, 500),
            ("ToolTimeout", 70003, 504),
            ("SandboxTimeout", 70004, 504),
            ("SqlForbidden", 70005, 403),
            ("DomainNotAllowed", 70006, 403),
            ("McpConnectFailed", 80001, 502),
            ("McpToolFailed", 80002, 502),
            ("A2ATaskFailed", 80003, 502),
            ("A2ACardFetchFailed", 80004, 502),
            ("Internal", 90001, 500),
            ("NotImplemented", 90002, 501),
            ("ServiceUnavailable", 90003, 503),
        ];

        assert_eq!(ALL_CODES.len(), CONTRACT.len(), "业务码数量与契约不符");

        for (name, code, http) in CONTRACT {
            let c = ALL_CODES
                .iter()
                .find(|c| format!("{:?}", c) == *name)
                .unwrap_or_else(|| panic!("契约里的 {name} 在实现中不存在"));
            assert_eq!(c.code(), *code, "{name} 码值与契约不符");
            assert_eq!(c.http_status(), *http, "{name} HTTP 状态与契约不符");
        }
    }

    #[test]
    fn 每个码段都能正确划分() {
        for c in ALL_CODES {
            let seg = c.segment();
            assert!((1..=9).contains(&seg), "{c:?} 码段越界：{seg}");
            // 码值的前两位必须等于码段
            assert_eq!(c.code() / 10000, seg as i32, "{c:?} 码值与码段不一致");
        }
    }

    #[test]
    fn 超时类错误都归为_结果未知() {
        // 这组是本设计相对参考实现的关键改进：
        // 超时 → 请求可能已到达上游 → 自动重试会重复扣费
        for c in [
            ErrorCode::ModelTimeout,
            ErrorCode::ToolFailed,
            ErrorCode::ToolTimeout,
            ErrorCode::SandboxTimeout,
            ErrorCode::McpToolFailed,
            ErrorCode::A2ATaskFailed,
        ] {
            let e = AppError::new(c, "x");
            assert_eq!(
                e.retry_policy(),
                RetryPolicy::UnknownOutcome,
                "{c:?} 应归为 UnknownOutcome"
            );
        }
    }

    #[test]
    fn 明确失败类错误归为_可重试() {
        for c in [
            ErrorCode::RateLimited,
            ErrorCode::ModelUnavailable,
            ErrorCode::StorageUnavailable,
            ErrorCode::McpConnectFailed,
            ErrorCode::ServiceUnavailable,
        ] {
            let e = AppError::new(c, "x");
            assert_eq!(
                e.retry_policy(),
                RetryPolicy::Retryable,
                "{c:?} 应归为 Retryable"
            );
        }
    }

    #[test]
    fn 内部原因绝不出现在_user_facing_里() {
        let err = AppError::new(ErrorCode::Internal, "系统内部错误")
            .with_cause(std::io::Error::other("secret=abc123"));
        let json = serde_json::to_string(&err.facing).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("abc123"));
    }

    #[test]
    fn 超时属于结果未知而非可直接重试() {
        let timeout = AppError::from(ModelError::Timeout);
        assert_eq!(timeout.retry_policy(), RetryPolicy::UnknownOutcome);

        // 明确失败可重试
        let unavail = AppError::from(ModelError::RateLimited { retry_after: None });
        assert_eq!(unavail.retry_policy(), RetryPolicy::Retryable);

        // 参数错误不重试
        assert_eq!(
            AppError::new(ErrorCode::InvalidParam, "参数错误").retry_policy(),
            RetryPolicy::Terminal
        );
    }

    #[test]
    fn 领域错误可穷尽匹配() {
        // 这个 match 若新增变体不处理，编译器会报错 —— 这就是价值。
        fn classify(e: &ModelError) -> &'static str {
            match e {
                ModelError::Timeout => "timeout",
                ModelError::RateLimited { .. } => "rate_limited",
                ModelError::Upstream { .. } => "upstream",
                ModelError::Malformed(_) => "malformed",
            }
        }
        assert_eq!(classify(&ModelError::Timeout), "timeout");
    }
}
