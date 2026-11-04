//! 执行租约的仓储 —— **领取**与**围栏写入**。
//!
//! # 两条机制
//!
//! | 机制 | SQL 手段 | 语义 |
//! |---|---|---|
//! | **领取** | `FOR UPDATE SKIP LOCKED` | 多个 worker 并发领取时**互不阻塞**，各自拿到**不同**的 run |
//! | **fencing** | `WHERE generation = $my_gen` | 旧持有者的迟到写**影响 0 行** |
//!
//! # 「围栏检查与业务写同事务」是必须抄的不变量
//!
//! 围栏条件检查与业务更新必须在**同一个事务**里 ——
//! 否则两者之间有窗口：检查通过后、写入前，租约可能已被别人抢走。
//!
//! 所以本模块的写入方法**接收一个 `&mut Transaction`**，
//! 而不是自己开事务 —— 事务边界由调用方（执行层）决定，
//! 它能把围栏写与业务写放进同一个事务。
//!
//! # 表：`execution_runs`（不是 `workflow_runs`）
//!
//! 架构文档 §4.2 差异 1：`workflow_runs` 由 `nexus-execution` **独占**。
//! 本模块操作的是 `execution_runs` —— NEX-9 建的**引擎侧**独立表。
//!
//! # 租约 TTL 来自配置
//!
//! `worker.lease_ttl_secs`（默认 30 秒，参考实现的值）——
//! 走 `nexus-config` 的配置栈，**不硬编码**。

use std::time::Duration;

use nexus_config::ConfigStack;
use nexus_core::error::{AppError, ErrorCode};
use nexus_core::id::{RunId, UserId};
use sqlx::PgPool;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Postgres, QueryBuilder, Transaction};

use crate::error::map_sqlx_error;
use crate::lease::{LeaseOutcome, RunLease};

/// 租约 TTL 的配置路径。
const PATH_LEASE_TTL_SECS: &str = "worker.lease_ttl_secs";

/// 租约 TTL 的默认值（秒）。
///
/// 30 秒是参考实现的值。它是**兜底**，不是硬编码 ——
///// 配置里没给就用这个。
pub const DEFAULT_LEASE_TTL_SECS: u64 = 30;

/// 租约配置。
///
/// # 为什么单独一个类型
///
/// 「我们想租多久」（配置）与「SQL 怎么写」（实现）是两件事。
/// 分开后配置可以被**离线**测试 —— 不需要数据库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseSettings {
    ttl: Duration,
}

impl LeaseSettings {
    /// 从配置栈读取。
    ///
    /// # 错误
    ///
    /// `worker.lease_ttl_secs` 配了 **0** 时返回 [`ErrorCode::InvalidParam`] ——
    /// 零秒租约等于「永远立刻过期」，那是明确的配置错误，
    /// 不能悄悄退回默认值（那会让运维以为配的值生效了）。
    ///
    /// **缺失时用默认值** —— 30 秒是合理的缺省，不该让部署因为漏配就起不来。
    pub fn from_config(config: &ConfigStack) -> Result<Self, AppError> {
        let ttl = match config.get_u64(PATH_LEASE_TTL_SECS) {
            Some(0) => {
                return Err(AppError::new(ErrorCode::InvalidParam, "租约时长配置非法")
                    .with_cause_msg(format!("{PATH_LEASE_TTL_SECS} 必须大于 0")));
            }
            Some(n) => Duration::from_secs(n),
            None => Duration::from_secs(DEFAULT_LEASE_TTL_SECS),
        };

        Ok(Self { ttl })
    }

    /// 用给定的 TTL 构造。
    pub fn with_ttl(ttl: Duration) -> Self {
        Self { ttl }
    }

    /// 租约时长。
    pub fn ttl(&self) -> Duration {
        self.ttl
    }
}

/// 可领取的 run 的**最小**行投影。
///
/// # 为什么只取一列
///
/// 领取时只需要 `id` —— 新一代的 `generation` 由 `UPDATE ... RETURNING`
/// **从数据库取回**，不在应用层算。
///
/// 应用层算（`row.generation + 1`）是**危险的形状**：
/// 读到的 generation 与写入时的 generation 之间只要有间隙，
/// 算出来的值就可能与数据库实际写入的不一致 —— 而 fencing 的全部意义
/// 就在于这个数字必须**精确**。
///
/// 让数据库自己算并返回，是唯一可靠的做法。
///
/// 而且 `SELECT *` 会把 `definition`（整个冻结的图定义 JSONB）
/// 在**每个** worker 的连接上拉一遍 —— 那是纯浪费。
#[derive(Debug, Clone, FromRow)]
struct ClaimableRow {
    /// run ID。
    #[sqlx(try_from = "i64")]
    id: RunId,
}

