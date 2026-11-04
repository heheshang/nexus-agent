//! 租约与 **generation fencing** 的集成测试 —— 连**真实 PostgreSQL**。
//!
//! # 覆盖的行为层不变量
//!
//! | 测试 | 断言的不变量 |
//! |---|---|
//! | `three_concurrent_claimers_get_different_runs` | `SKIP LOCKED` 让并发领取**互不阻塞**且各得不同 run（AC #5） |
//! | `stale_generation_write_is_rejected` | **僵尸写入影响 0 行**（AC #6） |
//! | `current_holder_write_is_applied` | 当前持有者写入**影响 1 行**（AC #7） |
//! | `lease_holds_typed_run_id` | 租约用 `RunId` 而非裸 `i64`（AC #8） |
//! | `lease_expiry_is_judged_by_clock_timestamp` | 过期判定用 `clock_timestamp()`（AC #9） |
//! | `reclaim_after_expiry_bumps_generation` | 重新领取让 `generation` 递增 |
//! | `expired_lease_blocks_writes_even_with_matching_generation` | 租约过期是 `generation` 之外的第二道防线 |
//!
//! # ⚠️ 为什么这些测试**必须串行**
//!
//! `claim` 是**全局队列**语义 —— 它领取队列里**任意**一个可运行的 run。
//! 而 `cargo test` 默认**并行**执行同一文件内的测试，于是：
//!
//! - 测试 A 造了一个 run
//! - 测试 B 的 `claim` 抢走了 A 的 run
//! - A 断言「领到的就是我造的那个」→ **失败**
//!
//! 这不是实现的问题，是**隔离维度**的问题：
//! `claim` 的选择键是 `(status, available_at, id)` —— **全局**的，
//! 没有「按 request_key 领取」这种入口（那不是生产语义）。
//!
//! 所以唯一正确的隔离手段是**串行化** —— 用一个进程内的互斥锁
//! 把「领取 → 断言」的整段临界区包起来。
//!
//! 这与 NEX-11 的 `status` 隔离问题是**同一类**：
//! 隔离维度必须落在「被操作的那个键」上。
//! 队列操作的键是**全局队列**，那就只能串行。
//!
//! # 数据隔离
//!
//! 每个测试用**唯一的 `request_key`**（后缀是随机 UUID）——
//! `execution_runs` 上有 `(owner_id, request_key)` 唯一索引，
//! 随机 key 保证并行执行的测试互不冲突。

mod common;

use std::time::Duration;

use nexus_core::error::ErrorCode;
use nexus_core::id::{RunId, UserId};
use nexus_model::NewUser;
use nexus_repo::{
    CounterColumn, ExecutionLeaseRepository, LeaseSettings, RunLease, UserRepository,
};
use tokio::sync::Mutex;
use uuid::Uuid;

/// **全局**领取互斥锁。
///
/// 见模块文档 —— 领取是全局队列操作，测试必须串行。
static CLAIM_LOCK: Mutex<()> = Mutex::const_new(());

/// 造一个唯一的 `request_key`。
fn unique_request_key() -> String {
    format!("test_{}", Uuid::new_v4().simple())
}

