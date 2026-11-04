//! 执行 outbox 与幂等键的**仓储**。
//!
//! # 两条机制
//!
//! | 机制 | SQL 手段 | 语义 |
//! |---|---|---|
//! | **事务 outbox** | 事件与状态在**同一事务**写入 `execution_outbox` | 投递是**至少一次**，消费方按 `(run_id, seq)` 去重 |
//! | **幂等键** | `ON CONFLICT (owner_id, request_key) DO NOTHING` | 重复提交**返回既有 run**，不报错 |
//!
//! # 表：`execution_outbox` / `execution_events` / `execution_runs`
//!
//! 三张表都属于**引擎侧** —— `execution_runs` 是 NEX-9 建的独立表，
//! 不涉及 `workflow_runs`（那由 `nexus-execution` 独占，架构文档 §4.2 差异 1）。
//!
//! # 「同一事务」是**调用方的义务**
//!
//! 本模块的写入方法接收 `&mut Transaction` ——
//! **不开自己的事务**。这样调用方（执行层）能把
//! 「序号分配 + 事件写入 + outbox 写入」放进**同一个事务**。
//!
//! 为什么不让本模块自己开事务？因为它**不知道**调用方还要写什么 ——
//! 状态更新（`status` / `state_version`）也在同一个事务里，
//! 那是调用方的事。
//!
//! 参考实现 `appendEvent`（`internal/execution/store.go:421`）就是这么做的。

use nexus_core::error::AppError;
use nexus_core::id::{RunId, UserId};
use sqlx::PgPool;
use sqlx::{FromRow, Postgres, Transaction};

use crate::error::map_sqlx_error;
use crate::lease::RunLease;
use crate::outbox::{OutboxMessage, RequestKey, SubmitOutcome};
use crate::rows::NullableId;

/// outbox 默认批量领取条数。
///
/// 100 是参考实现的行为量级 —— 一次领取的量要够摊薄事务开销，
/// 又不能多到让单次事务持锁太久。
pub const DEFAULT_OUTBOX_BATCH: usize = 100;

/// outbox 仓储。
#[derive(Debug, Clone)]
pub struct OutboxRepository {
    pool: PgPool,
}

impl OutboxRepository {
    /// 用连接池构造。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 底层连接池。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// **原子分配事件序号**并写入事件与 outbox。
    ///
    /// # 三件事在**同一个事务**里
    ///
    /// 1. `UPDATE execution_runs SET next_seq = next_seq + 1 ... RETURNING next_seq`
    ///    —— 序号由**数据库**分配
    /// 2. `INSERT execution_events (run_id, seq, type, data)`
    /// 3. `INSERT execution_outbox (run_id, seq, topic, payload)`
    ///
    /// # 为什么序号必须由**数据库**分配
    ///
    /// 可能有多**个** writer 进程。应用层自增（`seq = last + 1`）
    /// 在并发下会**重复分配**同一个序号 —— 而 `(run_id, seq)` 是
    /// outbox 的唯一键，重复会撞约束。
    ///
    /// `next_seq = next_seq + 1 RETURNING next_seq` 是**原子的**：
    /// 读改写由数据库在**行锁**保护下完成。
    ///
    /// # 与 `nexus-core` 的 `EventSink` 的关系
    ///
    /// 两者是**不同层次**的不变量，不要混淆：
    ///
    /// | | 序号 |
    /// |---|---|
    /// | `EventSink`（NEX-5） | **进程内**的单次运行流序号，由 `&mut self` 保证严格单调 |
    /// | 本方法 | **持久化的**、**跨进程**的序号，由数据库原子分配 |
    ///
    /// 持久化场景必须用后者 —— 因为有多个 writer 进程。
    ///
    /// # 围栏
    ///
    /// 序号分配**带围栏条件** —— 僵尸 worker 不能分配序号。
    /// 围栏失败（影响 0 行）时返回 [`ErrorCode::RunConflict`]。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    /// - 围栏拒绝（僵尸写入）→ [`ErrorCode::RunConflict`]
    ///
    /// # 为什么是**关联函数**而不是方法
    ///
    /// 它不需要连接池 —— 所有 SQL 都跑在调用方传进来的 `tx` 上。
    /// 做成方法会多一个**用不到的** `&self`（clippy 会告警），
    /// 而且会让人误以为它需要仓储自己的连接。
    pub async fn append_event(
        tx: &mut Transaction<'_, Postgres>,
        lease: &RunLease,
        event_type: &str,
        data: &serde_json::Value,
    ) -> Result<AppendedEvent, AppError> {
        // ── 1. 原子分配序号（带围栏） ──
        //
        // `next_seq = next_seq + 1 RETURNING next_seq` —— 读改写由数据库
        // 在行锁保护下完成，跨进程也单调。
        //
        // 围栏条件由 `RunLease` 提供 —— 僵尸 worker 影响 0 行。
        let mut builder = sqlx::QueryBuilder::<Postgres>::new(
            "UPDATE execution_runs SET next_seq = next_seq + 1, \
                    state_version = state_version + 1 ",
        );
        builder.push(" WHERE ");
        lease.fence().push_conditions(&mut builder);
        builder.push(" RETURNING next_seq");

        let seq: Option<(i64,)> = builder
            .build_query_as()
            .fetch_optional(&mut **tx)
            .await
            .map_err(map_sqlx_error)?;

        let Some((seq,)) = seq else {
            // 影响 0 行 → 围栏拒绝 —— 本 worker 已是僵尸。
            return Err(crate::lease::LeaseLost::new(lease.run_id()).into());
        };

        // ── 2. 写事件流 ──
        sqlx::query(
            "INSERT INTO execution_events (run_id, seq, type, schema_version, data) \
             VALUES ($1, $2, $3, 1, $4)",
        )
        .bind(lease.run_id().into_inner())
        .bind(seq)
        .bind(event_type)
        .bind(data)
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx_error)?;