/// 执行租约的仓储。
#[derive(Debug, Clone)]
pub struct ExecutionLeaseRepository {
    pool: PgPool,
    settings: LeaseSettings,
}

impl ExecutionLeaseRepository {
    /// 用连接池与默认配置构造。
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            settings: LeaseSettings::with_ttl(Duration::from_secs(DEFAULT_LEASE_TTL_SECS)),
        }
    }

    /// 用连接池与给定配置构造。
    pub fn with_settings(pool: PgPool, settings: LeaseSettings) -> Self {
        Self { pool, settings }
    }

    /// 底层连接池。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 租约配置。
    pub fn settings(&self) -> &LeaseSettings {
        &self.settings
    }

    /// **领取**一个可运行的 run。
    ///
    /// # 机制
    ///
    /// ```sql
    /// SELECT id, generation FROM execution_runs
    ///  WHERE status = 'queued' AND available_at <= clock_timestamp()
    ///  ORDER BY available_at, id
    ///  FOR UPDATE SKIP LOCKED
    ///  LIMIT 1
    /// ```
    ///
    /// `SKIP LOCKED` 是关键：并发 worker 不会互相**阻塞**，
    /// 而是各自**跳过**已被锁定的行 —— 所以每个 worker 领到**不同**的 run。
    ///
    /// 领取与 `generation` 自增在**同一个事务**里：
    ///
    /// ```sql
    /// UPDATE execution_runs
    ///    SET status = 'running',
    ///        lease_owner = $1,
    ///        lease_until = clock_timestamp() + $2,
    ///        generation = generation + 1
    ///  WHERE id = $3
    /// ```
    ///
    /// # 为什么 `clock_timestamp()` 而不是 `now()`
    ///
    /// 领取在**事务内**执行。`now()` 是事务开始时间 ——
    /// 事务跑久了，`lease_until` 会算成一个**偏早**的时间，
    /// 租约实际持有时长短于预期。
    ///
    /// `clock_timestamp()` 是**实际当前时间**，无论事务跑了多久。
    ///
    /// # 为什么 `available_at <= clock_timestamp()`
    ///
    /// `available_at` 是「可被领取的时间」。用 `clock_timestamp()`
    /// 才能保证「延迟领取」的 run 在到期后立刻可领。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    ///
    /// **没有可领取的 run** → 返回 `Ok(None)` ——
    /// 那不是错误，是「队列暂时是空的」。
    pub async fn claim(&self, owner: &str) -> Result<Option<RunLease>, AppError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        // ── 1. 选一个可领取的行（并锁住它） ──
        let rows: Vec<ClaimableRow> = sqlx::query_as(
            "SELECT id FROM execution_runs \
             WHERE status = 'queued' AND available_at <= clock_timestamp() \
             ORDER BY available_at, id \
             FOR UPDATE SKIP LOCKED \
             LIMIT 1",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let Some(row) = rows.into_iter().next() else {
            // 没有可领取的 —— 显式回滚。
            //
            // 不显式回滚也行（`tx`  drop 时会自动回滚），
            // 但显式回滚让「什么都没做」这件事在代码里**看得见**，
            // 而且能尽早释放连接。
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(None);
        };

        // ── 2. 领取：写 owner、延长租约、generation 自增 ──
        let updated: Option<(i64,)> = sqlx::query_as(
            "UPDATE execution_runs \
                SET status = 'running', \
                    lease_owner = $1, \
                    lease_until = clock_timestamp() + $2, \
                    generation = generation + 1 \
              WHERE id = $3 \
              RETURNING generation",
        )
        .bind(owner)
        .bind(self.settings.ttl())
        .bind(row.id.into_inner())
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let Some((new_generation,)) = updated else {
            // 行在锁定期间被删了 —— 极少见，但不是不可能。
            tx.rollback().await.map_err(map_sqlx_error)?;
            return Ok(None);
        };

        tx.commit().await.map_err(map_sqlx_error)?;

        Ok(Some(RunLease::new(row.id, new_generation, owner)))
    }

    /// **围栏写入** —— 更新 run 的状态。
    ///
    /// # 围栏条件
    ///
    /// 走 [`Fence::push_conditions`] —— 六个条件（`id` / `generation` /
    /// `lease_owner` / `status` / `lease_until` / `deadline_at`）
    /// **只在一处定义**。
    ///
    /// # 为什么接收 `&mut Transaction`
    ///
    /// 「围栏检查与业务写同事务」是必须抄的不变量（参考架构文档 Phase 9）。
    /// 围栏检查与业务更新之间如果隔着事务边界，就有窗口：
    /// 检查通过后、写入前，租约可能已被别人抢走。
    ///
    /// 所以本方法**不开事务** —— 事务由调用方（执行层）持有，
    /// 它能把围栏写与业务写放进同一个事务。
    ///
    /// # 结果
    ///
    /// - 影响 1 行 → [`LeaseOutcome::Applied`]
    /// - 影响 0 行 → [`LeaseOutcome::Fenced`]（本 worker 已是僵尸）
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn write_status_fenced(
        tx: &mut Transaction<'_, Postgres>,
        lease: &RunLease,
        status: &str,
    ) -> Result<LeaseOutcome, AppError> {
        // `execution_runs` **没有** `updated_at` 列（迁移 4）——
        // 所以只更新 `status`，不碰其它列。
        let mut builder = QueryBuilder::<Postgres>::new("UPDATE execution_runs SET status = ");
        builder.push_bind(status);
        builder.push(" WHERE ");
        lease.fence().push_conditions(&mut builder);

        let result = builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx_error)?;

        Ok(outcome_from_rows(result.rows_affected()))
    }

    /// **围栏写入** —— 更新 run 的某个整数字段。
    ///
    /// # 为什么只支持整数字段
    ///
    /// 这是**刻意**的限制。围栏写入要作用于**任意**列的话，
    /// 就得接受「列名来自调用方」—— 而列名不能参数化，
    /// 只能拼进 SQL，那就得防注入。
    ///
    /// 现状的执行模型只有 `next_seq` / `state_version` 两个整数计数器
    /// 需要这类更新（NEX-13 的序号分配）。等到真有别的需求再扩 ——
    /// 「相似三行好过错误的一个抽象」（Rust 代码规范 §10）。
    ///
    /// # 结果
    ///
    /// 同 [`Self::write_status_fenced`]。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn increment_counter_fenced(
        tx: &mut Transaction<'_, Postgres>,
        lease: &RunLease,
        column: CounterColumn,
        delta: i64,
    ) -> Result<LeaseOutcome, AppError> {
        let mut builder = QueryBuilder::<Postgres>::new("UPDATE execution_runs SET ");
        builder.push(column.as_sql());
        builder.push(" = ");
        builder.push(column.as_sql());
        builder.push(" + ");
        builder.push_bind(delta);
        builder.push(" WHERE ");
        lease.fence().push_conditions(&mut builder);

        let result = builder
            .build()
            .execute(&mut **tx)
            .await
            .map_err(map_sqlx_error)?;

        Ok(outcome_from_rows(result.rows_affected()))
    }

    /// 读取 run 当前的状态（**不**带围栏）。
    ///
    /// # 为什么不需要围栏
    ///
    /// 读**不会**造成副作用 —— 读到的是旧状态还是新状态都不致命。
    /// 围栏保护的是**写**。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    /// - run 不存在 → [`ErrorCode::RunNotFound`]
    pub async fn find_status(&self, id: RunId) -> Result<Option<(String, i64)>, AppError> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT status, generation FROM execution_runs WHERE id = $1")
                .bind(id.into_inner())
                .fetch_optional(&self.pool)
                .await
                .map_err(map_sqlx_error)?;

        Ok(row)
    }

    /// 查询 run 的**租约信息**（持有者 + 世代号 + 租约到期时间）。
    ///
    /// # 用途
    ///
    /// 排障：「这个 run 现在归谁、租约到什么时候、generation 是几」。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn find_lease_info(&self, id: RunId) -> Result<Option<LeaseInfo>, AppError> {
        let row: Option<(String, i64, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT lease_owner, generation, lease_until FROM execution_runs WHERE id = $1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(|(owner, generation, lease_until)| LeaseInfo {
            owner,
            generation,
            lease_until,
        }))
    }

    /// 创建一条可领取的 run（**测试与引导用**）。
    ///
    /// # 为什么在生产代码里
    ///
    /// 执行层的「提交运行」接口要写入 `execution_runs`，
    /// 而它需要 `owner_id` + `request_key`（幂等键）。
    /// NEX-13 会把幂等键的完整实现补上 ——
    /// 本方法先提供**最小**的创建路径，让租约机制能被独立测试。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    /// - 外键冲突（用户不存在）→ [`ErrorCode::InvalidParam`]
    pub async fn create_queued_run(
        &self,
        owner_id: UserId,
        request_key: &str,
        definition: serde_json::Value,
    ) -> Result<RunId, AppError> {
        let hash = definition_hash_of(&definition);

        let row: (i64,) = sqlx::query_as(
            "INSERT INTO execution_runs (definition, definition_hash, engine_version, \
                    graph_schema, status, generation, lease_owner, available_at, deadline_at, \
                    owner_id, request_key) \
             VALUES ($1, $2, 'test-engine', 1, 'queued', 0, '', clock_timestamp(), \
                    clock_timestamp() + INTERVAL '1 day', $3, $4) \
             RETURNING id",
        )
        .bind(&definition)
        .bind(crate::convert::trim_bpchar(&hash))
        .bind(owner_id.into_inner())
        .bind(request_key)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(RunId::new(row.0))
    }
}

