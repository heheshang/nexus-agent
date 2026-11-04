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
