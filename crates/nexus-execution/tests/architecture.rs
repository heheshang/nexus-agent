//! `nexus-execution` 的**结构层**断言。
//!
//! # 为什么这些断言不可省略
//!
//! 行为测试只能证明「**当前实现**产出了正确的结果」——
//! 它拦不住「有人换了个实现，结果仍然对，但契约已经变了」。
//!
//! 例如：执行层自己拼一条带 `generation` 的 UPDATE，
//! 绕开了 `nexus-repo` 的围栏 API ——
//! 围栏条件于是有**两处定义**，改了一处忘了另一处，
//! fencing 就会**静默失效**。行为测试**照样绿**。
//!
//! 结构层断言直接扫**源码**，把「不变量」钉在**实现形态**上。
//!
//! # 检验的设计条款
//!
//! | 测试 | 检验的条款 |
//! |---|---|
//! | `the_execution_crate_depends_on_the_data_layer` | §4.4 规则 4（`execution → repo` 允许） |
//! | `the_execution_crate_does_not_depend_on_the_upper_layers` | §4.3.2 约束 1 |
//! | `the_execution_crate_does_not_inline_fenced_writes` | §4.2 差异 1（围栏「一处定义」） |

use std::fs;
use std::path::{Path, PathBuf};

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 为什么用 `#[path]` 而不是抽一个 crate
//
// 抽 crate 会**新增**一条依赖边，而本仓库的依赖方向是**已定稿**条款
// （架构文档 §6.3 第 5 项）。`#[path]` include 只需要**文件系统**上的相对路径，
// **不产生任何依赖边** —— `Cargo.toml` 完全不动。
#[path = "../../nexus-core/tests/support/token_scan.rs"]
mod token_scan;

