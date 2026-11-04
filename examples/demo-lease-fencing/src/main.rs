//! demo-lease-fencing
//!
//! 验证设计文档 §2.5.4 的**持久执行租约模型**，以及 §1.3.1 的
//! 「PostgreSQL 是唯一权威，Redis 只做唤醒提示」约定。
//!
//! 两条核心机制：
//!
//! 1. **领取**：`FOR UPDATE SKIP LOCKED` —— 多个 worker 并发领取时
//!    互不阻塞，且各自拿到**不同**的 run。
//! 2. **fencing**：`WHERE generation = $my_gen` —— worker 崩溃后被别人
//!    重新领取，旧 worker 的后续写入**影响 0 行**（被拒绝）。
//!
//! ## 需要数据库
//!
//! ```bash
//! DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_design \
//!   cargo run -p demo-lease-fencing
//! ```
//!
//! 库需要先应用参考实现的 13 组迁移（43 张表）。

use sqlx::postgres::PgPoolOptions;
use sqlx::{Acquire, FromRow};

/// 数据库 URL。
fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://nexus:nexus@127.0.0.1:5432/nexus_design".to_string())
}

/// 运行记录。
#[derive(Debug, FromRow)]
struct RunRow {
    id: i64,
    /// 当前状态（queued / running / done …）
    #[allow(dead_code)]
    status: String,
    generation: i64,
    /// 持有租约的 worker
    #[allow(dead_code)]
    lease_owner: String,
}

/// 租约句柄。
///
/// **generation 封装在里面** —— 调用方拿不到裸数字，
/// 因此写不出「忘记带 generation」的 UPDATE。
#[derive(Debug, Clone)]
struct RunLease {
    run_id: i64,
    generation: i64,
    /// 持有者 —— 便于日志与排障
    #[allow(dead_code)]
    owner: String,
}

impl RunLease {
    /// 用当前租约写状态。
    ///
    /// 返回 `false` 表示**被 fencing 拒绝** —— 说明本 worker 已是僵尸。
    async fn write_status(&self, pool: &sqlx::PgPool, status: &str) -> Result<bool, sqlx::Error> {
        let r = sqlx::query(
            "UPDATE workflow_runs SET status = $1
             WHERE id = $2 AND generation = $3",
        )
        .bind(status)
        .bind(self.run_id)
        .bind(self.generation)
        .execute(pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }
}

/// 领取一个可运行的 run。
async fn claim(pool: &sqlx::PgPool, owner: &str) -> Result<Option<RunLease>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows: Vec<RunRow> = sqlx::query_as(
        "SELECT id, status, generation, lease_owner FROM workflow_runs
         WHERE status = 'queued' AND definition IS NOT NULL
         ORDER BY available_at, id
         FOR UPDATE SKIP LOCKED
         LIMIT 1",
    )
    .fetch_all(&mut *tx)
    .await?;

    let Some(row) = rows.first() else {
        tx.rollback().await?;
        return Ok(None);
    };

    // 领取：写 owner、延长租约、generation 自增
    sqlx::query(
        "UPDATE workflow_runs
         SET status = 'running',
             lease_owner = $1,
             lease_until = NOW() + INTERVAL '30 seconds',
             generation = generation + 1
         WHERE id = $2",
    )
    .bind(owner)
    .bind(row.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Some(RunLease {
        run_id: row.id,
        generation: row.generation + 1,
        owner: owner.to_string(),
    }))
}