/// 造一个唯一的持有者标识。
fn unique_owner() -> String {
    format!("worker_{}", Uuid::new_v4().simple())
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

/// 造一个可领取的 run。
async fn create_queued_run(pool: &sqlx::PgPool, owner_id: UserId) -> RunId {
    let repo = ExecutionLeaseRepository::new(pool.clone());
    repo.create_queued_run(owner_id, &unique_request_key(), serde_json::json!({"a": 1}))
        .await
        .expect("创建 run 失败")
}

/// 把 run 重置为「可领取」—— 模拟 worker 崩溃后租约被回收。
async fn reset_to_queued(pool: &sqlx::PgPool, run_id: RunId) {
    sqlx::query(
        "UPDATE execution_runs SET status = 'queued', lease_owner = '', lease_until = NULL \
         WHERE id = $1",
    )
    .bind(run_id.into_inner())
    .execute(pool)
    .await
    .expect("重置 run 状态失败");
}

/// 把 run 的 `lease_owner` 与 `lease_until` **改回**旧持有者的值，
/// 但**保留**新的 `generation`。
///
/// # 为什么需要这个「伪造」步骤
///
/// 围栏条件有**六个**子句。僵尸写入被拒，可能是**任意一个**子句拦下的 ——
/// 那样测试就测不出「`generation` 是否真的在起作用」。
///
/// 典型的陷阱：worker-b 重新领取后，`lease_owner` 也变了、
/// `lease_until` 也刷新了 —— 于是即使把 `generation` 条件**整个删掉**，
/// 僵尸写入**照样**被 `lease_owner` 拦下。测试照样绿。
///
/// 这就是**变异测试**教给我们的东西：
/// 「测试通过」不等于「测试在测你以为的东西」。
///
/// 所以本函数把 `lease_owner` / `lease_until` 恢复成旧持有者的样子，
/// 让**唯一**的差异是 `generation` —— 这时删掉 `generation` 条件，
/// 测试**必须**变红。
///
/// # 为什么这样是安全的
///
/// `lease_until` 被设成**将来**（+30 秒），`status` 仍是 `'running'`，
/// `deadline_at` 仍在将来 —— 所以除了 `generation` 之外，
/// 围栏的其它条件**全部满足**。差异被隔离到单一维度。
async fn forge_lease_back_to(pool: &sqlx::PgPool, run_id: RunId, owner: &str) {
    sqlx::query(
        "UPDATE execution_runs SET lease_owner = $2, \
                lease_until = clock_timestamp() + INTERVAL '30 seconds' \
         WHERE id = $1",
    )
    .bind(run_id.into_inner())
    .bind(owner)
    .execute(pool)
    .await
    .expect("伪造租约归属失败");
}

// ─────────────────────────────────────────────────────────
// AC #5：FOR UPDATE SKIP LOCKED —— 并发领取互不阻塞
// ─────────────────────────────────────────────────────────

#[tokio::test]
async fn three_concurrent_claimers_get_different_runs() {
    let Some(pool) = common::test_pool("three_concurrent_claimers_get_different_runs")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;

    // 造 3 个可领取的 run。
    let mut created = Vec::new();
    for _ in 0..3 {
        created.push(create_queued_run(&pool, user_id).await);
    }

    // 3 个**并发**的领取者，各自用**独立的连接**。
    //
    // ⚠️ 必须用独立连接 —— 同一个 `PgPool` 里的并发查询可能复用同一个
    // 连接，那样就不是真正的并发领取了。
    let mut handles = Vec::new();
    for i in 0..3 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            let repo = ExecutionLeaseRepository::new(pool);
            repo.claim(&format!("worker-{i}")).await
        }));
    }

    let mut claimed = Vec::new();
    for handle in handles {
        let lease = handle
            .await
            .expect("领取任务不应 panic")
            .expect("领取不应失败")
            .expect("应能领到一个 run");
        claimed.push(lease);
    }

    // 每个 worker 都领到了**不同**的 run。
    let ids: Vec<RunId> = claimed.iter().map(RunLease::run_id).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        3,
        "3 个并发领取者应各领到不同的 run，实际：{ids:?}"
    );

    // 而且领到的正是我们造的那 3 个。
    for id in &ids {
        assert!(
            created.contains(id),
            "领到的 run {id} 不在预期集合内：{created:?}"
        );
    }
}

#[tokio::test]
async fn claim_returns_none_when_queue_is_empty() {
    let Some(pool) = common::test_pool("claim_returns_none_when_queue_is_empty")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    // 把所有行的 `available_at` 推到未来 —— 让任何行都**不可领取**。
    //
    // ⚠️ 这是**全局**修改，但在领取互斥锁的保护下没有别的测试在跑。
    // 结束后**还原**，免得把污染留给后续的测试运行。
    sqlx::query("UPDATE execution_runs SET available_at = clock_timestamp() + INTERVAL '1 hour'")
        .execute(&pool)
        .await
        .expect("推迟可领取时间失败");

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let lease = repo.claim(&unique_owner()).await.expect("领取不应失败");

    // 还原。
    sqlx::query("UPDATE execution_runs SET available_at = clock_timestamp()")
        .execute(&pool)
        .await
        .expect("还原可领取时间失败");

    assert!(
        lease.is_none(),
        "没有可领取的 run 时应返回 None，而不是错误"
    );
}

// ─────────────────────────────────────────────────────────
// AC #6 / #7：fencing 拒绝僵尸写入，放行当前持有者
// ─────────────────────────────────────────────────────────