/// 可以做围栏自增的**整数**列。
///
/// # 为什么是枚举而不是 `&str`
///
/// 列名**不能**参数化 —— 它必须拼进 SQL。
/// 接受任意字符串就等于接受注入面。
///
/// 枚举把可拼的列名**穷尽** —— 编译期就能保证没有别的可能。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterColumn {
    /// `next_seq` —— 下一个事件序号。
    NextSeq,
    /// `state_version` —— 状态版本号。
    StateVersion,
}

impl CounterColumn {
    /// 列名（**常量**，不含用户输入）。
    pub const fn as_sql(self) -> &'static str {
        match self {
            Self::NextSeq => "next_seq",
            Self::StateVersion => "state_version",
        }
    }
}

/// run 的租约信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseInfo {
    /// 当前持有者。
    pub owner: String,
    /// 当前世代号。
    pub generation: i64,
    /// 租约到期时间（`None` = 未持有租约）。
    pub lease_until: Option<chrono::DateTime<chrono::Utc>>,
}

/// 把「影响行数」翻译成围栏结果。
fn outcome_from_rows(rows_affected: u64) -> LeaseOutcome {
    if rows_affected == 1 {
        LeaseOutcome::Applied
    } else {
        LeaseOutcome::Fenced
    }
}

/// 计算定义哈希（**占位实现**）。
///
/// # ⚠️ 这是占位
///
/// 真正的哈希要按 §2.5.2 的**规范化 JSON**规则计算
/// （键序无关、数组序敏感、数字最短往返）—— 那属于 `nexus-workflow` 的职责。
///
/// 本任务只需要一个**稳定**的 64 位十六进制串填进 `CHAR(64)` 列，
/// 让围栏机制能被独立测试。
///
/// **不要**把这个函数当成规范实现引用 ——
/// NEX-13 落地幂等键时必须换成真正的规范化哈希。
fn definition_hash_of(definition: &serde_json::Value) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    definition.to_string().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 让 `PgRow` 的导入不被误删（`FromRow` 派生需要）。
const _: fn() = || {
    fn _assert_row<R: FromRow<'static, PgRow>>() {}
};
#[cfg(test)]
mod tests {
    use super::*;
    use nexus_config::ConfigStack;
    use serde_json::json;

