//! 迁移基线的集成测试。
//!
//! 验证 P2 判据「**迁移可应用**」—— 真实 PostgreSQL 上的行为。
//!
//! # 「无库则跳过」，但 CI 里**不许**跳过
//!
//! 这些测试需要真实数据库。没配 `DATABASE_URL` 时**跳过**（不是失败）——
//! 否则每个没有 PG 的开发者、以及 CI 的非集成 job 都会看到红灯。
//!
//! 判据是 `DATABASE_URL` **是否存在**，不检查它连不连得上 ——
//! 连不上时测试会**失败**（那是真的坏了），而不是被静默跳过。
//!
//! ⚠️ **但 CI 里缺 `DATABASE_URL` 会直接失败** —— 见 [`database_url`]。
//! 否则「跳过」在测试摘要里显示成 `ok`，CI 全绿但**一条数据库测试都没跑** ——
//! 「集成测试绿」就成了假象。这正是 NEX-14 的 AC #14 要防的事，
//! 在这里就先堵上。

use sqlx::AssertSqlSafe;

/// 测试用的数据库 URL。
///
/// 返回 `None` 表示「本环境不该跑集成测试」。
///
/// # CI 里的特殊处理
///
/// CI 环境下**缺 `DATABASE_URL` 是错误**，不是「跳过」——
/// 因为 CI **一定**应该能跑集成测试（`.github/workflows/ci.yml` 配了
/// `postgres:17` 服务）。缺了就说明 CI 配置坏了，必须暴露出来。
///
/// 本地开发没配库时**跳过** —— 那不是错误，只是「这台机器没准备数据库」。
fn database_url() -> Option<String> {
    let url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|u| !u.trim().is_empty());

    if url.is_none() && is_ci() {
        panic!(
            "CI 环境下必须有 DATABASE_URL —— 集成测试被跳过了，\
             「集成测试绿」是假象。检查 .github/workflows/ci.yml 的 services 与 env。"
        );
    }

    url
}

/// 是否在 CI 环境。
///
/// `CI` 是 GitHub Actions 等 CI 平台统一设置的环境变量。
fn is_ci() -> bool {
    std::env::var("CI")
        .map(|v| !v.is_empty() && v != "false" && v != "0")
        .unwrap_or(false)
}

/// 测试用的数据库 URL。
///
/// 标记测试为「需要数据库」。
///
/// 展开成「连不上就 panic」—— 数据库**连得上但迁移失败**是必须暴露的问题，
/// 不该被当成「跳过」。
macro_rules! require_db {
    () => {
        match database_url() {
            Some(url) => url,
            None => {
                eprintln!("跳过：未配置 DATABASE_URL");
                return;
            }
        }
    };
}

/// 连到测试库，并在**独立 schema** 里建表。
///
/// # 为什么每个测试一个 schema
///
/// 集成测试会**清空**目标 —— 所以多个测试并行跑时必须互相隔离，
/// 否则一个测试的 `DROP SCHEMA` 会把另一个测试刚建的表删掉，
/// 失败原因还会指向错误的测试（极难排查）。
///
/// 用 schema 隔离而不是「每个测试一个数据库」—— 建库要连 `postgres` 库、
/// 要等 DROP DATABASE 完成，慢得多；schema 隔离是同样的隔离度、便宜得多。
///
/// # 关于动态 SQL
///
/// schema 名是**测试里的字面量常量**，不是外部输入 —— 所以
/// [`AssertSqlSafe`] 在这里是**正当**的。但仍要过一道校验：
/// `assert_safe_schema_name` 拒绝任何非标识符字符。
async fn fresh_pool(url: &str, schema: &str) -> sqlx::PgPool {
    assert_safe_schema_name(schema);

    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .expect("应能连上测试库");

    // 清掉可能残留的同名 schema，再建一个干净的。
    sqlx::query(AssertSqlSafe(format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE"
    )))
    .execute(&admin)
    .await
    .expect("应能清理旧 schema");
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .expect("应能建 schema");
    admin.close().await;

    // 连接串上挂 `search_path` —— 池里的每条连接都落到该 schema。
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .expect("URL 应能解析")
        .options([("search_path", schema)]);

    sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("应能连上测试库")
}

/// 断言 schema 名是安全的标识符。
///
/// 只有字母、数字和下划线，且不以数字开头 —— 杜绝 SQL 注入的余地。
/// 这样 [`AssertSqlSafe`] 的使用就是**有依据**的，而不是「我保证没事」。
fn assert_safe_schema_name(name: &str) {
    assert!(
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit()),
        "schema 名必须是纯标识符：{name}"
    );
}

#[tokio::test]
async fn migrations_apply_to_an_empty_database() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_apply").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    // 「迁移跑完了」与「表建对了」是两件事 —— 必须断言具体的表。
    nexus_infra::migrate::assert_phase0_tables(&pool)
        .await
        .expect("Phase 0 的表应全部存在");
}

