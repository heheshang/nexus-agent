//! 架构不变量的自动化守卫。
//!
//! 这些测试把**设计文档**里的约定变成了可执行的断言。
//! 设计文档：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`
//!
//! | 测试 | 检验的设计条款 |
//! |---|---|
//! | `nexus_core_depends_on_no_internal_crate` | §4.4 规则 2 |
//! | `capability_layer_does_not_depend_on_service_api_cli` | §4.4 规则 1 |
//! | `dependency_graph_is_acyclic` | §4.1.2 |
//! | `sqlx_appears_only_in_data_access_layers` | §4.4 规则 3 |
//! | `nexus_model_has_no_database_traces` | §4.2 差异 2 |
//! | `capability_layer_does_not_depend_on_nexus_repo` | §4.1.2（防环） |
//! | `nexus_repo_depends_only_on_capability_layer` | §4.1.2（正向：repo → 能力层） |
//! | `nexus_repo_does_not_touch_workflow_runs` | §4.2 差异 1（负向：`nexus-repo` 不许**写**） |
//! | `workflow_runs_appears_only_in_execution_owned_crates` | §4.2 差异 1（**写操作**归属：全 workspace） |
//! | `nexus_repo_does_not_depend_on_the_execution_layer` | §4.4 规则 4（反向依赖禁止） |
//!
//! # ⚠️ 「写入权唯一」的判定手段（决策 D12 · 选项 B，2026-11-06 修订）
//!
//! 原判定是「**类型**归属」—— `nexus-repo` 不得引用 `workflow_runs` 的**类型**。
//! 依赖方向改为 `execution → repo` 后，那个手段**不再等价**于「写入权唯一」：
//! `nexus-repo` 的类型对所有依赖它的 crate **可见**，
//! 「名字不出现在 `nexus-repo`」**推不出**「别的 crate 写不了」。
//!
//! 判定手段改为「**写操作**归属」——
//! 全 workspace 扫描 `workflow_runs` / `workflow_step_runs` 的
//! `INSERT` / `UPDATE` / `DELETE`，写操作**只允许**出现在 `nexus-execution`；
//! `nexus-infra` 允许**建表 DDL**（迁移），不是访问。
//!
//! 强度**不降** —— 前者可被「把类型下沉到 `nexus-repo`」绕过，
//! 后者管住**任何 crate 的任何写操作**。
//!
//! 设计文档 §5.2 还要求一类**负向测试**（用 `trybuild` 断言某段代码不能编译），
//! 用于验证「取消语义靠类型保证」。那部分属于 P1（`nexus-core` 类型落地后）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 为什么用 `#[path]` 而不是抽一个 crate
//
// 抽 crate 会**新增**一条依赖边，而本仓库的依赖方向是**已定稿**条款
// （架构文档 §6.3 第 5 项）。`#[path]` include 只需要**文件系统**上的相对路径，
// **不产生任何依赖边** —— `Cargo.toml` 完全不动。
#[path = "support/token_scan.rs"]
mod token_scan;

/// workspace 根目录。
fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/nexus-core，上溯两级是 workspace 根
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("无法定位 workspace 根目录")
        .to_path_buf()
}

/// 解析 crate 的 Cargo.toml，返回 `(内部依赖, [dependencies] 段的原始行)`。
///
/// 只看 `[dependencies]` 段，忽略 `[package]` / `[[bin]]` / `[lints]` 等其他段。
fn parse_manifest(crate_name: &str) -> (Vec<String>, Vec<String>) {
    let manifest = workspace_root()
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    let content = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("读取 {} 失败：{e}", manifest.display()));

    let mut internal = Vec::new();
    let mut dep_lines = Vec::new();
    let mut in_deps = false;

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 段切换
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps {
            continue;
        }
        dep_lines.push(line.to_string());
        // 形如 `nexus-xxx.workspace = true`
        if let Some(name) = line.strip_suffix(".workspace = true")
            && name.starts_with("nexus-")
        {
            internal.push(name.to_string());
        }
    }

    (internal, dep_lines)
}

/// 读取某个 crate 声明的内部依赖。
fn internal_deps(crate_name: &str) -> Vec<String> {
    parse_manifest(crate_name).0
}

