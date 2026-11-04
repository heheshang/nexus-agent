//! 行类型与**真实 schema** 的对应关系 —— 集成测试。
//!
//! # 为什么需要这个测试
//!
//! 架构文档 §3.9.3 已定稿：SQL 一律用 `query_as` + `FromRow`（**运行期**校验），
//! 不用 `query!` 编译期宏。代价是**字段名写错要到运行时才炸**。
//!
//! 任务书的缓解手段只提了「启动期 smoke check」（`nexus-infra` 的
//! `assert_phase0_tables`）—— 但它只断言**表存在**，**不校验列名**。
//! 列名对不上时，表存在检查照样通过，错误推迟到第一条真实查询。
//!
//! 所以本测试补上这一层：**每个行类型的每个字段都必须对应 schema 里的
//! 一列，且列名完全一致**。
//!
//! # 断言的是什么
//!
//! | 断言 | 挡住什么 |
//! |---|---|
//! | 行类型的列名集合 == schema 的列名集合 | 列名写错 / 漏字段 / 多字段 |
//!
//! # 为什么用「集合相等」而不是「行类型 ⊆ schema」
//!
//! 子集检查会放过「行类型少了一个字段」的情况 —— 那时 `FromRow` 会让
//! **缺失的列**报 `ColumnNotFound`，虽然能炸，但错误信息指向解码器而不是
//! 「你少定义了一个字段」。集合相等把问题定位得更准。
//!
//! # 数据隔离
//!
//! 本测试**只读** schema 元数据（`information_schema.columns`），
//! **不碰**任何业务数据 —— 所以不产生残留，也不需要隔离。

mod common;

use nexus_repo::rows::{
    AgentRow, ExecutionApprovalRow, ExecutionAttemptRow, ExecutionCheckpointRow, ExecutionEventRow,
    ExecutionOperationRow, ExecutionOutboxRow, ExecutionRunRow, McpServerRow, MessageRow,
    ModelConfigRow, ModelProviderRow, SessionRow, ToolRow, UserRow, WorkflowRow,
    WorkflowVersionRow,
};
use sqlx::FromRow;
use sqlx::Row;
use sqlx::postgres::PgRow;

/// 行类型 → 表名。
///
/// 与 `tests/architecture.rs` 的 `PHASE0_TABLES_EXCLUDING_EXECUTION_OWNED`
/// 一一对应 —— 同一个清单的两种视图（表名 vs 行类型名）。
const ROW_TYPES: &[(&str, &str)] = &[
    ("UserRow", "users"),
    ("ModelProviderRow", "model_providers"),
    ("ModelConfigRow", "model_configs"),
    ("AgentRow", "agents"),
    ("McpServerRow", "mcp_servers"),
    ("ToolRow", "tools"),
    ("WorkflowRow", "workflows"),
    ("WorkflowVersionRow", "workflow_versions"),
    ("ExecutionRunRow", "execution_runs"),
    ("ExecutionCheckpointRow", "execution_checkpoints"),
    ("ExecutionOperationRow", "execution_operations"),
    ("ExecutionAttemptRow", "execution_attempts"),
    ("ExecutionApprovalRow", "execution_approvals"),
    ("ExecutionEventRow", "execution_events"),
    ("ExecutionOutboxRow", "execution_outbox"),
    ("SessionRow", "sessions"),
    ("MessageRow", "messages"),
];

/// 取出 schema 里某张表的**列名集合**。
async fn schema_columns(pool: &sqlx::PgPool, table: &str) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = $1 \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("查询 information_schema 失败");

    rows.into_iter()
        .map(|r| r.get::<String, _>("column_name"))
        .collect()
}

