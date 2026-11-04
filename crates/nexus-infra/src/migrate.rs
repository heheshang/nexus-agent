//! 迁移基线。
//!
//! 迁移文件在 `crates/nexus-infra/migrations/`，用 [`sqlx::migrate!`] **嵌入二进制** ——
//! 部署时不需要额外拷贝目录（§3.9.3）。
//!
//! # Phase 0 范围
//!
//! 参考实现有 42 表 + 1 视图。Rust 版**按 Phase 渐进建表**（§3.9.4）——
//! 核心表 Phase 0 一次建齐，避免后期反复改表。
//!
//! | 迁移 | 内容 | 对应参考实现 |
//! |---|---|---|
//! | 1 | `users` / `model_providers` / `model_configs` | 迁移 001、003 |
//! | 2 | `agents` / `mcp_servers` / `tools` | 迁移 001、003、005、010、011 |
//! | 3 | `workflows` / `workflow_versions` | 迁移 006 |
//! | 4 | `execution_runs` / `workflow_runs` / `workflow_step_runs` | 迁移 004、006 |
//! | 5 | `execution_*` 支撑表（6 张） | 迁移 004 |
//! | 6 | `sessions` / `messages` | 迁移 001、009 |
//!
//! 其余表（`documents` / `memories` / `a2a_*` / `eval_*` / `usage_logs` 等）
//! 留给后续 Phase。
//!
//! # 两处刻意偏离
//!
//! **1. `execution_runs` 独立新建** —— 参考实现给 `workflow_runs` 加 11 列
//! （历史兼容）。Rust 版拆成「引擎侧 `execution_runs`」+「产品侧 `workflow_runs`」
//! 两张表，避免历史数据迁移的复杂度（§3.9.4）。
//!
//! **2. 按 Phase 建表** —— 参考实现一次建完 42 表。

use sqlx::PgPool;
use sqlx::migrate::MigrateError;

pub use sqlx::migrate::Migrator;

/// 嵌入式迁移器。
///
/// [`sqlx::migrate!`] 在**编译期**读 `migrations/` 目录并把内容嵌进二进制。
/// 目录里没被识别的文件会被**静默忽略** —— 所以
/// [`MIGRATION_TABLE_NAMES`] 断言是必需的，不是可选的保险。
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");
/// Phase 0 应存在的表。
///
/// **这张清单是「迁移被静默跳过」的防线** —— 见 [`assert_phase0_tables`]。
pub const PHASE0_TABLES: &[&str] = &[
    // 1
    "users",
    "model_providers",
    "model_configs",
    // 2
    "agents",
    "mcp_servers",
    "tools",
    // 3
    "workflows",
    "workflow_versions",
    // 4
    "execution_runs",
    "workflow_runs",
    "workflow_step_runs",
    // 5
    "execution_checkpoints",
    "execution_operations",
    "execution_attempts",
    "execution_approvals",
    "execution_events",
    "execution_outbox",
    // 6
    "sessions",
    "messages",
];

/// 应用所有迁移。
///
/// **幂等** —— 已应用的版本会被跳过（sqlx 记录在 `_sqlx_migrations` 表）。
///
/// # 错误
///
/// 返回 [`MigrateError`]。调用方应把它包装成 `AppError`（`ErrorCode::StorageUnavailable`）。
pub async fn run(pool: &PgPool) -> Result<(), MigrateError> {
    MIGRATOR.run(pool).await
}

/// 回滚到指定版本。
///
/// # 错误
///
/// 返回 [`MigrateError`]。
pub async fn undo(pool: &PgPool, target: i64) -> Result<(), MigrateError> {
    MIGRATOR.undo(pool, target).await
}

/// 当前已应用的最高版本。
///
/// 没有应用过任何迁移时返回 `0`。
pub async fn current_version(pool: &PgPool) -> Result<i64, MigrateError> {
    let applied: Vec<(i64,)> = sqlx::query_as("SELECT version FROM _sqlx_migrations")
        .fetch_all(pool)
        .await?;
    Ok(applied.iter().map(|(v,)| *v).max().unwrap_or(0))
}