/// 列出所有 crate 名。
fn all_crates() -> Vec<String> {
    let crates_dir = workspace_root().join("crates");
    let mut names: Vec<String> = std::fs::read_dir(&crates_dir)
        .expect("无法读取 crates 目录")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn nexus_core_depends_on_no_internal_crate() {
    // 设计文档 §4.4 规则 2：nexus-core 是依赖图的根
    assert!(
        internal_deps("nexus-core").is_empty(),
        "nexus-core 必须是最底层，不得依赖任何内部 crate，实际依赖：{:?}",
        internal_deps("nexus-core")
    );
}

#[test]
fn capability_layer_does_not_depend_on_service_api_cli() {
    // 设计文档 §4.4 规则 1
    let upper = ["nexus-service", "nexus-api", "nexus-cli"];

    for crate_name in all_crates() {
        // 上层 crate 之间互相依赖是允许的
        if upper.contains(&crate_name.as_str()) {
            continue;
        }
        for dep in internal_deps(&crate_name) {
            assert!(
                !upper.contains(&dep.as_str()),
                "{crate_name} -> {dep}：能力层不得反向依赖 service/api/cli（会形成环）"
            );
        }
    }
}

#[test]
fn capability_layer_does_not_depend_on_nexus_repo() {
    // 设计文档 §4.1.2：能力层绝不能反向依赖数据层，
    // 否则 `repo → rag` 的单向依赖会被打破，形成真正的环。
    const CAPABILITY: &[&str] = &[
        "nexus-llm",
        "nexus-tool",
        "nexus-rag",
        "nexus-memory",
        "nexus-capacity",
        "nexus-ingestion",
        "nexus-vector",
        "nexus-security",
        "nexus-config",
        "nexus-observability",
        "nexus-model",
    ];

    for crate_name in CAPABILITY {
        if !workspace_root().join("crates").join(crate_name).exists() {
            continue;
        }
        for dep in internal_deps(crate_name) {
            assert_ne!(
                dep, "nexus-repo",
                "{crate_name} 不得依赖 nexus-repo —— 依赖方向必须是 repo → 能力层"
            );
        }
    }
}

/// §4.1.2 —— `nexus-repo` **只**依赖能力层（单向依赖的正向侧）。
///
/// # 与 [`capability_layer_does_not_depend_on_nexus_repo`] 的关系
///
/// 那条断言的是**反向**：能力层不得依赖 `nexus-repo`。
/// 本条断言的是**正向**：`nexus-repo` 的内部依赖**全部**落在能力层里。
///
/// # 为什么必须断言「正向」
///
/// 只断言反向的话，`nexus-repo` 依赖 `nexus-service`（上层）也能通过 ——
/// 那会形成 `service → repo → service` 的环。
///
/// `scripts/check-deps.sh` 规则 2 已经覆盖了「能力层不得依赖 service/api/cli」，
/// 但它检查的是**能力层**那侧；`nexus-repo` 自己依赖上层**没有任何脚本覆盖**。
///
/// # 允许的两个例外
///
/// | crate | 为什么允许 |
/// |---|---|
/// | `nexus-core` | 依赖图的根 —— 所有 crate 都依赖它 |
/// | `nexus-infra` | 数据访问层的基础设施（PG / Redis / S3 / Milvus）—— 同层，不是上层 |
///
/// `nexus-infra` **不是**能力层，但它也不是「上层」——
/// 它是数据访问层，与 `nexus-repo` 平级。把它排掉会让这条断言
/// 拒绝一个**正确**的依赖。
#[test]
fn nexus_repo_depends_only_on_capability_layer() {
    /// 能力层 + 依赖图的根 + 同层的数据访问基础设施。
    const ALLOWED: &[&str] = &[
        // 依赖图的根
        "nexus-core",
        // 能力层
        "nexus-config",
        "nexus-model",
        "nexus-observability",
        "nexus-security",
        "nexus-vector",
        "nexus-llm",
        "nexus-tool",
        "nexus-rag",
        "nexus-ingestion",
        "nexus-capacity",
        "nexus-memory",
        // 同层的数据访问基础设施
        "nexus-infra",
    ];

    for dep in internal_deps("nexus-repo") {
        assert!(
            ALLOWED.contains(&dep.as_str()),
            "nexus-repo → {dep}：nexus-repo 只允许依赖能力层。\
             依赖上层（service / api / cli / agent / workflow / execution / mcp / a2a）\
             会形成环 —— 依赖方向必须是 repo → 能力层。"
        );
    }
}

#[test]
fn dependency_graph_is_acyclic() {
    // 设计文档 §4.1.2：Kahn 算法做拓扑排序。若无法排完，说明存在环。
    let crates: Vec<String> = all_crates();

    // dependents[X] = 依赖 X 的 crate 集合（X 的「后继」）
    let mut dependents: HashMap<String, HashSet<String>> = HashMap::new();
    for crate_name in &crates {
        for dep in internal_deps(crate_name) {
            dependents
                .entry(dep)
                .or_default()
                .insert(crate_name.clone());
        }
    }

    // indegree[X] = X 有多少个依赖（必须先排完依赖才能排 X）
    let mut indegree: HashMap<&str, usize> = crates
        .iter()
        .map(|n| (n.as_str(), internal_deps(n).len()))
        .collect();

    let mut queue: Vec<&str> = crates
        .iter()
        .map(|s| s.as_str())
        .filter(|n| indegree[n] == 0)
        .collect();

    let mut visited = 0usize;
    while let Some(node) = queue.pop() {
        visited += 1;
        if let Some(children) = dependents.get(node) {
            for child in children {
                let d = indegree.get_mut(child.as_str()).expect("节点应存在");
                *d -= 1;
                if *d == 0 {
                    queue.push(child.as_str());
                }
            }
        }
    }

    assert_eq!(
        visited,
        crates.len(),
        "依赖图存在环：只有 {visited}/{} 个 crate 可被拓扑排序",
        crates.len()
    );
}

#[test]
fn sqlx_appears_only_in_data_access_layers() {
    // 设计文档 §4.4 规则 3
    let allowed = ["nexus-infra", "nexus-repo", "nexus-execution"];
    for crate_name in all_crates() {
        let (_, dep_lines) = parse_manifest(&crate_name);
        let has_sqlx = dep_lines
            .iter()
            .any(|l| l.starts_with("sqlx") && l.contains("workspace = true"));
        if has_sqlx {
            assert!(
                allowed.contains(&crate_name.as_str()),
                "{crate_name} 不应直接依赖 sqlx —— 数据访问必须收敛到 infra/repo/execution"
            );
        }
    }
}

#[test]
fn nexus_model_has_no_database_traces() {
    // 设计文档 §4.2 差异 2：nexus-model 只放纯 DTO，
    // 不带 FromRow / sqlx::Type —— 领域概念与数据库列布局分离。
    //
    // ⚠️ 用 **token 级**扫描（[`token_scan`]）——
    // 文档注释里提到 `FromRow` 是**说明性文字**，不是数据库痕迹。
    // 旧的手写词法器**没有字符字面量状态**，字符 `'"'` 会让注释**不再被剥**。
    //
    // ⚠️ 判据是**标识符精确匹配** ——
    // 「`sqlx` 这个**词**出现过」，而不是「源码里有 `sqlx::` 这个**字符串**」。
    // 后者对 `sqlx::` 的写法很敏感（`use sqlx::Type` 与 `sqlx::Type` 都行，
    // 但 `sqlx ::Type` 就不行）—— 那是**文本形态**的脆弱性。
    const FORBIDDEN: &[&str] = &["FromRow", "sqlx", "sqlx_macros"];

    let src_dir = workspace_root()
        .join("crates")
        .join("nexus-model")
        .join("src");
    if !src_dir.exists() {
        return;
    }

    let mut violations = Vec::new();
    for file in walk_rs(&src_dir) {
        let content = std::fs::read_to_string(&file).expect("读取源文件失败");
        let scanned = token_scan::scan_code(&content).unwrap_or_else(|e| {
            panic!(
                "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                 那正是「静默失效」的形态",
                file.display()
            )
        });
        for pat in FORBIDDEN {
            if scanned.identifiers.contains(*pat) {
                violations.push(format!("{} 含数据库痕迹：{pat}", file.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "nexus-model 必须保持纯 DTO：\n{}",
        violations.join("\n")
    );
}

/// §4.2 差异 1 —— `workflow_runs` / `workflow_step_runs` 的**写操作**
/// **只允许**出现在 `nexus-execution`。
///
/// # 为什么判定手段是「**写操作**归属」而不是「**类型**归属」
///
/// 依赖方向改为 `execution → repo`（决策 D12 · 选项 B）之后，
/// `nexus-repo` 的类型对**所有依赖它的 crate**（`nexus-service` / `nexus-api`）
/// **可见** —— 于是「`nexus-repo` 不持有该类型」**推不出**
/// 「别的 crate 写不了」。
///
/// 原判定（`nexus-repo` 不得引用 `workflow_runs` 的**类型**）
/// 可被「把类型下沉到 `nexus-repo`」绕过 —— 那时别的 crate 照样能写。
///
/// 改为「**写操作**只允许出现在某处」后，管住的是
/// **任何 crate 的任何写操作** —— 强度**不降**。
///
/// # 判据
///
/// 全 workspace 扫描**生产代码**（`src/`）：
/// 对 `workflow_runs` / `workflow_step_runs` 的
/// `INSERT` / `UPDATE` / `DELETE`，写操作**只允许**出现在 `nexus-execution`。
///
/// # 为什么**不扫描** `tests/`
///
/// 架构测试**自己**必须提到表名 —— 断言的允许集里就写着
/// `"workflow_runs"`。扫描 `tests/` 会让这条测试**把自己判成违规**。
///
/// 约束的对象是**生产代码** —— `src/` 里出现写操作才意味着
/// 「有 crate 在访问它」。`tests/` 里出现只意味着「有测试在断言它」。
///
/// # 为什么允许 `nexus-infra`
///
/// `nexus-infra` 持有**迁移文件** —— `workflow_runs` 的表结构必须在
/// 迁移里定义（否则表根本建不出来）。那是**建表 DDL**，不是**访问**。
///
/// ⚠️ 关键区分：`CREATE TABLE workflow_runs (...)` **不是写操作** ——
/// 扫描器只认 `INSERT` / `UPDATE` / `DELETE`。建表是**结构**定义，
/// 不是**数据**写入。
///
/// # 为什么允许 `nexus-execution`
///
/// 架构文档 §4.2 差异 1 明文规定：`workflow_runs` / `workflow_step_runs`
/// 的**写入权**由 `nexus-execution` 独占。
#[test]
fn workflow_runs_appears_only_in_execution_owned_crates() {
    /// 允许**写** `workflow_runs` / `workflow_step_runs` 的 crate。
    const WRITE_OWNER: &str = "nexus-execution";

    /// 被独占的表名。
    const EXCLUSIVE_TABLES: &[&str] = &["workflow_runs", "workflow_step_runs"];

    let crates_dir = workspace_root().join("crates");
    let mut violations = Vec::new();

    for crate_name in all_crates() {
        // 独占方**自己**写 —— 允许。
        if crate_name == WRITE_OWNER {
            continue;
        }

        let src_dir = crates_dir.join(&crate_name).join("src");
        if !src_dir.exists() {
            continue;
        }

        for file in walk_rs(&src_dir) {
            let content = std::fs::read_to_string(&file).expect("读取源文件失败");
            // ⚠️ 用 `code_and_strings` —— 表名**既**可能是标识符，
            // **也**可能是 SQL 字符串里的表名（`"SELECT * FROM workflow_runs"`）。
            // 只扫标识符会**漏掉**后者，而那**正是**要防的。
            let scanned = token_scan::scan_code_and_strings(&content).unwrap_or_else(|e| {
                panic!(
                    "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                     那正是「静默失效」的形态",
                    file.display()
                )
            });
            for table in EXCLUSIVE_TABLES {
                // ⚠️ 判据是「**写**过这张表」，不是「提到过这张表」。
                // 「提到」会把 `SELECT` 也算进去 —— 而**读**不构成写入。
                if scanned.writes_table(table) {
                    violations.push(format!(
                        "{}（crate `{crate_name}`）**写**了 `{table}`",
                        file.display()
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "`workflow_runs` / `workflow_step_runs` 的**写操作**只允许出现在 \
         `{WRITE_OWNER}`（写入权独占方）：\n{}",
        violations.join("\n")
    );
}

/// §4.2 差异 1 —— `nexus-repo` **不得**写 `workflow_runs`。
///
/// # 与 [`workflow_runs_appears_only_in_execution_owned_crates`] 的关系
///
/// 那条是**全 workspace** 的允许集断言；本条是针对 `nexus-repo` 的
/// **负向**断言 —— 它给出**更具体**的失败信息：
/// 「`nexus-repo` 写了产品侧运行表」正是最可能的违规形态。
///
/// # 为什么在依赖方向改为 `execution → repo` 之后**仍然**要这条
///
/// 依赖方向改了，但「`workflow_runs` 的写入权归执行层」这条**契约**没改。
/// `nexus-repo` 持有引擎侧持久化能力（租约 / fencing / 幂等提交）——
/// 那让它「**有能力**写 `workflow_runs`」变成了现实风险，
/// 所以这条断言从「不得**引用**」收紧为「不得**写**」。
#[test]
fn nexus_repo_does_not_touch_workflow_runs() {
    let src_dir = workspace_root()
        .join("crates")
        .join("nexus-repo")
        .join("src");
    if !src_dir.exists() {
        return;
    }

    let mut violations = Vec::new();
    for file in walk_rs(&src_dir) {
        let content = std::fs::read_to_string(&file).expect("读取源文件失败");
        let scanned = token_scan::scan_code_and_strings(&content).unwrap_or_else(|e| {
            panic!(
                "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                 那正是「静默失效」的形态",
                file.display()
            )
        });
        for table in ["workflow_runs", "workflow_step_runs"] {
            if scanned.writes_table(table) {
                violations.push(format!("{} **写**了 {table}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "workflow_runs 由 nexus-execution 独占 —— nexus-repo 不得写它：\n{}",
        violations.join("\n")
    );
}

/// §4.4 规则 4 —— `nexus-repo` **不得**依赖 `nexus-execution`。
///
/// # 为什么这条必须存在
///
/// 决策 D12（选项 B）确立了 `nexus-execution → nexus-repo` 的**单向**依赖。
/// 反向依赖会成环（`execution → repo → execution`）——
/// Cargo 在**编译期**就会拒绝，所以它不会「悄悄跑起来」。
///
/// 但这条断言**不是**多此一举：
///
/// 1. 它给出的**失败信息**比 Cargo 的环报错**可读得多**；
/// 2. 它把「单向」这条**契约**钉在**测试**里，
///    而不只是依赖 Cargo 的隐式保证 ——
///    「Cargo 会拒绝」不等于「架构文档 §4.4 规则 4 被验证过」；
/// 3. `dependency_graph_is_acyclic` 判的是「**无环**」，
///    判不出「**方向对**」—— 一个 `repo → execution → repo` 的环
///    同样会让它红，但**不是**本条要说的那件事。
#[test]
fn nexus_repo_does_not_depend_on_the_execution_layer() {
    let deps = internal_deps("nexus-repo");

    assert!(
        !deps.iter().any(|d| d == "nexus-execution"),
        "nexus-repo 不得依赖 nexus-execution —— 反向会成环\
         （架构文档 §4.4 规则 4：execution → repo 允许，repo → execution 禁止）。\
         实际依赖：{deps:?}"
    );
}

#[test]
fn all_crates_declare_workspace_lints() {
    for crate_name in all_crates() {
        let manifest = workspace_root()
            .join("crates")
            .join(&crate_name)
            .join("Cargo.toml");
        let content = std::fs::read_to_string(&manifest).expect("读取 manifest 失败");
        assert!(
            content.contains("[lints]") && content.contains("workspace = true"),
            "{crate_name} 必须继承 workspace.lints"
        );
    }
}

#[test]
fn all_crates_must_not_hardcode_dependency_versions() {
    for crate_name in all_crates() {
        let (_, dep_lines) = parse_manifest(&crate_name);
        for line in dep_lines {
            // 合法写法只有 `name.workspace = true`
            assert!(
                line.ends_with(".workspace = true"),
                "{crate_name}：依赖必须写成 `name.workspace = true`，实际：{line}"
            );
        }
    }
}

#[test]
fn workspace_lockfile_exists_and_is_in_sync() {
    let root = workspace_root();
    let lock = root.join("Cargo.lock");
    assert!(lock.exists(), "Cargo.lock 必须入库");

    // Cargo.lock 必须包含全部成员 crate
    let content = std::fs::read_to_string(&lock).expect("读取 Cargo.lock 失败");
    for crate_name in all_crates() {
        assert!(
            content.contains(&format!("name = \"{crate_name}\"")),
            "Cargo.lock 缺少成员 crate：{crate_name}"
        );
    }
}

#[test]
fn every_crate_has_lib_or_bin_entry() {
    for crate_name in all_crates() {
        let dir = workspace_root()
            .join("crates")
            .join(&crate_name)
            .join("src");
        let has_lib = dir.join("lib.rs").exists();
        let has_bin = dir.join("main.rs").exists() || dir.join("bin").is_dir();
        assert!(
            has_lib || has_bin,
            "{crate_name} 必须有 src/lib.rs 或 src/main.rs"
        );
    }
}

/// 递归收集目录下的 `.rs` 文件。
fn walk_rs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk_rs(&p));
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out
}
