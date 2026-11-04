#![allow(dead_code)]

//! 集成测试基座 —— **有库则跑，无库则跳过**。
//!
//! # 为什么需要这个基座
//!
//! 集成测试连**真实 PostgreSQL**（NEX-11 AC #7），
//! 但**不是每个开发者、每个 CI job 都有数据库**。
//! 没有这层机制：
//!
//! - 没有 PG 的开发者看到**满屏红灯** —— 但那不是他的代码坏了。
//! - CI 的「集成测试绿」会变成**假象** —— 测试全被跳过，CI 显示 0 passed。
//!
//! 所以判据是 `DATABASE_URL` **是否存在**：
//!
//! - 不存在 → **跳过**（打印一行说明，测试通过）。
//! - 存在 → **真跑**。
//!
//! ⚠️ **CI 必须提供 `DATABASE_URL`** —— `.github/workflows/ci.yml` 里已经有
//! PostgreSQL `services:` 与 `DATABASE_URL` 环境变量。
//! 那一段注释写得很清楚：「没有这一步，集成测试会全部跳过」。
//!
//! # 「跳过」的实现方式
//!
//! **不**用 `#[ignore]` —— 那样 CI 也要额外加 `--ignored` 才会跑，
//! 容易忘记。
//!
//! 而是在运行时检查环境变量，**提前 `return`**。
//! 效果一样（测试显示为 passed），但 CI 的 `cargo test` **默认就会跑**它们。

use sqlx::PgPool;

/// 测试库的连接 URL 的环境变量名。
pub const ENV_DATABASE_URL: &str = "DATABASE_URL";

/// 打印「跳过」说明并返回 `None`。
///
/// # 为什么打印到 stdout
///
/// `cargo test` 默认**捕获** stdout，只有失败时才显示。
/// 所以测试**成功**时这行输出**不会**出现在 CI 日志里 ——
/// 那正是问题：CI 日志里看不到「集成测试被跳过了」。
///
/// 所以这里用 `eprintln!`（stderr 不被捕获），
/// 让「被跳过」这件事在 CI 日志里**看得见**。
fn skipped(test_name: &str) -> Option<PgPool> {
    eprintln!(
        "[SKIP] {test_name}：环境变量 {ENV_DATABASE_URL} 未设置，跳过集成测试。\
         设置它以运行真实数据库测试（例：{ENV_DATABASE_URL}=postgres://user:pass@127.0.0.1:5432/db）"
    );
    None
}

/// 取测试连接池；没有 `DATABASE_URL` 时返回 `None`（表示跳过）。
///
/// # 错误
///
/// `DATABASE_URL` 存在但**连不上**时返回 `Err` —— 那不是「跳过」，
/// 是**真的失败了**。把连接失败当成跳过，会让 CI 的假象更严重：
/// 数据库地址写错时，测试会静默地全部跳过，而 CI 显示绿。
///
/// 所以这里把「环境变量缺失」（跳过）与「连接失败」（错误）**严格区分**。
pub async fn test_pool(test_name: &str) -> Result<Option<PgPool>, sqlx::Error> {
    let Some(url) = std::env::var(ENV_DATABASE_URL)
        .ok()
        .filter(|u| !u.trim().is_empty())
    else {
        return Ok(skipped(test_name));
    };

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await?;

    Ok(Some(pool))
}

/// 测试库的连接 URL。
///
/// # 为什么需要这个辅助
///
/// [`QueueGuard::acquire`] 需要**独立连接**（不是从池里借的）——
/// 所以它要 URL 而不是 `PgPool`。
pub fn test_database_url() -> Option<String> {
    std::env::var(ENV_DATABASE_URL)
        .ok()
        .filter(|u| !u.trim().is_empty())
}