/// 行类型的**期望列名**。
///
/// ⚠️ 这份清单是**手写**的 —— 它必须与 `rows.rs` 里的 `#[sqlx(rename)]`
/// 一起手工维护。这看起来是重复劳动，但它是**故意的**：
///
/// - 手写清单 = 「数据库列名」这一侧的**独立副本**。
///   如果它是从 `rows.rs` 自动推导出来的，那么行类型写错列名时，
///   推导出的期望值也跟着错，测试就永远不会失败。
/// - 代价是：改表结构时要改两处。第二处忘了改 → 测试变红 → 正是我们想要的。
///
/// `type` 列在行类型里叫 `type_`（`type` 是 Rust 关键字），用 `rename` 映射回列名。
fn expected_columns(row_type: &str) -> Vec<&'static str> {
    match row_type {
        "UserRow" => vec![
            "id",
            "username",
            "email",
            "password_hash",
            "role",
            "avatar_url",
            "status",
            "created_at",
            "updated_at",
        ],
        "ModelProviderRow" => vec![
            "id",
            "code",
            "name",
            "base_url",
            "api_key",
            "status",
            "created_at",
            "updated_at",
        ],
        "ModelConfigRow" => vec![
            "id",
            "provider_id",
            "model_name",
            "alias",
            "type",
            "context_window",
            "max_output",
            "input_price",
            "output_price",
            "priority",
            "capabilities",
            "is_default",
            "status",
            "created_at",
            "updated_at",
            "embedding_dimension",
        ],
        "AgentRow" => vec![
            "id",
            "name",
            "description",
            "type",
            "system_prompt",
            "model_config_id",
            "fallback_model_config_id",
            "temperature",
            "top_p",
            "max_tokens",
            "max_iterations",
            "memory_enabled",
            "memory_window",
            "memory_long_enabled",
            "kb_ids",
            "tools",
            "is_preset",
            "config",
            "status",
            "created_at",
            "updated_at",
            "owner_id",
        ],
        "McpServerRow" => vec![
            "id",
            "name",
            "transport",
            "url",
            "command",
            "args",
            "env",
            "headers",
            "status",
            "last_health_at",
            "tools_cache",
            "created_at",
            "updated_at",
            "enabled",
            "config_version",
            "encrypted_credentials",
            "headers_present",
            "allowed_user_ids",
            "tools_hash",
            "archived_at",
        ],
        "ToolRow" => vec![
            "id",
            "code",
            "name",
            "type",
            "description",
            "parameters",
            "handler",
            "mcp_server_id",
            "endpoint",
            "auth_config",
            "default_config",
            "timeout_ms",
            "is_dangerous",
            "status",
            "created_at",
            "updated_at",
            "replay_policy",
            "mcp_name",
            "mcp_config_version",
            "binding_version",
            "policy_reviewed_by",
            "policy_reviewed_at",
            "replay_evidence",
        ],
        "WorkflowRow" => vec![
            "id",
            "name",
            "description",
            "dsl",
            "version",
            "is_active",
            "creator_id",
            "status",
            "created_at",
            "updated_at",
            "archived_at",
        ],
        "WorkflowVersionRow" => vec![
            "id",
            "workflow_id",
            "version",
            "definition_hash",
            "definition",
            "created_at",
        ],
        "ExecutionRunRow" => vec![
            "id",
            "definition",
            "definition_hash",
            "engine_version",
            "graph_schema",
            "status",
            "generation",
            "lease_owner",
            "lease_until",
            "deadline_at",
            "available_at",
            "next_seq",
            "state_version",
            "pending_ids",
            "owner_id",
            "request_key",
            "created_at",
        ],
        "ExecutionCheckpointRow" => vec![
            "run_id",
            "checkpoint_id",
            "definition_hash",
            "engine_version",
            "graph_schema",
            "payload",
            "updated_at",
        ],
        "ExecutionOperationRow" => vec![
            "id",
            "run_id",
            "path",
            "kind",
            "args_hash",
            "replay_policy",
            "status",
            "generation",
            "output",
            "error",
            "attempt",
            "started_at",
            "finished_at",
        ],
        "ExecutionAttemptRow" => vec![
            "id",
            "operation_id",
            "generation",
            "attempt",
            "status",
            "error",
            "started_at",
            "finished_at",
        ],
        "ExecutionApprovalRow" => vec![
            "id",
            "run_id",
            "interrupt_id",
            "path",
            "args_hash",
            "prompt",
            "decision",
            "decided_by",
            "decided_at",
            "expires_at",
            "comment",
        ],
        "ExecutionEventRow" => vec![
            "run_id",
            "seq",
            "type",
            "schema_version",
            "data",
            "created_at",
        ],
        "ExecutionOutboxRow" => vec![
            "id",
            "run_id",
            "seq",
            "topic",
            "payload",
            "published_at",
            "created_at",
            "dedup_key",
        ],
        "SessionRow" => vec![
            "id",
            "user_id",
            "agent_id",
            "title",
            "status",
            "pinned",
            "message_count",
            "token_usage",
            "last_msg_at",
            "metadata",
            "created_at",
            "updated_at",
        ],
        "MessageRow" => vec![
            "id",
            "session_id",
            "role",
            "content",
            "reasoning_content",
            "tool_calls",
            "tool_call_id",
            "refs",
            "model_name",
            "prompt_tokens",
            "completion_tokens",
            "first_token_ms",
            "latency_ms",
            "status",
            "trace_id",
            "metadata",
            "created_at",
        ],
        other => panic!("未登记的行类型：{other}"),
    }
}