/// 断言 Phase 0 的表**全部存在**。
///
/// # 为什么需要这个断言
///
/// sqlx 解析迁移文件名时，格式不符的文件会被**静默忽略**
/// （`sqlx-core-0.9.0/src/migrate/source.rs:210`）——
/// 那个迁移**根本没跑**，且**不报错**。
///
/// 「迁移跑完了」与「表建对了」是两件事。只有断言具体表存在，
/// 才能发现「某个迁移被跳过了」。
///
/// # 错误
///
/// 缺表时返回错误信息，列出**缺了哪些**。
pub async fn assert_phase0_tables(pool: &PgPool) -> Result<(), String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询现有表失败：{e}"))?;

    let existing: std::collections::HashSet<&str> = rows.iter().map(|(t,)| t.as_str()).collect();

    let missing: Vec<&str> = PHASE0_TABLES
        .iter()
        .copied()
        .filter(|t| !existing.contains(t))
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Phase 0 表缺失：{}。\
             若迁移文件刚新增却没生效，检查文件名格式 —— \
             sqlx 会**静默忽略**不符合 `<整数版本号>_<描述>.sql` 的文件。",
            missing.join(", ")
        ))
    }
}

/// 断言某个索引**存在且带 `WHERE` 子句**（部分索引）。
///
/// 部分索引是「只索引就绪行」的关键（§3.9.1）——
/// 如果 `WHERE` 子句丢了，索引会退化成全表索引，
/// 在大表上从毫秒级变成秒级。
///
/// # 错误
///
/// 索引不存在、或不带 `WHERE` 子句时返回错误。
pub async fn assert_partial_index(pool: &PgPool, index_name: &str) -> Result<(), String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT indexdef FROM pg_indexes WHERE indexname = $1")
            .bind(index_name)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("查询索引 {index_name} 失败：{e}"))?;

    match rows.first() {
        None => Err(format!("索引 {index_name} 不存在")),
        Some((def,)) if def.contains(" WHERE ") => Ok(()),
        Some((def,)) => Err(format!(
            "索引 {index_name} 不是部分索引（缺少 WHERE 子句）：{def}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrator_is_embedded() {
        // 迁移器非空 —— 编译期就把目录读进来了。
        assert!(!MIGRATOR.migrations.is_empty());
    }

    #[test]
    fn every_migration_has_a_matching_up_and_down() {
        // 每个版本号都应同时有 up 与 down —— 缺一个就意味着回滚不了。
        let mut ups = std::collections::HashSet::new();
        let mut downs = std::collections::HashSet::new();

        for m in MIGRATOR.migrations.iter() {
            if m.migration_type.is_up_migration() {
                ups.insert(m.version);
            } else if m.migration_type.is_down_migration() {
                downs.insert(m.version);
            }
        }

        assert!(!ups.is_empty(), "应至少有一个 up 迁移");
        assert_eq!(
            ups, downs,
            "up 与 down 的版本号应一一对应 —— 缺一个就回滚不了"
        );
    }

    #[test]
    fn migration_versions_are_unique_per_direction() {
        // ⚠️ up 与 down **共享同一个版本号** —— 这是 sqlx 0.9 可逆迁移的设计
        // （官方测试 `sqlx-0.9.0/tests/postgres/migrate.rs:75` 用
        // `20220721124650_add_table.up.sql` / `.down.sql` 同一版本号）。
        //
        // 所以版本号在**同一个方向内**唯一，而不是全局唯一。
        let mut ups = std::collections::HashSet::new();
        let mut downs = std::collections::HashSet::new();

        for m in MIGRATOR.migrations.iter() {
            if m.migration_type.is_up_migration() {
                assert!(ups.insert(m.version), "up 迁移的版本号重复：{}", m.version);
            } else if m.migration_type.is_down_migration() {
                assert!(
                    downs.insert(m.version),
                    "down 迁移的版本号重复：{}",
                    m.version
                );
            }
        }

        assert!(!ups.is_empty(), "应至少有一个 up 迁移");
    }

    #[test]
    fn phase0_table_list_is_not_empty() {
        assert_eq!(
            PHASE0_TABLES.len(),
            19,
            "Phase 0 应有 19 张表 —— 清单与迁移不一致，说明有一方漏了"
        );
        // 清单里不该有重复项。
        let mut sorted = PHASE0_TABLES.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "Phase 0 表清单里有重复");
    }

    #[test]
    fn execution_runs_is_a_separate_table_from_workflow_runs() {
        // 「两表分离」是架构文档 §3.9.4 的明文偏离 —— 用测试锁住它，
        // 免得后人「顺手」把它们合回去。
        assert!(PHASE0_TABLES.contains(&"execution_runs"));
        assert!(PHASE0_TABLES.contains(&"workflow_runs"));
    }
}