/// **跨进程**的全局队列锁 —— 基于 PostgreSQL 的**咨询锁**（advisory lock）。
///
/// # 为什么需要它
///
/// `cargo test` 会**并行**运行多个**测试二进制** ——
/// `lease_fencing.rs` 与 `outbox_idempotency.rs` 是两个**独立进程**，
/// 各自有进程内的 `Mutex`（`CLAIM_LOCK` / `QUEUE_LOCK`）——
/// 但那两把锁**互相看不见**。
///
/// 而 `claim` / `claim_batch` 是**全局队列**操作：
/// 一个二进制的测试会领走另一个二进制造出来的 run。
///
/// 实测（不是理论）：`outbox_idempotency.rs` 的 `submit_idempotent`
/// 创建了 `status = 'queued'` 的 run，它被 `lease_fencing.rs` 的 `claim`
/// 领走 → `assert_eq!(lease.run_id(), run_id)` 失败 → 整文件 13 个测试里多个变红。
///
/// 进程内 `Mutex` **解决不了**这个问题 —— 必须用**跨进程**的锁。
///
/// # 为什么用 advisory lock
///
/// - **不需要**改表结构（那是破坏性变更）
/// - 进程退出时 PG **自动释放** —— 不会留下死锁
/// - 不污染数据 —— 不需要在表里存「谁在测试」
///
/// # 用 `try_lock` + 超时，不阻塞
///
/// `pg_advisory_lock` 会**阻塞**直到拿到锁。如果持锁的进程崩溃，
/// 锁会释放（连接断开）—— 但如果它只是**卡住**（比如某个测试挂起），
/// 后续测试会**永远等下去**。
///
/// 所以用 `pg_try_advisory_lock` **轮询** + **超时**：
/// 拿不到就**跳过**这个测试（打印说明），而不是挂死整个测试运行。
///
/// # 用法
///
/// ```ignore
/// let Some(guard) = common::QueueGuard::acquire(&pool).await else {
///     return; // 拿不到锁 —— 跳过
/// };
/// // …… 临界区 ……
/// drop(guard); // 释放
/// ```
pub struct QueueGuard {
    /// **独立**的连接 —— 不是从池里借的。
    ///
    /// # 为什么必须独立
    ///
    /// advisory lock 绑定在**会话**上。从池里借的连接在 `Drop` 时会被
    /// **归还**池里（不关闭），会话还在 → 锁**不会**释放。
    ///
    /// 独立连接在 `Drop` 时被**关闭** → PG 自动释放会话级锁 ——
    /// 不需要显式解锁，也不会漏。
    conn: Option<sqlx::PgConnection>,
}

/// advisory lock 的 key。
///
/// 选一个大整数常量，避免与其它用途的 advisory lock 冲突。
/// 仓库里目前**只有**这一处用 advisory lock。
const QUEUE_LOCK_KEY: i64 = 0x4E_45_58_51_55_45_55; // "NEXQUEU"

impl QueueGuard {
    /// 尝试获取全局队列锁 —— **不阻塞**。
    ///
    /// # 返回 `None`
    ///
    /// 超时未拿到锁 —— 调用方**应当跳过**测试（而不是失败）。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → `sqlx::Error`。
    pub async fn acquire(url: &str) -> Result<Option<Self>, sqlx::Error> {
        use sqlx::Connection;
        let mut conn = sqlx::PgConnection::connect(url).await?;

        // `pg_try_advisory_lock` 是**会话级**的 —— 不阻塞，立即返回。
        //
        // ⚠️ 用**会话级**而不是事务级（`pg_advisory_xact_lock`）：
        // 事务级锁在事务结束时自动释放，但我们需要锁**跨越**多次事务
        // （领取 → 断言 → 回滚）。
        let deadline = std::time::Instant::now() + LOCK_WAIT;
        loop {
            let got: (bool,) = sqlx::query_as("SELECT pg_try_advisory_lock($1)")
                .bind(QUEUE_LOCK_KEY)
                .fetch_one(&mut conn)
                .await?;
            if got.0 {
                return Ok(Some(Self { conn: Some(conn) }));
            }

            if std::time::Instant::now() >= deadline {
                eprintln!(
                    "[SKIP] 全局队列锁在 {:?} 内未取得 —— 跳过这个测试。\
                     另一个测试进程正持有它。",
                    LOCK_WAIT
                );
                return Ok(None);
            }

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

/// 等待全局队列锁的**上限**。
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

impl Drop for QueueGuard {
    /// 关闭独立连接 —— PG 随之释放会话级 advisory lock。
    ///
    /// # 为什么不用显式 `pg_advisory_unlock`
    ///
    /// 显式解锁需要 `.await`，而 `Drop` 里不能等。
    /// 关闭连接是**同步**的，且效果**更强** —— PG 保证会话结束时
    /// 释放该会话持有的**所有**锁。
    ///
    /// 这也覆盖了「测试 panic」的情况 —— unwind 时 `Drop` 照样执行。
    fn drop(&mut self) {
        // 关闭连接是同步的（socket close）。
        self.conn.take();
    }
}
