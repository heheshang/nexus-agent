//! nexus-infra
//!
//! **基础设施层**：PG / Redis / S3 / Milvus 的**客户端构造**。
//!
//! 架构文档 §4.3.3 划定的边界：
//!
//! | 本 crate 负责 | 本 crate **不**负责 |
//! |---|---|
//! | 构造并持有客户端 | 任何业务语义（怎么用这些客户端） |
//! | 从 `nexus-config` 读配置 | 定义环境变量名（那由 `nexus-config` 的映射表负责） |
//! | 池参数、超时、连接策略 | SQL 语句（那是 `nexus-repo` 的事） |
//!
//! # 依赖方向
//!
//! §4.4 规则 4：`nexus-infra` 只依赖底层 crate
//! （`nexus-core` / `nexus-config` / `nexus-observability`），
//! **不依赖任何上层**。`scripts/check-deps.sh` 规则 4 强制这一点。
//!
//! # 子模块
//!
//! - [`pg`] —— PostgreSQL 连接池（PG 部分）
//! - [`migrate`] —— 嵌入式迁移基线

pub mod migrate;
pub mod pg;

pub use migrate::{
    Migrator, PHASE0_TABLES, assert_partial_index, assert_phase0_tables, current_version,
    run as run_migrations, undo as undo_migrations,
};
pub use pg::{
    DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_MAX_CONNECTIONS, PgHandle, PgSettings,
    SafeConnectOptions, connect, connect_lazy, connect_options, connect_with, pool_options,
    safe_connect_options,
};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
