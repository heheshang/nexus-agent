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
//! - [`redis`] —— Redis 客户端（Redis 部分）
//! - [`s3`] —— S3 / MinIO 客户端（S3 部分）
//! - [`milvus`] —— Milvus 客户端（Milvus 部分）
//!
//! # 四个存储的权威性不同（§1.3）
//!
//! | 存储 | 权威性 | 不可用时 |
//! |---|---|---|
//! | PostgreSQL | **唯一权威** | 业务不可用 |
//! | Redis | 纯派生 · 可丢弃 | **降级但可用** |
//! | Milvus | 纯派生 · 可重建 | 检索降级为「仅关键词」 |
//! | S3 / MinIO | 纯派生 · 可重建 | 上传/下载失败，其余可用 |
//!
//! 所以除了 PG，另外三个客户端的构造失败**都不该**让进程起不来 ——
//! 调用方应当记录告警并降级。

pub mod migrate;
pub mod milvus;
pub mod pg;
pub mod redis;
pub mod s3;

pub use migrate::{
    Migrator, PHASE0_TABLES, assert_partial_index, assert_phase0_tables, current_version,
    run as run_migrations, undo as undo_migrations,
};
pub use milvus::{
    DEFAULT_CONNECT_BUDGET_SECS as DEFAULT_MILVUS_CONNECT_BUDGET_SECS,
    DEFAULT_TIMEOUT_SECS as DEFAULT_MILVUS_TIMEOUT_SECS, MilvusHandle, MilvusSettings,
    connect as connect_milvus, connect_with as connect_milvus_with,
};
pub use pg::{
    DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_MAX_CONNECTIONS, PgHandle, PgSettings,
    SafeConnectOptions, connect, connect_lazy, connect_options, connect_with, pool_options,
    safe_connect_options,
};
pub use redis::{
    DEFAULT_CONNECT_TIMEOUT_SECS as DEFAULT_REDIS_CONNECT_TIMEOUT_SECS, RedisHandle, RedisSettings,
    connect as connect_redis, connect_lazy as connect_redis_lazy,
    connect_lazy_with as connect_redis_lazy_with, connect_with as connect_redis_with,
    manager_config as redis_manager_config,
};
pub use s3::{
    DEFAULT_REGION as DEFAULT_S3_REGION, S3Handle, S3Settings, connect as connect_s3,
    connect_with as connect_s3_with,
};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