/// 准备 N 个可领取的 run。
///
/// **自己 INSERT**，不依赖库里有现成数据 ——
/// 迁移只建表不灌 `workflow_runs`（真实数据由业务写入）。
///
/// 注意外键链：`workflow_runs.workflow_id → workflows.id`，
/// 而 `workflows.creator_id → users.id`。所以要先保证有可用的 user 与 workflow。
async fn seed(pool: &sqlx::PgPool, n: i64) -> Result<Vec<i64>, sqlx::Error> {
    // 1. 确保有一个 user（workflows.creator_id 的外键目标）
    let user_id = match sqlx::query_as::<_, (i64,)>("SELECT id FROM users LIMIT 1")
        .fetch_one(pool)
        .await
    {
        Ok((id,)) => id,
        Err(_) => {
            sqlx::query_as::<_, (i64,)>(
                "INSERT INTO users (username, email, password_hash, status)
                 VALUES ('demo-lease-fencing', 'demo@local', 'x', 1) RETURNING id",
            )
            .fetch_one(pool)
            .await?
            .0
        }
    };

    // 2. 确保有一个 workflow
    let workflow_id = match sqlx::query_as::<_, (i64,)>("SELECT id FROM workflows LIMIT 1")
        .fetch_one(pool)
        .await
    {
        Ok((id,)) => id,
        Err(_) => {
            sqlx::query_as::<_, (i64,)>(
                "INSERT INTO workflows (name, dsl, creator_id, is_active)
                 VALUES ('demo-lease-fencing', '{}', $1, true) RETURNING id",
            )
            .bind(user_id)
            .fetch_one(pool)
            .await?
            .0
        }
    };

    // 3. 插入可领取的 run
    let mut ids = Vec::new();
    for _ in 0..n {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO workflow_runs
                (workflow_id, trigger_type, status, input, definition,
                 generation, lease_owner, available_at, deadline_at)
             VALUES ($1, 'manual', 'queued', '{}'::jsonb, '{\"a\":1}'::jsonb,
                     0, '', NOW(), NOW() + INTERVAL '1 day')
             RETURNING id",
        )
        .bind(workflow_id)
        .fetch_one(pool)
        .await?;
        ids.push(id);
    }
    Ok(ids)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = match PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&database_url())
        .await
    {
        Ok(p) => p,
        Err(e) => {
            // 数据库不可用时**明确退出**，而不是挂住 —— CI 无库环境要能跳过。
            println!("✗ 无法连接数据库：{e}");
            println!("  需要：DATABASE_URL 指向已应用参考实现 13 组迁移的 PostgreSQL");
            return Ok(());
        }
    };
    println!("✓ 连接成功");

    let ids = seed(&pool, 3).await?;
    if ids.is_empty() {
        println!("✗ 没能准备可领取的 run");
        return Ok(());
    }
    println!("✓ 准备了 {} 个可领取的 run: {ids:?}\n", ids.len());

    // ── 1. SKIP LOCKED：并发领取互不阻塞 ──
    println!("--- 1. FOR UPDATE SKIP LOCKED ---");
    let mut c1 = pool.acquire().await?;
    let mut c2 = pool.acquire().await?;
    let mut tx1 = c1.begin().await?;
    let mut tx2 = c2.begin().await?;

    let sql = "SELECT id FROM workflow_runs
               WHERE status='queued' AND definition IS NOT NULL
               ORDER BY available_at, id FOR UPDATE SKIP LOCKED LIMIT 1";

    let r1: Vec<(i64,)> = sqlx::query_as(sql).fetch_all(&mut *tx1).await?;
    let r2: Vec<(i64,)> = sqlx::query_as(sql).fetch_all(&mut *tx2).await?;

    println!("  连接1 领到 {:?}", r1.first().map(|x| x.0));
    println!("  连接2 领到 {:?}", r2.first().map(|x| x.0));
    assert_ne!(
        r1.first().map(|x| x.0),
        r2.first().map(|x| x.0),
        "两个连接应领到不同的 run"
    );
    println!("  ✓ 互不阻塞，且各自拿到不同的 run");

    // 第三个连接也能领到第三个
    let mut c3 = pool.acquire().await?;
    let mut tx3 = c3.begin().await?;
    let r3: Vec<(i64,)> = sqlx::query_as(sql).fetch_all(&mut *tx3).await?;
    println!("  连接3 领到 {:?}\n", r3.first().map(|x| x.0));

    tx1.rollback().await?;
    tx2.rollback().await?;
    tx3.rollback().await?;

    // ── 2. fencing：拒绝僵尸写入 ──
    println!("--- 2. generation fencing ---");
    let lease = claim(&pool, "worker-a").await?.expect("应能领到一个 run");
    println!(
        "  worker-a 领到 run_id={} generation={}",
        lease.run_id, lease.generation
    );

    // worker-a 崩溃，worker-b 重新领取 → generation 递增
    let lease_b = claim(&pool, "worker-b").await?.expect("应能领到第二个 run");
    // 模拟 worker-a 的 run 被重新领取：手动把 worker-a 的 run 也置为可领
    sqlx::query(
        "UPDATE workflow_runs SET status='queued', lease_owner='', lease_until=NULL
         WHERE id = $1",
    )
    .bind(lease.run_id)
    .execute(&pool)
    .await?;
    let lease_a2 = claim(&pool, "worker-c").await?;
    let stale_generation = lease.generation;
    println!("  模拟 worker-a 崩溃后被重新领取，旧 generation = {stale_generation}");

    // 旧 worker 用**过期** generation 写入 —— 应被拒绝
    let stale_lease = RunLease {
        run_id: lease.run_id,
        generation: stale_generation,
        owner: "worker-a".into(),
    };
    let ok = stale_lease.write_status(&pool, "done").await?;
    println!(
        "  过期 generation 写入 → 影响行数 = {}（应为 0 = 被拒绝）",
        ok as u8
    );
    assert!(!ok, "过期 generation 应被 fencing 拒绝");

    // 当前 generation 写入 —— 应放行
    let fresh_lease = match lease_a2 {
        Some(l) if l.run_id == lease.run_id => l,
        _ => {
            // 若重新领取到了别的 run，直接构造正确的 lease
            let row: Vec<RunRow> = sqlx::query_as(
                "SELECT id, status, generation, lease_owner FROM workflow_runs WHERE id = $1",
            )
            .bind(lease.run_id)
            .fetch_all(&pool)
            .await?;
            RunLease {
                run_id: lease.run_id,
                generation: row[0].generation,
                owner: "worker-c".into(),
            }
        }
    };
    let ok = fresh_lease.write_status(&pool, "done").await?;
    println!(
        "  当前 generation 写入 → 影响行数 = {}（应为 1 = 放行）",
        ok as u8
    );
    assert!(ok, "当前 generation 应放行");
    let _ = lease_b;

    println!("\n✓ 僵尸写入被 fencing 拒绝");
    Ok(())
}