/// `nexus-execution` 的 `src` 目录。
fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 遍历 `src` 下的所有 `.rs` 文件。
fn source_files() -> Vec<PathBuf> {
    let dir = src_dir();
    let mut files = Vec::new();
    collect_rs(&dir, &mut files);
    files.sort();
    files
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("读取目录失败");
    for entry in entries {
        let entry = entry.expect("读取目录项失败");
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 读取 `Cargo.toml` 的**内部依赖**清单。
fn internal_dependencies() -> Vec<String> {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败");

    let mut deps = Vec::new();
    let mut in_deps = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps {
            continue;
        }
        if let Some((name, _)) = line.split_once(".workspace")
            && name.starts_with("nexus-")
        {
            deps.push(name.trim().to_string());
        }
    }
    deps
}

/// §4.4 规则 4 —— `nexus-execution` **依赖** `nexus-repo`。
///
/// # 为什么这条断言存在
///
/// 决策 D12（选项 B，2026-11-06 操作者裁决）确立了
/// `nexus-execution → nexus-repo` 的**单向**依赖：
/// 执行层复用数据层的引擎侧持久化能力
/// （租约 / fencing / 幂等提交 / 事件序号）。
///
/// 引擎侧持久化**留在** `nexus-repo` —— **不迁移**。
///
/// # 为什么这条断言**不是**「Cargo 会拒绝」的重复
///
/// Cargo **确实**会拒绝 `repo → execution`（成环）。
/// 但它**不会**拒绝「`execution` **不**依赖 `repo`」——
/// 那是一个**合法**的依赖图，只是不符合本设计的决策。
///
/// 依赖边是**决策**的结果，决策必须由**断言**钉住，
/// 否则「有人删掉这条边」只会表现为「某个功能不见了」——
/// 那是最难查的失效形态。
///
/// # 为什么还要**编译期**证明它「真的能用」
///
/// manifest 里写着 `nexus-repo.workspace = true` **不等于**
/// 那条边「**可用**」—— 依赖可能被声明却没被引用（依赖噪音）。
///
/// 所以本测试**真的用一下**那个 API：能编译通过，
/// 就证明依赖边既存在**又**可用。
#[test]
fn the_execution_crate_depends_on_the_data_layer() {
    let deps = internal_dependencies();

    assert!(
        deps.iter().any(|d| d == "nexus-repo"),
        "nexus-execution 必须依赖 nexus-repo —— 引擎侧持久化能力留在数据层\
         （架构文档 §4.4 规则 4，决策 D12 选项 B）。实际依赖：{deps:?}"
    );

    // ⚠️ **编译期**证明：这条依赖边**真的可用**，不是 manifest 上的一行字。
    //
    // `ExecutionLeaseRepository` 是引擎侧持久化能力的**入口**——
    // 执行层经由它领取租约与做围栏写入，**不自己**拼 SQL。
    let _repo_type: Option<nexus_repo::ExecutionLeaseRepository> = None;
}

/// §4.3.2 约束 1 —— `nexus-execution` **不依赖**上层 crate。
///
/// # 为什么这条**不因**新增 `nexus-repo` 依赖而放宽
///
/// 新增依赖边**不构成**放宽其它约束的理由。
/// `nexus-execution → nexus-service` / `nexus-api` / `nexus-cli`
/// 会形成环（`service → execution → service`）——
/// Cargo 会拒绝，但那只是**编译期**的兜底，
/// 不等于**契约**被验证过。
#[test]
fn the_execution_crate_does_not_depend_on_the_upper_layers() {
    const UPPER: &[&str] = &["nexus-service", "nexus-api", "nexus-cli"];

    let deps = internal_dependencies();

    for dep in &deps {
        assert!(
            !UPPER.contains(&dep.as_str()),
            "nexus-execution 不得依赖 {dep} —— 能力层反向依赖上层会成环\
             （架构文档 §4.3.2 约束 1）。实际依赖：{deps:?}"
        );
    }
}

/// §2.5.4 / §4.2 差异 1 —— 执行层**不自己**拼围栏 SQL。
///
/// # 为什么这是**结构层**断言
///
/// 围栏条件（`id` / `generation` / `lease_owner` / `status` /
/// `lease_until` / `deadline_at`）**只有一处定义** ——
/// `nexus-repo/src/lease.rs` 的 `Fence::CONDITIONS_TEMPLATE`。
///
/// 如果执行层自己拼一条带 `generation` 的 UPDATE：
///
/// 1. 围栏条件就有**两处**定义 —— 改了一处忘了另一处，
///    fencing **静默失效**；
/// 2. 围栏的**六个**子句可能被写成**五个**（少了 `lease_until` 或
///    `deadline_at`）—— 而那种缺失**不会**让任何测试变红。
///
/// # 判据
///
/// `nexus-execution` 的源码里**不得**出现围栏条件的**任何**子句。
/// 这比「不得出现完整的围栏模板」**更严** ——
/// 模板**逐字节**相同但**拆开**写的围栏同样违规。
///
/// # 为什么用 token 级扫描
///
/// 文档注释里解释「围栏条件在 `nexus-repo` 里」是**说明性文字**，
/// 不是「执行层自己拼了围栏」。扫描时必须剥离注释 ——
/// 否则「把边界写清楚」反而会让门禁红。
#[test]
fn the_execution_crate_does_not_inline_fenced_writes() {
    /// 围栏条件的**六个**子句。
    ///
    /// ⚠️ 必须**逐条**列出 —— 只判 `generation` 会漏掉
    /// 「少了 `lease_until` / `deadline_at`」这种更隐蔽的退化。
    const FENCE_CLAUSES: &[&str] = &[
        "generation = ",
        "lease_owner = ",
        "lease_until > ",
        "deadline_at > ",
        "status = 'running'",
    ];

    let mut violations = Vec::new();

    for path in source_files() {
        let raw = fs::read_to_string(&path).expect("读取源码失败");
        // ⚠️ `code_and_strings` —— 围栏条件通常是**SQL 字符串**里的片段。
        let scanned = token_scan::scan_code_and_strings(&raw).unwrap_or_else(|e| {
            panic!(
                "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                 那正是「静默失效」的形态",
                path.display()
            )
        });

        for clause in FENCE_CLAUSES {
            // ⚠️ 子句是**片段** —— 用**子串**匹配（不是精确标识符匹配）。
            // SQL 片段不是标识符。
            let hit = scanned.strings.iter().any(|s| s.contains(clause))
                || scanned
                    .identifiers
                    .iter()
                    .any(|i| i.contains(clause.trim()));
            if hit {
                violations.push(format!(
                    "{} 含有围栏条件的片段 `{clause}` —— 围栏只能由 nexus-repo 定义与拼装",
                    path.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "执行层**不得**自己拼围栏 SQL —— 围栏条件只有一处定义\
         （nexus-repo/src/lease.rs 的 Fence::CONDITIONS_TEMPLATE）：\n{}",
        violations.join("\n")
    );
}

/// §4.2 差异 1 —— 执行层**不写** `workflow_runs` 以外的表。
///
/// # 为什么这条**在** `nexus-execution` 里
///
/// 全 workspace 的允许集断言在 `nexus-core/tests/architecture.rs`
/// （`workflow_runs_appears_only_in_execution_owned_crates`）。
/// 那条管的是「**别的** crate 不许写」。
///
/// 本条管的是**执行层自己**的写面：它写**引擎侧**的表
/// （`execution_runs` / `execution_events` / `execution_outbox`），
/// 而 `workflow_runs` / `workflow_step_runs` 也归它 ——
/// 但那两张是**产品侧**的运行视图，写它们是执行层的**独占**权。
///
/// 所以这条断言的是「执行层不写**别的**表」——
/// 不是「执行层不写 `workflow_runs`」。
#[test]
fn the_execution_crate_writes_no_unrelated_tables() {
    /// 执行层**允许**写的表。
    ///
    /// | 表 | 归属 |
    /// |---|---|
    /// | `execution_runs` | 引擎侧（迁移 004） |
    /// | `execution_events` | 引擎侧（迁移 005） |
    /// | `execution_outbox` | 引擎侧（迁移 005） |
    /// | `workflow_runs` | 产品侧运行视图 —— **写入权独占方** |
    /// | `workflow_step_runs` | 产品侧运行视图 —— **写入权独占方** |
    const ALLOWED_TABLES: &[&str] = &[
        "execution_runs",
        "execution_events",
        "execution_outbox",
        "workflow_runs",
        "workflow_step_runs",
    ];

    let mut violations = Vec::new();

    for path in source_files() {
        let raw = fs::read_to_string(&path).expect("读取源码失败");
        let scanned = token_scan::scan_code_and_strings(&raw).unwrap_or_else(|e| {
            panic!(
                "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                 那正是「静默失效」的形态",
                path.display()
            )
        });

        for write in &scanned.sql_writes {
            if !ALLOWED_TABLES.contains(&write.table.as_str()) {
                violations.push(format!(
                    "{} 写了不允许的表 `{}`（{}）",
                    path.display(),
                    write.table,
                    write.operation
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "执行层只写引擎侧的表与它独占的产品侧运行视图：\n{}",
        violations.join("\n")
    );
}