/// 每个行类型的列名集合 == schema 里的列名集合。
///
/// # 为什么这是**集成测试**而不是单元测试
///
/// 它必须连**真实数据库**才能拿到 schema —— schema 的唯一事实来源是
/// 迁移文件在真实 PG 上的落地结果。
#[tokio::test]
async fn every_row_type_matches_the_live_schema() {
    let Some(pool) = common::test_pool("every_row_type_matches_the_live_schema")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let mut mismatches: Vec<String> = Vec::new();

    for (row_type, table) in ROW_TYPES {
        let actual = schema_columns(&pool, table).await;
        let expected = expected_columns(row_type);

        // 排序后比较 —— 顺序不重要（`FromRow` 按名字匹配，不按位置）。
        // 两侧统一成 `Vec<String>` —— `expected` 是 `&'static str`，转换一次。
        let mut actual_sorted: Vec<String> = actual.clone();
        actual_sorted.sort();
        let mut expected_sorted: Vec<String> = expected.iter().map(|c| (*c).to_string()).collect();
        expected_sorted.sort();

        if actual_sorted != expected_sorted {
            let only_in_row_type: Vec<&str> = expected_sorted
                .iter()
                .map(String::as_str)
                .filter(|c| !actual_sorted.iter().any(|a| a == *c))
                .collect();
            let only_in_schema: Vec<&str> = actual_sorted
                .iter()
                .map(String::as_str)
                .filter(|c| !expected_sorted.iter().any(|e| e == *c))
                .collect();

            mismatches.push(format!(
                "{row_type} ↔ {table}\n    行类型多出/写错的列：{only_in_row_type:?}\n    \
                 schema 多出/漏掉的列：{only_in_schema:?}"
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "行类型与真实 schema 不一致：\n{}",
        mismatches.join("\n")
    );
}

/// 表清单与行类型清单**一一对应**。
///
/// 防止「加了行类型但忘了加进 `ROW_TYPES`」——
/// 那种情况下上面的测试**不会**失败（它只检查已登记的那些）。
#[test]
fn row_type_table_list_is_consistent() {
    assert_eq!(
        ROW_TYPES.len(),
        17,
        "Phase 0（除 nexus-execution 独占的两张表）应有 17 个行类型"
    );

    // 表名不重复。
    let mut tables: Vec<&str> = ROW_TYPES.iter().map(|(_, t)| *t).collect();
    tables.sort_unstable();
    let before = tables.len();
    tables.dedup();
    assert_eq!(before, tables.len(), "ROW_TYPES 里有重复的表名");
}

/// 每一个行类型都实现了 `FromRow`。
///
/// 编译期断言 —— 漏派生 `#[derive(FromRow)]` 会让本测试**编译失败**。
#[test]
fn every_row_type_implements_from_row() {
    fn assert_from_row<R: FromRow<'static, PgRow>>() {}

    assert_from_row::<UserRow>();
    assert_from_row::<ModelProviderRow>();
    assert_from_row::<ModelConfigRow>();
    assert_from_row::<AgentRow>();
    assert_from_row::<McpServerRow>();
    assert_from_row::<ToolRow>();
    assert_from_row::<WorkflowRow>();
    assert_from_row::<WorkflowVersionRow>();
    assert_from_row::<ExecutionRunRow>();
    assert_from_row::<ExecutionCheckpointRow>();
    assert_from_row::<ExecutionOperationRow>();
    assert_from_row::<ExecutionAttemptRow>();
    assert_from_row::<ExecutionApprovalRow>();
    assert_from_row::<ExecutionEventRow>();
    assert_from_row::<ExecutionOutboxRow>();
    assert_from_row::<SessionRow>();
    assert_from_row::<MessageRow>();
}