#[tokio::test]
async fn stale_generation_write_is_rejected() {
    let Some(pool) = common::test_pool("stale_generation_write_is_rejected")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());

    // worker-a 领取。
    let owner_a = unique_owner();
    let lease_a = repo
        .claim(&owner_a)
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");
    assert_eq!(lease_a.run_id(), run_id, "应领到自己造的那个 run");

    // 模拟 worker-a 崩溃：把 run 重新置为可领取。
    reset_to_queued(&pool, run_id).await;

    // worker-b 重新领取 → `generation` 递增。
    let lease_b = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    assert_eq!(lease_b.run_id(), run_id, "应重新领到同一个 run");

    // ⚠️ **关键步骤**：把 `lease_owner` 与 `lease_until` 改回 worker-a 的样子，
    // 只保留 worker-b 的新 `generation`。
    //
    // 这样一来，worker-a 的租约与数据库行之间**唯一的差异**就是 `generation`。
    // 没有这一步，测试会被 `lease_owner` 拦下 ——
    // 删掉 `generation` 条件它照样绿，测不出东西（变异测试的教训）。
    forge_lease_back_to(&pool, run_id, &owner_a).await;

    // worker-a 用**过期**的 generation 写入 → **影响 0 行**。
    let mut tx = pool.begin().await.expect("开启事务失败");
    let outcome = ExecutionLeaseRepository::write_status_fenced(&mut tx, &lease_a, "done")
        .await
        .expect("围栏写入不应失败");
    tx.rollback().await.expect("回滚失败");

    assert!(
        outcome.is_fenced(),
        "过期 generation 的写入必须被 fencing 拒绝（影响 0 行）"
    );

    // 状态**没被改** —— 这才是「拒绝」的真实含义。
    let status = repo
        .find_status(run_id)
        .await
        .expect("查询状态失败")
        .expect("run 应存在");
    assert_eq!(status.0, "running", "僵尸写入不应改动状态");
}

#[tokio::test]
async fn current_holder_write_is_applied() {
    let Some(pool) = common::test_pool("current_holder_write_is_applied")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    assert_eq!(lease.run_id(), run_id);

    // 当前持有者写入 → **影响 1 行**。
    let mut tx = pool.begin().await.expect("开启事务失败");
    let outcome = ExecutionLeaseRepository::write_status_fenced(&mut tx, &lease, "done")
        .await
        .expect("围栏写入不应失败");
    tx.commit().await.expect("提交失败");

    assert!(
        outcome.is_applied(),
        "当前持有者的写入应被放行（影响 1 行）"
    );

    let status = repo
        .find_status(run_id)
        .await
        .expect("查询状态失败")
        .expect("run 应存在");
    assert_eq!(status.0, "done");
}

#[tokio::test]
async fn reclaim_after_expiry_bumps_generation() {
    let Some(pool) = common::test_pool("reclaim_after_expiry_bumps_generation")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());

    let lease_a = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");
    let gen_a = repo
        .find_lease_info(run_id)
        .await
        .expect("查询租约信息失败")
        .expect("run 应存在")
        .generation;

    // 模拟崩溃 → 重新领取。
    reset_to_queued(&pool, run_id).await;

    let lease_b = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    let gen_b = repo
        .find_lease_info(run_id)
        .await
        .expect("查询租约信息失败")
        .expect("run 应存在")
        .generation;

    // `generation` 必须**严格递增** —— 这是 fencing 的全部依据。
    assert!(
        gen_b > gen_a,
        "重新领取后 generation 必须递增：{gen_a} → {gen_b}"
    );

    // 两个租约指向同一个 run，但只有后者有效。
    assert_eq!(lease_a.run_id(), lease_b.run_id());
}

#[tokio::test]
async fn expired_lease_blocks_writes_even_with_matching_generation() {
    let Some(pool) = common::test_pool("expired_lease_blocks_writes_even_with_matching_generation")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    // 把租约改成**已过期**，但**不动** generation ——
    // 于是 generation 仍然匹配，只有 `lease_until` 这道防线生效。
    sqlx::query(
        "UPDATE execution_runs SET lease_until = clock_timestamp() - INTERVAL '1 second' \
         WHERE id = $1",
    )
    .bind(run_id.into_inner())
    .execute(&pool)
    .await
    .expect("设置过期时间失败");

    let mut tx = pool.begin().await.expect("开启事务失败");
    let outcome = ExecutionLeaseRepository::write_status_fenced(&mut tx, &lease, "done")
        .await
        .expect("围栏写入不应失败");
    tx.rollback().await.expect("回滚失败");

    // `generation` 对得上，但租约过期了 → 仍然被拒。
    //
    // 这是 `generation` 之外的**第二道防线**（AC #9 的语义）。
    assert!(
        outcome.is_fenced(),
        "租约过期后即使 generation 匹配也必须被拒绝"
    );
}

