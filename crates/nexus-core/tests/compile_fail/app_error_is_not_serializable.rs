//! 负向样例：把 `AppError` 整体序列化。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.6.5 —— **内部原因绝不外泄**。
//!
//! `AppError` **刻意不实现 `Serialize`** —— 它带着 `cause`（内部原因，
//! 可能含数据库口令、上游 URL 等）。要输出给客户端，只能给 `UserFacing`。
//! 不实现 `Serialize` 从类型上消灭了「不小心把完整错误发给用户」的可能。
//!
//! 期望的错误：
//! ```text
//! error[E0277]: the trait bound `AppError: Serialize` is not satisfied
//! ```

use nexus_core::{AppError, ErrorCode};

fn main() {
    let err = AppError::new(ErrorCode::Internal, "系统内部错误")
        .with_cause_msg("db password=hunter2");

    // ❌ 违反设计：完整错误不可序列化 —— 只有 `facing` 可以。
    let _json = serde_json::to_string(&err).unwrap();
}