#[tokio::test]
async fn migrations_are_idempotent() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_idem").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("首次迁移应成功");
    nexus_infra::migrate::run(&pool)
        .await
        .expect("重复迁移不应报错");

    let version = nexus_infra::migrate::current_version(&pool)
        .await
        .expect("应能读到当前版本");
    assert_eq!(version, 6, "应有 6 组迁移");
}

#[tokio::test]
async fn partial_indexes_are_actually_partial() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_idx").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    // 「只索引就绪行」是 §3.9.1 的关键索引模式。
    // 如果 WHERE 子句丢了，索引会退化成全表索引 —— 大表上从毫秒变秒。
    for index in [
        "idx_execution_ready",
        "idx_execution_active_owner",
        "idx_execution_live_deadline",
        "idx_execution_outbox_pending",
        "idx_mcp_active_tools",
        "idx_mcp_active_servers",
        "idx_messages_memory_cursor",
    ] {
        nexus_infra::migrate::assert_partial_index(&pool, index)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
    }
}

#[tokio::test]
async fn idempotency_key_is_unique_per_owner() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_idemkey").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    // 幂等键约束（§4.1.1）：`(owner_id, request_key)` 唯一。
    // 重复提交返回既有 run，靠的就是这条约束。
    sqlx::query("INSERT INTO users (username, email, password_hash) VALUES ('u1', 'u1@x', 'x')")
        .execute(&pool)
        .await
        .expect("应能插入 user");
    let user_id: (i64,) = sqlx::query_as("SELECT id FROM users WHERE username = 'u1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let owner_id = user_id.0;

    // 重复的 (owner_id, request_key) 必须被唯一约束拒绝。
    let insert_sql = "INSERT INTO execution_runs \
         (definition, definition_hash, engine_version, owner_id, request_key) \
         VALUES ('{}'::jsonb, 'abc', 'v1', $1, 'key-1')";

    sqlx::query(insert_sql)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect("首次插入应成功");

    let err = sqlx::query(insert_sql)
        .bind(owner_id)
        .execute(&pool)
        .await
        .expect_err("幂等键重复应被拒绝");

    assert!(
        err.to_string().contains("duplicate key") || err.to_string().contains("unique"),
        "应是唯一约束错误，实际：{err}"
    );
}

#[tokio::test]
async fn replay_policy_check_constraint_is_enforced() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_policy").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    sqlx::query(
        "INSERT INTO tools (code, name, type, description, parameters, replay_policy) \
         VALUES ('t1', 'T1', 'builtin', 'd', '{}'::jsonb, 'readonly')",
    )
    .execute(&pool)
    .await
    .expect("合法取值应能插入");

    // 约束下沉到数据库（§3.9.2）—— 非法取值在数据库层就被拒。
    let err = sqlx::query(
        "INSERT INTO tools (code, name, type, description, parameters, replay_policy) \
         VALUES ('t2', 'T2', 'builtin', 'd', '{}'::jsonb, 'nonsense')",
    )
    .execute(&pool)
    .await
    .expect_err("非法 replay_policy 应被 CHECK 约束拒绝");

    assert!(
        err.to_string().contains("replay_policy") || err.to_string().contains("check"),
        "应是 CHECK 约束错误，实际：{err}"
    );
}

#[tokio::test]
async fn execution_runs_is_a_separate_table_from_workflow_runs() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_split").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    // 「两表分离」是 §3.9.4 的明文偏离 —— 用集成测试锁住它。
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = current_schema() \
           AND table_name IN ('execution_runs', 'workflow_runs')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(rows.len(), 2, "两张表都应存在");

    // `workflow_runs` 必须**引用** `execution_runs`（产品视图挂在引擎侧）。
    let fk: Vec<(String,)> = sqlx::query_as(
        "SELECT c.conname FROM pg_constraint c \
         JOIN pg_class t ON t.oid = c.conrelid \
         WHERE c.contype = 'f' AND t.relname = 'workflow_runs' \
           AND c.confrelid = 'execution_runs'::regclass",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    assert_eq!(fk.len(), 1, "workflow_runs 应外键引用 execution_runs");
}

#[tokio::test]
async fn migrations_can_be_rolled_back() {
    let url = require_db!();
    let pool = fresh_pool(&url, "t_undo").await;

    nexus_infra::migrate::run(&pool)
        .await
        .expect("迁移应能应用");

    // 回滚到版本 0 —— 所有**业务**表都应消失。
    //
    // 注意：`_sqlx_migrations` 是 sqlx 自己的记账表，**不会**被回滚删除 ——
    // 它记录「哪些版本已应用」。删掉它反而会破坏迁移状态。
    nexus_infra::migrate::undo(&pool, 0)
        .await
        .expect("回滚应成功");

    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = current_schema() AND table_type = 'BASE TABLE'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    let business_tables: Vec<&str> = rows
        .iter()
        .map(|(t,)| t.as_str())
        .filter(|t| !t.starts_with("_sqlx"))
        .collect();

    assert!(
        business_tables.is_empty(),
        "回滚后不应剩下业务表，实际：{business_tables:?}"
    );
}
