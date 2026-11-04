//! 数据访问层的错误映射。
//!
//! # 为什么需要这一层
//!
//! `sqlx::Error` 是**驱动层**的错误 —— 它泄露 SQL 文本、约束名、表名。
//! 直接把它返回给上层，等于把内部结构暴露给调用方。
//!
//! 架构文档 §2.6.5 要求：**内部原因绝不外泄**，对外只有
//! `UserFacing { code, message }`。所以本模块是 `sqlx::Error` → `AppError`
//! 的**唯一**转换点。
//!
//! # 映射原则
//!
//! | sqlx 错误 | 业务码 | 重试策略 | 理由 |
//! |---|---|---|---|
//! | 连接 / IO / 池超时 | `StorageUnavailable` | `Retryable` | 明确失败，重试安全 |
//! | 唯一约束冲突 | `UserExists`（按表定制） | `Terminal` | 重复提交，重试无意义 |
//! | 外键约束冲突 | `InvalidParam` | `Terminal` | 调用方给了非法的关联 ID |
//! | 非空 / CHECK 约束冲突 | `InvalidParam` | `Terminal` | 调用方给了非法值 |
//! | 其余数据库错误 | `Internal` | `Terminal` | 未知故障，不该让调用方重试 |
//! | 行不存在 | 由调用方决定 | — | 「查不到」在不同语境下码不同 |
//!
//! **行不存在不在这里统一映射** —— `users` 查不到是 `UserNotFound` 类的语义，
//! `sessions` 查不到是 `SessionNotFound`。统一映射成 `Internal` 会丢掉这个信息。

use nexus_core::error::{AppError, ErrorCode};

/// 把 `sqlx` 错误映射为 [`AppError`]。
///
/// **默认映射**：无法归类的数据库错误 → `Internal`（终止型）。
///
/// # 为什么不用 `StorageUnavailable` 作为默认
///
/// `StorageUnavailable` 是 `Retryable` —— 调用方会自动重试。
/// 如果把「SQL 写错了」这种**永久性**故障也标成可重试，
/// 故障会被反复放大。默认值必须是**保守**的那一个。
///
/// # 错误
///
/// 无 —— 本函数**永不失败**。
pub fn map_sqlx_error(err: sqlx::Error) -> AppError {
    match err {
        // 连接层故障：明确失败，重试安全。
        sqlx::Error::Io(io_err) => {
            AppError::new(ErrorCode::StorageUnavailable, "数据库暂不可用，请稍后再试")
                .with_cause(io_err)
        }
        sqlx::Error::PoolTimedOut => {
            AppError::new(ErrorCode::StorageUnavailable, "数据库繁忙，请稍后再试")
        }
        sqlx::Error::PoolClosed => AppError::new(
            ErrorCode::StorageUnavailable,
            "数据库连接已关闭，请稍后再试",
        ),
        sqlx::Error::Tls(tls_err) => {
            AppError::new(ErrorCode::StorageUnavailable, "数据库暂不可用，请稍后再试")
                .with_cause_msg(format!("TLS 握手失败：{tls_err}"))
        }
        // 数据库返回了错误。
        sqlx::Error::Database(db_err) => map_database_error(db_err),
        // 协议 / 编解码 / 列找不到 —— 编程错误，不是运行时故障。
        sqlx::Error::Configuration(cfg_err) => AppError::new(ErrorCode::Internal, "数据库操作失败")
            .with_cause_msg(format!("配置错误：{cfg_err}")),
        sqlx::Error::ColumnNotFound(col) => AppError::new(ErrorCode::Internal, "数据库操作失败")
            .with_cause_msg(format!("列不存在：{col}")),
        sqlx::Error::ColumnDecode { index, source } => {
            AppError::new(ErrorCode::Internal, "数据库操作失败")
                .with_cause_msg(format!("列 {index} 解码失败：{source}"))
        }
        sqlx::Error::Decode(decode_err) => AppError::new(ErrorCode::Internal, "数据库操作失败")
            .with_cause_msg(format!("解码失败：{decode_err}")),
        sqlx::Error::Encode(encode_err) => AppError::new(ErrorCode::Internal, "数据库操作失败")
            .with_cause_msg(format!("编码失败：{encode_err}")),
        sqlx::Error::Protocol(msg) => AppError::new(ErrorCode::Internal, "数据库操作失败")
            .with_cause_msg(format!("协议错误：{msg}")),
        other => AppError::new(ErrorCode::Internal, "数据库操作失败").with_cause(other),
    }
}

/// 把数据库返回的错误映射为 [`AppError`]。
///
/// `cause` 里带**约束名**与**表名** —— 它们只进日志，不进对外响应。
fn map_database_error(db_err: Box<dyn sqlx::error::DatabaseError>) -> AppError {
    let kind = db_err.kind();
    let constraint = db_err.constraint().unwrap_or_default().to_string();
    let table = db_err.table().unwrap_or_default().to_string();

    match kind {
        sqlx::error::ErrorKind::UniqueViolation => {
            AppError::new(ErrorCode::UserExists, "记录已存在")
                .with_cause_msg(format!("唯一约束冲突：表 {table} 约束 {constraint}"))
        }
        sqlx::error::ErrorKind::ForeignKeyViolation => {
            AppError::new(ErrorCode::InvalidParam, "关联的记录不存在")
                .with_cause_msg(format!("外键约束冲突：表 {table} 约束 {constraint}"))
        }
        sqlx::error::ErrorKind::NotNullViolation | sqlx::error::ErrorKind::CheckViolation => {
            AppError::new(ErrorCode::InvalidParam, "参数不合法")
                .with_cause_msg(format!("约束冲突：表 {table} 约束 {constraint}"))
        }
        _ => AppError::new(ErrorCode::Internal, "数据库操作失败").with_cause_msg(format!(
            "数据库错误：表 {table} 约束 {constraint}：{db_err}"
        )),
    }
}