        // ── 3. 写 outbox（同一事务） ──
        //
        // 负载是**扁平的** `{"type": ..., ...}` —— 与 SSE 协议同形，
        // 消费方不需要为了读 outbox 而再解一层。
        let payload = serde_json::json!({
            "type": event_type,
            "data": data,
        });

        sqlx::query(
            "INSERT INTO execution_outbox (run_id, seq, topic, payload, dedup_key) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(lease.run_id().into_inner())
        .bind(seq)
        .bind(outbox_topic(event_type))
        .bind(&payload)
        .bind(dedup_key_of(lease.run_id(), seq))
        .execute(&mut **tx)
        .await
        .map_err(map_sqlx_error)?;

        Ok(AppendedEvent { seq })
    }

    /// **领取**待投递的 outbox 消息（`FOR UPDATE SKIP LOCKED`）。
    ///
    /// # 机制
    ///
    /// ```sql
    /// SELECT ... FROM execution_outbox
    ///  WHERE published_at IS NULL
    ///  ORDER BY id
    ///  LIMIT $1
    ///  FOR UPDATE SKIP LOCKED
    /// ```
    ///
    /// `SKIP LOCKED` 是关键：并发投递方**不会互相阻塞**，
    /// 而是各自**跳过**已被锁定的行 —— 所以每个投递方领到**不同的**消息。
    ///
    /// 这与 NEX-12 的租约领取是**同一个手法**。
    ///
    /// # 事务边界
    ///
    /// 本方法**开自己的事务**并**保持它开启** —— 返回的
    /// [`OutboxTransaction`] 持有事务，调用方投递完之后
    /// 用 [`OutboxTransaction::mark_published`] 标记，
    /// 然后 [`OutboxTransaction::commit`] 提交。
    ///
    /// 为什么不在本方法里就提交？因为**投递必须在行锁释放前完成** ——
    /// 否则两个投递方会领到同一条消息。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn claim_batch(&self, batch: usize) -> Result<OutboxTransaction, AppError> {
        let mut tx = self.pool.begin().await.map_err(map_sqlx_error)?;

        let rows: Vec<OutboxRow> = sqlx::query_as(
            "SELECT id, run_id, seq, topic, payload, dedup_key FROM execution_outbox \
             WHERE published_at IS NULL \
             ORDER BY id \
             LIMIT $1 \
             FOR UPDATE SKIP LOCKED",
        )
        .bind(batch as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(map_sqlx_error)?;

        let messages = rows.into_iter().map(OutboxRow::into_message).collect();

        Ok(OutboxTransaction { tx, messages })
    }

