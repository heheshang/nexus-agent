//! 传输层错误 —— 三层错误模型的**第三层**。
//!
//! 架构文档 §2.6.1：
//!
//! | 层 | 类型 | 谁处理 |
//! |---|---|---|
//! | 领域错误 | 各领域的 enum | 领域内部 |
//! | 应用错误 | `AppError` | 用例层 → 协议层 |
//! | **传输错误** | `axum` 的 `IntoResponse` | **协议层** |
//!
//! 本 crate 是**协议层**，所以「响应体长什么样」由这里决定。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nexus_core::{AppError, ErrorCode};

/// 协议层的错误响应。
///
/// # 响应体形状
///
/// 只有 `code` 与 `message` —— 架构文档 §2.6.5：
/// **内部原因绝不外泄**。
///
/// 所以这里序列化的是 `UserFacing`（**不是** `AppError`）——
/// `AppError` 根本不实现 `Serialize`，类型上就无法把 `cause` 带出去。
#[derive(Debug)]
pub struct ApiError {
    /// 内部的应用错误。`cause` 只进日志。
    inner: AppError,
}

impl ApiError {
    /// 构造一个**内部错误**。
    ///
    /// 内部原因进 `cause`（只进日志），对外只给「系统内部错误」。
    pub fn internal(cause: impl Into<String>) -> Self {
        Self {
            inner: AppError::new(ErrorCode::Internal, "系统内部错误").with_cause_msg(cause),
        }
    }

    /// 业务码。
    pub fn code(&self) -> ErrorCode {
        self.inner.facing.code
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let facing = self.inner.user_facing();
        let status = StatusCode::from_u16(facing.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        // ⚠️ 只序列化 `UserFacing` —— `AppError` 不实现 `Serialize`，
        // 所以「不小心把内部原因发出去」在**类型上**就不可能。
        let body = serde_json::to_vec(facing).unwrap_or_else(|_| {
            // 理论上不可达（`UserFacing` 的字段都是平凡类型）。
            // 但兜底**必须有** —— 一个空 body 会让客户端拿到 0 字节，
            // 那比一个格式错误的 body 更难排查。
            // ⚠️ 不能用 `br#"..."#` —— 字节串字面量**必须**是 ASCII，
            // 而兜底消息是中文。
            r#"{"code":90001,"message":"系统内部错误"}"#.as_bytes().to_vec()
        });

        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}
