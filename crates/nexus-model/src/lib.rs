//! nexus-model
//!
//! **纯 DTO crate** —— 跨 crate 共享的领域类型与请求/响应结构。
//!
//! # 职责边界（架构文档 §4.2 差异 2）
//!
//! | crate | 内容 | 禁止 |
//! |---|---|---|
//! | `nexus-model` | **领域类型**（本 crate） | 任何数据库痕迹（`FromRow` / `sqlx::`） |
//! | `nexus-repo` | **行类型**（`#[derive(FromRow)]`）+ 仓储 | 业务语义 |
//!
//! 两者**分开** —— 这是差异 2 的核心：改表结构不会波及 API 契约。
//!
//! # 强制约束
//!
//! 本 crate **不得**出现 `FromRow` / `sqlx::` / `sqlx_macros` ——
//! 由 `scripts/check-deps.sh` 规则 6 扫描源码强制。
//!
//! # 依赖位置
//!
//! 只依赖 `nexus-core`（类型化 ID、错误码）+ `serde` / `serde_json` / `chrono` / `uuid`。
//! **零内部依赖之外的重依赖** —— 没有 sqlx、没有 axum、没有 tokio。

pub mod session;
pub mod user;

pub use session::{Message, NewSession, Session};
pub use user::{NewUser, User, UserCredential};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