    /// **幂等提交** —— 重复提交返回**既有**的 run。
    ///
    /// # 机制
    ///
    /// ```sql
    /// INSERT INTO execution_runs (..., owner_id, request_key)
    /// VALUES (...)
    /// ON CONFLICT (owner_id, request_key) DO NOTHING
    /// RETURNING id
    /// ```
    ///
    /// `DO NOTHING` + `RETURNING` 的组合是关键：
    ///
    /// - **首次**提交 → 插入成功，`RETURNING` 返回新 id
    /// - **重复**提交 → 冲突被吞掉，`RETURNING` 返回**空**
    ///   → 此时**回查**既有 run 的 id 返回
    ///
    /// # 为什么重复提交**不是错误**
    ///
    /// 「重复提交返回既有 run」**就是幂等的定义**。
    /// 报冲突错误会让客户端无法区分「网络重试导致的重复」
    /// 与「真的提交了两次不同的东西」。
    ///
    /// # 唯一约束
    ///
    /// `(owner_id, request_key)` 上的唯一索引由 NEX-9 的迁移建好
    /// （`idx_execution_request_key`）。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    /// - 幂等键非法（超长 / 空）→ [`ErrorCode::InvalidParam`]
    /// - 外键冲突（owner 不存在）→ [`ErrorCode::InvalidParam`]
    pub async fn submit_idempotent(
        &self,
        owner_id: UserId,
        request_key: &RequestKey,
        definition: &serde_json::Value,
    ) -> Result<SubmitOutcome, AppError> {
        let hash = crate::leases::definition_hash_of(definition);

        // 首次尝试：插入并返回 id。
        let inserted: Option<(i64,)> = sqlx::query_as(
            "INSERT INTO execution_runs (definition, definition_hash, engine_version, \
                    graph_schema, status, generation, lease_owner, available_at, deadline_at, \
                    owner_id, request_key) \
             VALUES ($1, $2, 'nexus-engine', 1, 'queued', 0, '', clock_timestamp(), \
                    clock_timestamp() + INTERVAL '1 day', $3, $4) \
             ON CONFLICT (owner_id, request_key) DO NOTHING \
             RETURNING id",
        )
        .bind(definition)
        .bind(crate::convert::trim_bpchar(&hash))
        .bind(owner_id.into_inner())
        .bind(request_key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        if let Some((id,)) = inserted {
            return Ok(SubmitOutcome::Created(RunId::new(id)));
        }

        // 冲突被吞掉 → 回查既有 run。
        //
        // 为什么**必须**回查而不是直接报错：
        // 「重复提交返回既有 run」就是幂等的定义。
        let existing: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM execution_runs WHERE owner_id = $1 AND request_key = $2",
        )
        .bind(owner_id.into_inner())
        .bind(request_key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        match existing {
            Some((id,)) => Ok(SubmitOutcome::Existing(RunId::new(id))),
            // 回查不到 —— 极少见：并发下另一个事务插入后又删除了。
            // 那不是「幂等命中」，是**数据不一致**，必须报错而不是假装成功。
            None => Err(AppError::new(
                nexus_core::error::ErrorCode::RunNotFound,
                "幂等提交结果不一致，请重试",
            )
            .with_cause_msg("ON CONFLICT DO NOTHING 命中冲突，但回查不到既有 run")),
        }
    }
}

/// 写入成功后返回的**序号**。
///
/// # 为什么单独一个类型
///
/// 「分配了序号」是一个**事实** —— 调用方需要它来关联后续操作
/// （比如标记某个检查点对应的序号）。
/// 直接返回 `i64` 会让调用方不知道这个数字**是什么**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendedEvent {
    /// 数据库分配的序号。
    pub seq: i64,
}

/// outbox 领取的事务句柄。
///
/// # 为什么持有事务
///
/// 领取后**必须**保持行锁直到投递完成 ——
/// 否则两个投递方会领到同一条消息。
///
/// 但「投递」是**调用方的事**（P6 的范围）——
/// 所以这个类型只提供「标记已投递」与「提交/回滚」，
/// **不**提供投递逻辑。
///
/// # 生命周期
///
/// ```text
/// let batch = repo.claim_batch(100).await?;
/// for msg in batch.messages() {
///     deliver(msg).await?;          // ← 调用方投递
///     batch.mark_published(msg.id()).await?;
/// }
/// batch.commit().await?;
/// ```
pub struct OutboxTransaction {
    tx: Transaction<'static, Postgres>,
    messages: Vec<OutboxMessage>,
}

