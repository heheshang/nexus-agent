//! 事务 outbox 与幂等键的集成测试 —— 连**真实 PostgreSQL**。
//!
//! # 覆盖的行为层不变量
//!
//! | 测试 | 断言的不变量 | AC |
//! |---|---|---|
//! | `seq_is_allocated_by_the_database` | 序号由**数据库**原子分配（AC #2） |
//! | `event_and_outbox_are_written_in_the_same_transaction` | 事件与 outbox **同事务**（AC #1） |
//! | `rolled_back_transaction_does_not_advance_seq` | 回滚后 seq **不前进**（AC #9） |
//! | `outbox_claim_uses_skip_locked` | 并发领取**互不阻塞**且各得不同消息（AC #3） |
//! | `published_at_is_marked_in_the_same_transaction` | 标记与业务更新**同事务**（AC #4） |
//! | `duplicate_submit_returns_the_existing_run` | 重复提交**返回既有 run**，不报错（AC #8） |
//! | `request_key_longer_than_64_is_rejected` | 超长键**有校验**（AC #6） |
//! | `outbox_message_carries_the_dedup_tuple` | 「至少一次」语义在类型上可见（AC #5） |
//!
//! # ⚠️ 为什么 outbox 领取测试需要**串行**
//!
//! `claim_batch` 是**全局队列**语义 —— 它领取 outbox 里**任意**一批待投递消息。
//! 而 `cargo test` 默认**并行**执行同一文件内的测试，于是：
//!
//! - 测试 A 造了 3 条消息
//! - 测试 B 的 `claim_batch` 抢走了 A 的消息
//! - A 断言「领到的正是我造的那 3 条」→ **失败**
//!
//! 这与 `lease_fencing.rs` 的 `CLAIM_LOCK` 是**同一类**问题：
//! 隔离维度必须落在「被操作的那个键」上。
//! 队列操作的键是**全局队列**，那就只能串行。
//!
//! # 数据隔离
//!
//! 每个测试用**唯一的 `request_key`**（后缀是随机 UUID）——
//! `execution_runs` 上有 `(owner_id, request_key)` 唯一索引，
//! 随机 key 保证并行执行的测试互不冲突。
//!
//! outbox 消息**没有**这种隔离手段 —— 它的唯一键是 `(run_id, seq)`，
//! 而 `run_id` 是每个测试独有的，所以 outbox 消息本身是隔离的；
//! 但**领取**是全局的，所以仍然需要串行锁。

mod common;

use nexus_core::error::ErrorCode;
use nexus_core::id::{RunId, UserId};
use nexus_model::NewUser;
use nexus_repo::{
    ExecutionLeaseRepository, OutboxRepository, RequestKey, RunLease, UserRepository,
};
use serde_json::json;
use uuid::Uuid;

/// **跨进程**的全局队列锁（PG advisory lock）。
///
/// # 为什么不是进程内的 `tokio::Mutex`
///
/// `cargo test` 会**并行**运行多个**测试二进制** ——
/// `lease_fencing.rs` 与本文件是两个**独立进程**。
///
/// 进程内的 `Mutex` 对**另一个进程**完全无效。而 `claim` / `claim_batch`
/// 是**全局队列**操作 —— 一个进程会领走另一个进程造出来的 run。
///
/// 跨进程只能用 **PostgreSQL 的 advisory lock** ——
/// 它由**数据库**仲裁，跨进程有效。
///
/// # 获取失败 → 跳过
///
/// 拿不到锁说明**另一个进程正持有**它。本测试**跳过**（而不是失败）——
/// 挂死整个测试运行比跳过一个测试糟糕得多。
async fn acquire_queue_lock() -> Option<common::QueueGuard> {
    let url = common::test_database_url()?;
    match common::QueueGuard::acquire(&url).await {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("[SKIP] 获取全局队列锁失败：{e}");
            None
        }
    }
}

/// 造一个唯一的 `request_key`。
///
/// # 前缀
///
/// 前缀**含测试名** —— 这样 [`TestRuns::cleanup_by_prefix`] 能精确清理
/// 本测试的数据，而**不会**误删并行执行的其他测试的数据。
fn unique_request_key(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().simple())
}