#[tokio::test]
async fn fenced_counter_increment_is_rejected() {
    let Some(pool) = common::test_pool("fenced_counter_increment_is_rejected")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let owner_a = unique_owner();
    let lease = repo
        .claim(&owner_a)
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    // 模拟崩溃 → 重新领取（generation 递增）。
    reset_to_queued(&pool, run_id).await;
    repo.claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    // 把 owner / lease_until 改回旧持有者 —— 只留 `generation` 一个差异。
    forge_lease_back_to(&pool, run_id, &owner_a).await;

    // 旧租约的计数器自增 → 被拒绝。
    let mut tx = pool.begin().await.expect("开启事务失败");
    let outcome = ExecutionLeaseRepository::increment_counter_fenced(
        &mut tx,
        &lease,
        CounterColumn::NextSeq,
        1,
    )
    .await
    .expect("围栏自增不应失败");
    tx.rollback().await.expect("回滚失败");

    assert!(outcome.is_fenced(), "过期租约的计数器自增必须被拒绝");
}

// ─────────────────────────────────────────────────────────
// AC #8 / #9：类型与时间语义
// ─────────────────────────────────────────────────────────

#[tokio::test]
async fn lease_holds_typed_run_id() {
    let Some(pool) = common::test_pool("lease_holds_typed_run_id")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    // AC #8：`RunLease` 的 run ID 是 `nexus_core::id::RunId`。
    //
    // 这条断言的**类型层**价值：把 `lease.run_id()` 赋给 `RunId` 变量
    // 能编译通过，说明它**就是** `RunId` 而不是裸 `i64`。
    let typed: RunId = lease.run_id();
    assert_eq!(typed, run_id);
}

#[tokio::test]
async fn lease_expiry_is_judged_by_clock_timestamp() {
    let Some(pool) = common::test_pool("lease_expiry_is_judged_by_clock_timestamp")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    create_queued_run(&pool, user_id).await;

    let repo = ExecutionLeaseRepository::new(pool.clone());
    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    let info = repo
        .find_lease_info(lease.run_id())
        .await
        .expect("查询租约信息失败")
        .expect("run 应存在");

    // 领取后 `lease_until` 必须是**将来**的时刻 ——
    // 如果实现里用了 `now()`（事务开始时间），在长事务里这个值会偏早。
    //
    // ⚠️ 比较基准用**数据库**的 `clock_timestamp()`，不是应用侧的 `Utc::now()` ——
    // 两台机器的时钟有偏差，用应用侧时钟做断言会引入假失败。
    let db_now: (chrono::DateTime<chrono::Utc>,) = sqlx::query_as("SELECT clock_timestamp()")
        .fetch_one(&pool)
        .await
        .expect("查询数据库时钟失败");

    let lease_until = info.lease_until.expect("领取后应有租约到期时间");
    assert!(
        lease_until > db_now.0,
        "租约到期时间应在将来：{lease_until} vs {}",
        db_now.0
    );

    // 且到期时间 ≈ TTL 之内（留 1 秒的宽限给时钟偏差与执行耗时）。
    let ttl = chrono::Duration::from_std(LeaseSettings::with_ttl(Duration::from_secs(30)).ttl())
        .expect("TTL 应能转成 chrono 差值");
    let delta = lease_until - db_now.0;
    assert!(
        delta <= ttl + chrono::Duration::seconds(1),
        "租约到期时间不应超过 TTL：{delta:?} > {ttl:?}"
    );
}