impl std::fmt::Debug for OutboxTransaction {
    /// 手写 `Debug` —— `Transaction` 的 `Debug` 可能泄露连接信息。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboxTransaction")
            .field("messages", &self.messages.len())
            .finish()
    }
}

impl OutboxTransaction {
    /// 领取到的消息。
    pub fn messages(&self) -> &[OutboxMessage] {
        &self.messages
    }

    /// 标记一条消息**已投递**。
    ///
    /// # 与业务更新在**同一事务**
    ///
    /// 本方法**不开新事务** —— 它在领取时开的那**同一个事务**里更新。
    /// 所以「投递成功」与「标记已投递」是**原子的**：
    /// 事务回滚 → 消息**未被标记** → 下次会被**重投**。
    ///
    /// 这正是「至少一次」的来源 ——
    /// 对端可能已经收到，但标记没提交，于是重投。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn mark_published(&mut self, id: i64) -> Result<(), AppError> {
        sqlx::query("UPDATE execution_outbox SET published_at = clock_timestamp() WHERE id = $1")
            .bind(id)
            .execute(&mut *self.tx)
            .await
            .map_err(map_sqlx_error)?;

        Ok(())
    }

    /// 提交事务 —— 释放行锁。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn commit(self) -> Result<(), AppError> {
        self.tx.commit().await.map_err(map_sqlx_error)
    }

    /// 回滚事务 —— 消息**未被标记**，下次会被重投。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn rollback(self) -> Result<(), AppError> {
        self.tx.rollback().await.map_err(map_sqlx_error)
    }
}

/// outbox 行（**领取**用的最小投影）。
///
/// # 为什么只取需要的列
///
/// `payload` 可能很大 —— `SELECT *` 会把 outbox 里的**全部**负载
/// 在每个投递方的连接上拉一遍。
#[derive(Debug, Clone, FromRow)]
struct OutboxRow {
    /// outbox 行 ID。
    id: i64,
    /// 所属运行（可空）。
    #[sqlx(try_from = "Option<i64>")]
    run_id: NullableId<RunId>,
    /// 序号（可空）。
    seq: Option<i64>,
    /// 主题。
    topic: String,
    /// 负载。
    payload: serde_json::Value,
    /// 去重键（可空）。
    dedup_key: Option<String>,
}

impl OutboxRow {
    /// 转换成领域类型。
    fn into_message(self) -> OutboxMessage {
        OutboxMessage {
            id: self.id,
            run_id: self.run_id.get(),
            seq: self.seq,
            topic: self.topic,
            payload: self.payload,
            dedup_key: self.dedup_key,
        }
    }
}

/// outbox 的**主题**。
///
/// # 规则
///
/// 主题是事件类型本身 —— `run.delta` / `run.done`。
///
/// 消费方按主题路由，所以它必须是**稳定的**：
/// 改主题等于改事件协议（§2.2.4 事件名是前后端契约）。
fn outbox_topic(event_type: &str) -> String {
    format!("run.{event_type}")
}

/// 去重键。
///
/// # 格式
///
/// `run:{run_id}:{seq}` —— 与 `(run_id, seq)` 一一对应。
///
/// 为什么还要一个 `dedup_key` 列（`idx_outbox_dedup_key` 上有唯一索引）？
/// 因为它让「同一条消息」在**跨表**的场景下也能去重 ——
/// 消费方不必理解 `(run_id, seq)` 的二元结构，只比对一个字符串。
fn dedup_key_of(run_id: RunId, seq: i64) -> String {
    format!("run:{}:{}", run_id.into_inner(), seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_is_derived_from_the_event_type() {
        assert_eq!(outbox_topic("delta"), "run.delta");
        assert_eq!(outbox_topic("done"), "run.done");
    }

    #[test]
    fn dedup_key_encodes_both_halves_of_the_tuple() {
        let key = dedup_key_of(RunId::new(7), 3);

        assert_eq!(key, "run:7:3");
    }

    #[test]
    fn dedup_keys_of_different_sequences_differ() {
        // 同一 run 的不同 seq 必须产生不同的去重键 ——
        // 否则第二条消息会被唯一索引挡住。
        assert_ne!(
            dedup_key_of(RunId::new(7), 3),
            dedup_key_of(RunId::new(7), 4)
        );
    }

    #[test]
    fn dedup_keys_of_different_runs_differ() {
        assert_ne!(
            dedup_key_of(RunId::new(7), 3),
            dedup_key_of(RunId::new(8), 3)
        );
    }
}