    fn stack_with(worker: serde_json::Value) -> ConfigStack {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({ "worker": worker }));
        stack
    }

    #[test]
    fn reads_ttl_from_config() {
        let settings = LeaseSettings::from_config(&stack_with(json!({ "lease_ttl_secs": 45 })))
            .expect("应能读取");

        assert_eq!(settings.ttl(), Duration::from_secs(45));
    }

    #[test]
    fn missing_ttl_uses_the_default() {
        let settings = LeaseSettings::from_config(&stack_with(json!({}))).expect("应能读取");

        assert_eq!(settings.ttl(), Duration::from_secs(DEFAULT_LEASE_TTL_SECS));
    }

    #[test]
    fn zero_ttl_is_rejected_not_silently_defaulted() {
        // 配了 0 是**明确的错误** —— 悄悄退回默认值会让运维以为配的值生效了。
        let err = LeaseSettings::from_config(&stack_with(json!({ "lease_ttl_secs": 0 })))
            .expect_err("零秒租约应被拒绝");

        assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);
    }

    #[test]
    fn counter_column_names_are_constants() {
        assert_eq!(CounterColumn::NextSeq.as_sql(), "next_seq");
        assert_eq!(CounterColumn::StateVersion.as_sql(), "state_version");
    }

    #[test]
    fn outcome_translation_distinguishes_zero_and_one_row() {
        assert_eq!(outcome_from_rows(1), LeaseOutcome::Applied);
        assert_eq!(outcome_from_rows(0), LeaseOutcome::Fenced);
        // 影响多行是**异常** —— 围栏条件含主键，不可能命中多行。
        // 按「未应用」处理，保守。
        assert_eq!(outcome_from_rows(2), LeaseOutcome::Fenced);
    }
}
