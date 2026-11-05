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
//!
//! # 响应体形状由 [`crate::response`] 决定
//!
//! 本模块**不自己**定义响应形状 —— 它复用统一响应包
//! （[`ApiResponse`]），这样 REST 端点的成功与失败响应**形状一致**。
//!
//! 形状：`{"code": <业务码>, "message": "..."}`（失败时无 `data`）。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nexus_core::{AppError, ErrorCode};

use crate::response::ApiResponse;

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

    /// 构造一个**业务错误**（带**面向用户**的文案）。
    ///
    /// ⚠️ `message` 会**原样**进入响应体 ——
    /// **绝不能**把内部原因传进来。
    pub fn business(code: ErrorCode, message: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self {
            inner: AppError::new(code, message),
        }
    }

    /// 从 `AppError` 构造。
    ///
    /// 这是**唯一**能把领域/应用层错误带进协议层的入口 ——
    /// 它只取 `UserFacing`，`cause` **没有**外泄的路径。
    pub fn from_app_error(err: AppError) -> Self {
        Self { inner: err }
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
        let body = ApiResponse::from_facing(facing).to_bytes();

        (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response()
    }
}