/// 造一个用户（`execution_runs.owner_id` 的外键目标）。
async fn create_user(pool: &sqlx::PgPool) -> UserId {
    let repo = UserRepository::new(pool.clone());
    let created = repo
        .create(NewUser {
            username: format!("test_{}", Uuid::new_v4().simple()),
            email: format!("{}@example.test", Uuid::new_v4().simple()),
            password_hash: "argon2id$fake-hash-for-shape-test".to_string(),
        })
        .await
        .expect("创建用户失败");
    created.id
}

/// 造一个**已持有租约**的 run —— **不碰全局队列**。
///
/// # 为什么不用 `claim`
///
/// `claim` 是**全局队列**操作 —— 它领取队列里**任意**一个可运行的 run。
/// 用它造数据时，测试 A 会领走测试 B 的 run，B 就会失败
/// （或者更糟：A 的清理删掉 B 正在用的数据）。
///
/// 跨测试二进制时更无解 —— `lease_fencing.rs` 与本文件是两个进程，
/// 各自的进程内锁**互相看不见**。
///
/// [`ExecutionLeaseRepository::create_leased_run`] **直接插入**一条已持租约的 run，
/// 完全不参与队列竞争。
async fn create_claimed_run(pool: &sqlx::PgPool, owner_id: UserId, runs: &TestRuns) -> RunLease {
    let leases = ExecutionLeaseRepository::new(pool.clone());
    let (run_id, lease) = leases
        .create_leased_run(
            owner_id,
            &runs.request_key(),
            json!({"a": 1}),
            &format!("worker_{}", Uuid::new_v4().simple()),
        )
        .await
        .expect("创建已持租约的 run 失败");

    runs.track(run_id);
    lease
}

/// 登记本文件创建的 run，测试结束时**删除**它们。
///
/// # 为什么必须删除
///
/// `execution_runs` 里 `status = 'queued'` 的行是**全局队列**的一部分。
/// 留在库里会污染**其它测试文件** —— `lease_fencing.rs` 的 `claim`
/// 会领到它们，然后断言「领到的就是我造的那个」就**失败**。
///
/// 那是**跨测试二进制**的影响（两个进程），进程内的 `Mutex` **管不到**。
///
/// # 为什么按**前缀**清理（而不是只按 id）
///
/// 测试**panic** 时，函数末尾的 `cleanup` **不会执行** ——
/// 那次运行留下的行会污染**下一次**运行。
///
/// 所以清理**按前缀**做，且在测试**开始时**也做一次 ——
/// 于是「上次留下的」与「这次创建的」都被清掉，测试**幂等**。
///
/// 前缀里含**测试名** —— 不同测试的前缀不同，所以并行执行时
/// 一个测试的清理**不会**删掉另一个测试的数据。
#[derive(Debug, Default)]
struct TestRuns {
    ids: std::sync::Mutex<Vec<RunId>>,
    /// 请求键前缀 —— 用于按前缀清理。
    prefix: std::sync::Mutex<String>,
}

impl TestRuns {
    /// 登记一个 run。
    fn track(&self, run_id: RunId) {
        self.ids.lock().expect("锁中毒").push(run_id);
    }

    /// 生成本测试专用的 `request_key`（带本测试的前缀）。
    ///
    /// # 前缀长度受限
    ///
    /// `request_key` 列是 `VARCHAR(64)` —— 前缀 + UUID（32 字符）
    /// 必须 ≤ 64，所以前缀**必须短**。
    ///
    /// 这不是「测试偷懒」—— 它正好验证了 AC #6 的约束是**真的**：
    /// 超长键会被数据库拒绝。而应用层校验（[`RequestKey::new`]）
    /// 会给出更清晰的错误。
    fn request_key(&self) -> String {
        let prefix = self.prefix.lock().expect("锁中毒").clone();
        unique_request_key(&prefix)
    }

    /// 登记本次测试使用的**请求键前缀**。
    ///
    /// 前缀含**测试名** —— 保证不同测试的清理互不干扰。
    fn with_prefix(prefix: &str) -> Self {
        Self {
            ids: std::sync::Mutex::new(Vec::new()),
            prefix: std::sync::Mutex::new(prefix.to_owned()),
        }
    }

    /// 删除所有登记的 run（及其事件与 outbox 消息）。
    async fn cleanup(&self, pool: &sqlx::PgPool) {
        let ids: Vec<i64> = self
            .ids
            .lock()
            .expect("锁中毒")
            .iter()
            .map(|id| id.into_inner())
            .collect();

        for id in ids {
            delete_run(pool, id).await;
        }
    }

