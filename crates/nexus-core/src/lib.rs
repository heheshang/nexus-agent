//! `nexus-core` —— 跨 crate 共享的**纯类型**。
//!
//! # 职责边界
//!
//! 本 crate 只放**跨 crate 共享的纯数据类型与无状态工具**。
//!
//! # 明确**不做**的事
//!
//! - ❌ 不引入 sqlx / axum / reqwest 等重依赖
//! - ❌ 不放任何**有行为**的编排逻辑
//! - ❌ 不做业务判断
//!
//! # 状态
//!
//! 子模块待设计文档定稿后填充。参见
//! `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md`。

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
