//! 负向样例：给 `UserFacing` 加内部原因字段。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.6.5 —— `UserFacing` 在**类型上**就无法携带内部原因。
//!
//! `UserFacing` 的字段是**公有**的（协议层要读它们），但它只有两个字段 ——
//! `code` 与 `message`。想加一个 `cause` 字段？编译器会拒绝：
//! 「没有这个字段」。
//!
//! 期望的错误：
//! ```text
//! error[E0560]: struct `UserFacing` has no field named `cause`
//! ```

use nexus_core::{ErrorCode, UserFacing};

fn main() {
    // ❌ 违反设计：用户可见的部分不能携带内部原因。
    let _facing = UserFacing {
        code: ErrorCode::Internal,
        message: "系统内部错误".into(),
        cause: Some("db password=hunter2"),
    };
}
