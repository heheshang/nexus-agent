//! 结构层架构测试 —— 行类型与表结构的**对应关系**。
//!
//! # 为什么这是结构层而不是行为层
//!
//! 行为层（集成测试）验证「仓储能用」；结构层验证「**每张表都有对应的行类型**」
//! —— 漏一张表时，它会在第一次被查询时才暴露（运行时错误），
//! 而不是编译期。
//!
//! 架构文档 §3.9.4 点名 Phase 0 核心表一次建齐 —— 行类型也应当一次到位。
//!
//! # 三条断言
//!
//! 1. 每个行类型都实现了 `FromRow`（**编译期**检查 —— 漏派生就编译不过）。
//! 2. 表清单的长度与行类型数**一致** —— 防止「加了表但忘了写行类型」。
//! 3. `nexus-repo` 的源码里**不出现** `workflow_runs` / `workflow_step_runs`
//!    —— 架构文档 §4.2 差异 1 与 `docs/operations/p2-plan.md` 红线 3。

use nexus_repo::rows::{
    AgentRow, ExecutionApprovalRow, ExecutionAttemptRow, ExecutionCheckpointRow, ExecutionEventRow,
    ExecutionOperationRow, ExecutionOutboxRow, ExecutionRunRow, McpServerRow, MessageRow,
    ModelConfigRow, ModelProviderRow, SessionRow, ToolRow, UserRow, WorkflowRow,
    WorkflowVersionRow,
};
use sqlx::FromRow;

/// Phase 0 的表名清单（`nexus-execution` 独占的两张表除外）。
///
/// ⚠️ **不含** `workflow_runs` / `workflow_step_runs` —— 那两张表由
/// `nexus-execution` 独占（架构文档 §4.2 差异 1）。
const PHASE0_TABLES_EXCLUDING_EXECUTION_OWNED: &[&str] = &[
    "users",
    "model_providers",
    "model_configs",
    "agents",
    "mcp_servers",
    "tools",
    "workflows",
    "workflow_versions",
    "execution_runs",
    "execution_checkpoints",
    "execution_operations",
    "execution_attempts",
    "execution_approvals",
    "execution_events",
    "execution_outbox",
    "sessions",
    "messages",
];

/// 每个行类型都实现了 `FromRow`。
///
/// 编译器在这里逐个检查 —— 任何一个行类型漏掉 `#[derive(FromRow)]`
/// 都会让本测试**编译失败**。
#[test]
fn every_phase0_row_type_implements_from_row() {
    // `FromRow` 的签名是 `FromRow<'r, R: Row>` —— 这里用 `PgRow` 作为具体的行类型。
    fn assert_from_row<R: FromRow<'static, sqlx::postgres::PgRow>>() {}

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

/// 清单长度与实际行类型数**一致**。
///
/// 防止「往清单里加了表但忘了写行类型」——那种情况下
/// 上一条测试不会失败（它只检查已有的那些），但清单会**多出一项**，
/// 这里就会失败。
#[test]
fn phase0_table_list_matches_row_type_count() {
    assert_eq!(
        PHASE0_TABLES_EXCLUDING_EXECUTION_OWNED.len(),
        17,
        "Phase 0（除 nexus-execution 独占的两张表）应有 17 张表"
    );
}

/// `nexus-repo` **不得**引用 `workflow_runs` / `workflow_step_runs`。
///
/// 架构文档 §4.2 差异 1：`workflow_runs` 由 `nexus-execution` 独占
/// —— `nexus-repo` 引用它就是偏离架构决策。
///
/// # 为什么用**源码扫描**而不是编译期断言
///
/// 编译期断言要求「不存在某个类型」—— 而 Rust 没有「负向类型约束」。
/// 只能扫描源码。这是 `scripts/check-deps.sh` 规则 3 的同类手法
/// （那里也是扫描 `Cargo.toml`）。
///
/// 扫描范围是**本 crate 的 `src/` 目录** —— 测试文件本身会提到
/// 这两个名字（就在本文件的注释与常量里），所以必须排除。
#[test]
fn nexus_repo_must_not_reference_execution_owned_tables() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<String> = Vec::new();

    let mut stack = vec![src_dir];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).expect("读取源码目录失败");
        for entry in entries {
            let entry = entry.expect("读取目录项失败");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let content = std::fs::read_to_string(&path).expect("读取源码文件失败");
            for (line_no, line) in content.lines().enumerate() {
                // 只看**代码**：跳过注释行与文档注释行。
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                for forbidden in ["workflow_runs", "workflow_step_runs"] {
                    if trimmed.contains(forbidden) {
                        offenders.push(format!(
                            "{}:{}: 出现了 {forbidden}",
                            path.display(),
                            line_no + 1
                        ));
                    }
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "nexus-repo 不得引用 nexus-execution 独占的表：\n{}",
        offenders.join("\n")
    );
}
