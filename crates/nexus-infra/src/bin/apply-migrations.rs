//! `apply-migrations` —— 把嵌入式迁移应用到目标数据库。
//!
//! # 为什么需要这个二进制
//!
//! CI 的 PostgreSQL `services:` 起的是一个**空库** ——
//! `postgres:17` 镜像只创建数据库，**不建任何表**。
//!
//! 而集成测试直接操作 `public` schema 的表 —— 空库上跑必然全红。
//!
//! 所以 CI 需要一步「先把表建出来」。有两个选项：
//!
//! | 方案 | 问题 |
//! |---|---|
//! | CI 里装 `psql` 依次跑 `.up.sql` | runner 要额外装客户端；且**绕过**了 sqlx 的版本记账（`_sqlx_migrations`），迁移会被重复应用 |
//! | 用 Rust 跑 [`sqlx::migrate!`] | 迁移器**已嵌入二进制**，版本记账由 sqlx 负责，幂等 |
//!
//! 选后者 —— [`nexus_infra::migrate::run`] 已经封装好了一切，
//! 这个二进制只做「读 URL → 建池 → 跑迁移 → 断言表齐了」。
//!
//! # 为什么顺带断言表齐了
//!
//! 「迁移跑完了」与「表建对了」是两件事 —— sqlx 会**静默忽略**
//! 文件名格式不符的迁移文件。断言 [`PHASE0_TABLES`] 里的表都存在，
//! 才能发现「某个迁移被跳过了」。
//!
//! # 用法
//!
//! ```bash
//! DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_test \
//!   cargo run -p nexus-infra --bin apply-migrations
//! ```

use std::process::ExitCode;

use sqlx::postgres::PgPoolOptions;

/// 环境变量名 —— 数据库连接 URL。
const ENV_DATABASE_URL: &str = "DATABASE_URL";

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = std::env::var(ENV_DATABASE_URL)
        .ok()
        .filter(|u| !u.trim().is_empty())
    else {
        eprintln!(
            "错误：环境变量 {ENV_DATABASE_URL} 未设置 —— 无法应用迁移。\n\
             例：{ENV_DATABASE_URL}=postgres://nexus:nexus@127.0.0.1:5432/nexus_test"
        );
        return ExitCode::FAILURE;
    };

    let pool = match PgPoolOptions::new().max_connections(2).connect(&url).await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("错误：连不上数据库：{e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = nexus_infra::migrate::run(&pool).await {
        eprintln!("错误：迁移失败：{e}");
        return ExitCode::FAILURE;
    }

    let version = match nexus_infra::migrate::current_version(&pool).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误：读不到当前迁移版本：{e}");
            return ExitCode::FAILURE;
        }
    };

    // 表齐了才算成功 —— 迁移「跑完了」不等于「表建对了」。
    if let Err(missing) = nexus_infra::migrate::assert_phase0_tables(&pool).await {
        eprintln!("错误：{missing}");
        return ExitCode::FAILURE;
    }

    println!("迁移已应用（当前版本 {version}，Phase 0 的表全部就位）。");
    ExitCode::SUCCESS
}