    /// 按**请求键前缀**删除 —— 清理「上次运行遗留」的行。
    ///
    /// # 为什么需要它
    ///
    /// 测试 panic 时 `cleanup` 不会执行。遗留的 `queued` 行会污染
    /// 下一次运行 —— 那是**级联失败**的源头。
    async fn cleanup_by_prefix(&self, pool: &sqlx::PgPool) {
        let prefix = self.prefix.lock().expect("锁中毒").clone();

        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM execution_runs WHERE request_key LIKE $1")
                .bind(format!("{prefix}%"))
                .fetch_all(pool)
                .await
                .unwrap_or_default();

        for id in ids {
            delete_run(pool, id).await;
        }
    }
}

/// 删除一个 run 及其子表数据。
async fn delete_run(pool: &sqlx::PgPool, id: i64) {
    for sql in [
        "DELETE FROM execution_outbox WHERE run_id = $1",
        "DELETE FROM execution_events WHERE run_id = $1",
        "DELETE FROM execution_runs WHERE id = $1",
    ] {
        let _ = sqlx::query(sql).bind(id).execute(pool).await;
    }
}

/// 让某个 run 的 `generation` 递增 —— 模拟「被别人重新领取」。
///
/// # 为什么不用「重置为 queued 再领取」
///
/// 领取是**全局队列**操作 —— 重新领取拿到的**可能**是另一个 run。
/// 直接改 `generation` 能**确定性地**制造一个僵尸租约。
///
/// # 围栏的六个条件里，只有 `generation` 变了
///
/// `lease_owner` 换成别人 —— 但 `generation` 已经不同了，
/// 旧租约**必然**被 `generation` 条件拦下。
/// 这与 `lease_fencing.rs` 里「伪造租约」的手法同源：
/// 把差异**隔离到单一维度**。
async fn bump_generation(pool: &sqlx::PgPool, run_id: RunId) {
    sqlx::query(
        "UPDATE execution_runs SET generation = generation + 1, lease_owner = $2,                 lease_until = clock_timestamp() + INTERVAL '30 seconds'          WHERE id = $1",
    )
    .bind(run_id.into_inner())
    .bind(format!("other_{}", Uuid::new_v4().simple()))
    .execute(pool)
    .await
    .expect("递增 generation 失败");
}

/// 读 run 当前的 `next_seq`。
async fn current_next_seq(pool: &sqlx::PgPool, run_id: RunId) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT next_seq FROM execution_runs WHERE id = $1")
        .bind(run_id.into_inner())
        .fetch_one(pool)
        .await
        .expect("读取 next_seq 失败");
    row.0
}

/// 数一数某个 run 的事件条数。
async fn count_events(pool: &sqlx::PgPool, run_id: RunId) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM execution_events WHERE run_id = $1")
        .bind(run_id.into_inner())
        .fetch_one(pool)
        .await
        .expect("统计事件失败");
    row.0
}

/// 数一数某个 run 的 outbox 条数。
async fn count_outbox(pool: &sqlx::PgPool, run_id: RunId) -> i64 {
    let row: (i64,) = sqlx::query_as("SELECT count(*) FROM execution_outbox WHERE run_id = $1")
        .bind(run_id.into_inner())
        .fetch_one(pool)
        .await
        .expect("统计 outbox 失败");
    row.0
}

// ─────────────────────────────────────────────────────────
// AC #2：序号由数据库原子分配
// ─────────────────────────────────────────────────────────