/// AC #9 —— **长事务**里的过期判定。
///
/// # 为什么这条测试不可省略
///
/// `now()` 与 `clock_timestamp()` 的差别只在**长事务**里显现：
///
/// | 函数 | 返回 | 在事务里 |
/// |---|---|---|
/// | `now()` | 事务**开始**时间 | 固定值 —— 事务跑多久都不变 |
/// | `clock_timestamp()` | **实际**当前时间 | 每调用一次前进 |
///
/// 在一个跑了 2 秒的事务里判断「租约是否过期」：
///
/// - `now()` → 事务开始时的时刻 → 租约看起来**还没过期** → **错误地放行**
/// - `clock_timestamp()` → 真实的当前时刻 → 租约**已过期** → 正确拒绝
///
/// # 本测试的构造
///
/// 1. 用 **1 秒**的 TTL 领取租约。
/// 2. **开启事务**（此刻 `now()` 被固定）。
/// 3. `sleep` 到租约真的过期之后。
/// 4. 在**同一个事务**里做围栏写入。
///
/// 若实现用 `now()`，第 4 步会**被放行** → 测试失败。
#[tokio::test]
async fn expiry_is_judged_inside_a_long_transaction() {
    let Some(pool) = common::test_pool("expiry_is_judged_inside_a_long_transaction")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    let run_id = create_queued_run(&pool, user_id).await;

    // 1 秒 TTL —— 很快就会过期，但足够领取成功。
    let repo = ExecutionLeaseRepository::with_settings(
        pool.clone(),
        LeaseSettings::with_ttl(Duration::from_secs(1)),
    );
    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");
    assert_eq!(lease.run_id(), run_id);

    // ── 开启事务 —— `now()` 在此刻被固定 ──
    let mut tx = pool.begin().await.expect("开启事务失败");

    // 让租约真的过期。
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    // 事务里确认租约**确实**已经过期（用 `clock_timestamp()` 判断）。
    let expired: (bool,) =
        sqlx::query_as("SELECT lease_until <= clock_timestamp() FROM execution_runs WHERE id = $1")
            .bind(run_id.into_inner())
            .fetch_one(&mut *tx)
            .await
            .expect("查询租约状态失败");
    assert!(expired.0, "租约此刻应已过期");

    // ── 在**同一个事务**里做围栏写入 ──
    let outcome = ExecutionLeaseRepository::write_status_fenced(&mut tx, &lease, "done")
        .await
        .expect("围栏写入不应失败");
    tx.rollback().await.expect("回滚失败");

    // 用 `now()` 的实现会**放行**（错误）；用 `clock_timestamp()` 的实现正确拒绝。
    assert!(
        outcome.is_fenced(),
        "长事务里租约过期必须被判定为过期（不能用 now()）"
    );
}

#[tokio::test]
async fn lease_ttl_comes_from_configuration() {
    let Some(pool) = common::test_pool("lease_ttl_comes_from_configuration")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let _guard = CLAIM_LOCK.lock().await;

    let user_id = create_user(&pool).await;
    create_queued_run(&pool, user_id).await;

    // 用一个**很短的** TTL —— 领取后租约立刻过期，
    // 于是「过期租约被拒」这条断言不需要 sleep。
    let repo = ExecutionLeaseRepository::with_settings(
        pool.clone(),
        LeaseSettings::with_ttl(Duration::from_millis(1)),
    );

    let lease = repo
        .claim(&unique_owner())
        .await
        .expect("领取不应失败")
        .expect("应能领到一个 run");

    // 极短 TTL 下，租约几乎立刻过期。
    tokio::time::sleep(Duration::from_millis(20)).await;

    let mut tx = pool.begin().await.expect("开启事务失败");
    let outcome = ExecutionLeaseRepository::write_status_fenced(&mut tx, &lease, "done")
        .await
        .expect("围栏写入不应失败");
    tx.rollback().await.expect("回滚失败");

    assert!(outcome.is_fenced(), "TTL 到期后写入必须被拒绝");
}

#[tokio::test]
async fn claim_does_not_leak_internal_errors() {
    // 指向一个**不存在的**数据库 —— 领取必然失败。
    let bad_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect("postgres://nexus:nexus@127.0.0.1:1/nonexistent")
        .await;

    if bad_pool.is_err() {
        // 连不上 —— 跳过（本机可能没有那个端口在监听，但连接失败太快）。
        return;
    }

    let repo = ExecutionLeaseRepository::new(bad_pool.expect("已连接"));
    let err = repo
        .claim(&unique_owner())
        .await
        .expect_err("连不上数据库应当报错");

    // 数据库不可用 → `StorageUnavailable`（503，可重试）。
    assert_eq!(err.user_facing().code, ErrorCode::StorageUnavailable);

    // 对外消息**不能**泄露 SQL 文本或内部结构。
    let message = err.user_facing().message.to_string();
    assert!(!message.contains("SELECT"), "对外消息泄露了 SQL：{message}");
    assert!(
        !message.contains("execution_runs"),
        "对外消息泄露了表名：{message}"
    );
}

/// 领取互斥锁的**使用次数** —— 确保锁真的被用上了。
///
/// 这条断言是为了防止「有人删掉了 `CLAIM_LOCK.lock()`，
/// 测试又恰好通过」的退化。
#[test]
fn claim_lock_is_actually_used() {
    // 用源码扫描确认 —— 比运行时计数更直接。
    let src = include_str!("lease_fencing.rs");
    let count = src.matches("CLAIM_LOCK.lock().await").count();

    assert!(
        count >= 8,
        "领取类测试必须串行（应至少 8 处持有 CLAIM_LOCK），实际 {count} 处"
    );
}