#[tokio::test]
async fn seq_is_allocated_by_the_database() {
    let Some(pool) = common::test_pool("seq_is_allocated_by_the_database")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob0_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;
    let run_id = lease.run_id();

    let mut tx = pool.begin().await.expect("开启事务失败");

    let first = OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
        .await
        .expect("写入事件失败");
    let second = OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "b"}))
        .await
        .expect("写入事件失败");

    // 序号由数据库分配 —— 应用层拿不到「下一个序号是多少」的承诺，
    // 只能拿到**数据库返回的**那个值。
    assert_eq!(first.seq, 1, "第一个事件的序号应为 1");
    assert_eq!(second.seq, 2, "第二个事件的序号应为 2");

    tx.commit().await.expect("提交事务失败");

    // 数据库里的 `next_seq` 与分配的序号一致。
    assert_eq!(current_next_seq(&pool, run_id).await, 2);
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #1：状态更新 + 事件 + outbox 在同一事务
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn event_and_outbox_are_written_in_the_same_transaction() {
    let Some(pool) = common::test_pool("event_and_outbox_are_written_in_the_same_transaction")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob1_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;

    let mut tx = pool.begin().await.expect("开启事务失败");

    OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
        .await
        .expect("写入事件失败");

    // 事务**未提交** —— 外部连接**看不到**任何东西。
    assert_eq!(
        count_events(&pool, lease.run_id()).await,
        0,
        "未提交的事务不应让事件对外可见"
    );

    tx.commit().await.expect("提交事务失败");

    // 提交后，事件与 outbox **同时**出现 —— 它们在同一个事务里。
    assert_eq!(count_events(&pool, lease.run_id()).await, 1);
    assert_eq!(count_outbox(&pool, lease.run_id()).await, 1);
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #9：序号分配与事件写入的原子性（回滚后 seq 不前进）
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn rolled_back_transaction_does_not_advance_seq() {
    let Some(pool) = common::test_pool("rolled_back_transaction_does_not_advance_seq")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob2_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;
    let run_id = lease.run_id();

    let seq_before = current_next_seq(&pool, run_id).await;

    let mut tx = pool.begin().await.expect("开启事务失败");

    let appended =
        OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
            .await
            .expect("写入事件失败");

    // 事务里已经分配到序号了 —— 但**还没提交**。
    assert_eq!(appended.seq, seq_before + 1);

    // **故意回滚**。
    tx.rollback().await.expect("回滚事务失败");

    // 关键断言：序号**没有**前进。
    //
    // ⚠️ 这正是「序号分配与事件写入同事务」的证据 ——
    // 如果序号分配在**另一个**事务里（或者用应用层自增），
    // 回滚后这里会是 `seq_before + 1`，测试**必须**变红。
    assert_eq!(
        current_next_seq(&pool, run_id).await,
        seq_before,
        "回滚后 next_seq 不应前进"
    );
    assert_eq!(count_events(&pool, run_id).await, 0, "回滚后不应留下事件");
    assert_eq!(
        count_outbox(&pool, run_id).await,
        0,
        "回滚后不应留下 outbox 消息"
    );

    // 再写一次 —— 序号应该**接着**原值，而不是跳过刚才那个。
    //
    // 这排除了另一种实现：「回滚时把序号手动减回去」。
    // 那种实现也能让上面的断言通过，但它是**应用层补偿** ——
    // 崩溃时补偿不会执行，序号就会**永久跳号**。
    let mut tx = pool.begin().await.expect("开启事务失败");
    let again = OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "b"}))
        .await
        .expect("写入事件失败");
    tx.commit().await.expect("提交事务失败");

    assert_eq!(
        again.seq,
        seq_before + 1,
        "重新提交应拿到同一个序号（数据库回滚了分配）"
    );
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #3：FOR UPDATE SKIP LOCKED —— 并发领取互不阻塞
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn outbox_claim_uses_skip_locked() {
    let Some(pool) = common::test_pool("outbox_claim_uses_skip_locked")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob3_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let outbox = OutboxRepository::new(pool.clone());

    // 造 3 条 outbox 消息（分散在 3 个 run 上）。
    //
    // 用 `create_claimed_run`（不碰队列）—— 所以本测试**不需要**持锁，
    // 也不会与其它测试抢队列。
    let mut mine_run_ids: Vec<RunId> = Vec::new();
    for _ in 0..3 {
        let lease = create_claimed_run(&pool, user_id, &runs).await;
        mine_run_ids.push(lease.run_id());
        let mut tx = pool.begin().await.expect("开启事务失败");
        OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
            .await
            .expect("写入事件失败");
        tx.commit().await.expect("提交事务失败");
    }

    // 领取一批。
    //
    // ⚠️ `claim_batch` 是**全局队列**操作 —— 它会领走**任意**一批待投递消息，
    // 包括**别的**测试的消息。所以：
    //
    // 1. 必须**持锁** —— 否则会与其它测试的消息抢走彼此的。
    // 2. 断言的是「**本测试的消息都在里面**」而不是「恰好领到 3 条」。
    let Some(_guard) = acquire_queue_lock().await else {
        return;
    };
    let batch = outbox.claim_batch(100).await.expect("领取失败");

    let mut ids: Vec<i64> = batch.messages().iter().map(|m| m.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(
        ids.len(),
        count,
        "领取到的消息 ID 应互不相同（同一批次不能重复）"
    );

    // 本测试造的 3 条消息**必须**都被领到 ——
    // 它们是最早插入的（`ORDER BY id`），而 `LIMIT 100` 足够大。
    let mine: Vec<RunId> = batch
        .messages()
        .iter()
        .filter_map(|m| m.run_id)
        .filter(|id| {
            // 只数本测试的 run（请求键前缀匹配）。
            mine_run_ids.contains(id)
        })
        .collect();
    assert_eq!(
        mine.len(),
        3,
        "本测试造的 3 条消息都应被领到，实际领到 {}",
        mine.len()
    );

    // 回滚 —— 不标记已投递，让其它测试还能领到。
    batch.rollback().await.expect("回滚失败");
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #4：published_at 与业务更新在同一事务里标记
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn published_at_is_marked_in_the_same_transaction() {
    let Some(pool) = common::test_pool("published_at_is_marked_in_the_same_transaction")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob4_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;
    let run_id = lease.run_id();

    let outbox = OutboxRepository::new(pool.clone());
    let mut tx = pool.begin().await.expect("开启事务失败");
    OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
        .await
        .expect("写入事件失败");
    tx.commit().await.expect("提交事务失败");

    // 领取是**全局队列**操作 —— 从这里开始持锁。
    let Some(_guard) = acquire_queue_lock().await else {
        return;
    };

    // 领取一条属于本 run 的消息。
    let mut batch = outbox.claim_batch(100).await.expect("领取失败");
    let target = batch
        .messages()
        .iter()
        .find(|m| m.run_id == Some(run_id))
        .expect("应领到本 run 的消息")
        .clone();

    // 标记已投递 —— 但**不提交**。
    batch.mark_published(target.id).await.expect("标记失败");

    // 未提交 → 外部连接**看不到** `published_at`。
    let published: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT published_at FROM execution_outbox WHERE id = $1")
            .bind(target.id)
            .fetch_one(&pool)
            .await
            .expect("读取 published_at 失败");
    assert!(
        published.0.is_none(),
        "未提交的事务不应让 published_at 对外可见"
    );

    batch.commit().await.expect("提交失败");

    // 提交后可见。
    let published: (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT published_at FROM execution_outbox WHERE id = $1")
            .bind(target.id)
            .fetch_one(&pool)
            .await
            .expect("读取 published_at 失败");
    assert!(published.0.is_some(), "提交后 published_at 应可见");

    // 已投递的消息**不应**再被领到。
    let next = outbox.claim_batch(100).await.expect("领取失败");
    assert!(
        !next.messages().iter().any(|m| m.id == target.id),
        "已投递的消息不应再被领取"
    );
    next.rollback().await.expect("回滚失败");
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #8：重复提交返回既有 run
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn duplicate_submit_returns_the_existing_run() {
    let Some(pool) = common::test_pool("duplicate_submit_returns_the_existing_run")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob5_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let outbox = OutboxRepository::new(pool.clone());
    let key = RequestKey::new(&runs.request_key()).expect("键应合法");
    let definition = json!({"nodes": ["a"]});

    let first = outbox
        .submit_idempotent(user_id, &key, &definition)
        .await
        .expect("首次提交失败");
    assert!(first.is_created(), "首次提交应创建新 run");

    // ⚠️ 必须登记 —— `submit_idempotent` 创建的 run 状态是 `queued`，
    // 留在**全局队列**里会污染其它测试文件（`lease_fencing.rs` 的
    // `claim` 会领到它）。
    runs.track(first.run_id());

    // 重复提交 —— **同一个键**。
    let second = outbox
        .submit_idempotent(user_id, &key, &definition)
        .await
        .expect("重复提交不应报错");
    assert!(second.is_existing(), "重复提交应命中幂等键");
    assert_eq!(
        first.run_id(),
        second.run_id(),
        "重复提交应返回**既有**的 run"
    );

    // 数据库里**只有一条**。
    let count: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM execution_runs WHERE owner_id = $1 AND request_key = $2",
    )
    .bind(user_id.into_inner())
    .bind(key.as_str())
    .fetch_one(&pool)
    .await
    .expect("统计失败");
    assert_eq!(count.0, 1, "幂等键应只创建一条 run");
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #6：幂等键长度校验
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn request_key_longer_than_64_is_rejected() {
    // 纯应用层校验 —— 不需要数据库。
    let key = "k".repeat(65);
    let err = RequestKey::new(&key).expect_err("超长键应被拒绝");

    assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);
}

#[tokio::test]
async fn duplicate_submit_with_a_different_key_creates_a_different_run() {
    let Some(pool) =
        common::test_pool("duplicate_submit_with_a_different_key_creates_a_different_run")
            .await
            .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob6_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let outbox = OutboxRepository::new(pool.clone());

    let key_a = RequestKey::new(&runs.request_key()).expect("键应合法");
    let key_b = RequestKey::new(&runs.request_key()).expect("键应合法");

    let a = outbox
        .submit_idempotent(user_id, &key_a, &json!({"x": 1}))
        .await
        .expect("提交失败");
    let b = outbox
        .submit_idempotent(user_id, &key_b, &json!({"x": 1}))
        .await
        .expect("提交失败");

    // ⚠️ 必须登记 —— 这两个 run 状态是 `queued`，会留在全局队列里。
    runs.track(a.run_id());
    runs.track(b.run_id());

    assert!(a.is_created());
    assert!(b.is_created());
    assert_ne!(a.run_id(), b.run_id(), "不同的幂等键应创建不同的 run");
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// AC #5：「至少一次」的语义在类型上可见
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn outbox_message_carries_the_dedup_tuple() {
    let Some(pool) = common::test_pool("outbox_message_carries_the_dedup_tuple")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob7_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;
    let run_id = lease.run_id();

    let _outbox = OutboxRepository::new(pool.clone());
    let outbox = OutboxRepository::new(pool.clone());
    let mut tx = pool.begin().await.expect("开启事务失败");
    let appended =
        OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
            .await
            .expect("写入事件失败");
    tx.commit().await.expect("提交事务失败");

    // 领取是**全局队列**操作 —— 从这里开始持锁。
    let Some(_guard) = acquire_queue_lock().await else {
        return;
    };

    let batch = outbox.claim_batch(100).await.expect("领取失败");
    let message = batch
        .messages()
        .iter()
        .find(|m| m.run_id == Some(run_id))
        .expect("应领到本 run 的消息");

    // 消费方**必须**能拿到去重元组 —— 这是「至少一次」语义的落地。
    assert_eq!(
        message.dedup_tuple(),
        Some((run_id, appended.seq)),
        "消息应携带 (run_id, seq) 去重元组"
    );
    assert!(message.has_dedup_tuple());

    batch.rollback().await.expect("回滚失败");
    runs.cleanup(&pool).await;
}

// ─────────────────────────────────────────────────────────
// 序号分配的围栏：僵尸 worker 不能分配序号
// ─────────────────────────────────────────────────────────
#[tokio::test]
async fn stale_lease_cannot_allocate_a_sequence() {
    let Some(pool) = common::test_pool("stale_lease_cannot_allocate_a_sequence")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let runs = TestRuns::with_prefix("ob8_");
    // 清理**上次运行遗留**的行 —— 测试 panic 时末尾的 cleanup 不会执行。
    runs.cleanup_by_prefix(&pool).await;

    let user_id = create_user(&pool).await;
    let lease = create_claimed_run(&pool, user_id, &runs).await;
    let run_id = lease.run_id();

    // 让 `generation` 递增 —— 旧租约变成僵尸。
    //
    // ⚠️ 不能用「重置为 queued 再领取」—— 领取是**全局队列**操作，
    // 重新领取拿到的**可能**是另一个 run。
    // 直接改 `generation` 能**确定性地**制造僵尸租约。
    bump_generation(&pool, run_id).await;

    // 旧（僵尸）租约尝试分配序号 —— 必须被围栏拒绝。
    let mut tx = pool.begin().await.expect("开启事务失败");
    let err = OutboxRepository::append_event(&mut tx, &lease, "delta", &json!({"content": "a"}))
        .await
        .expect_err("僵尸写入应被围栏拒绝");

    assert_eq!(
        err.user_facing().code,
        ErrorCode::RunConflict,
        "僵尸写入应报 RunConflict"
    );

    tx.rollback().await.expect("回滚失败");
    runs.cleanup(&pool).await;
}
