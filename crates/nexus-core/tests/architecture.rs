//! 架构不变量的自动化守卫。
//!
//! 这些测试把**设计文档**里的约定变成了可执行的断言。
//! 设计文档：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`
//!
//! | 测试 | 检验的设计条款 |
//! |---|---|
//! | `nexus_core_不依赖任何内部_crate` | §4.4 规则 2 |
//! | `能力层不反向依赖_service_api_cli` | §4.4 规则 1 |
//! | `依赖图无环` | §4.1.2 |
//! | `sqlx_只出现在数据访问层` | §4.4 规则 3 |
//! | `nexus_model_不含数据库痕迹` | §4.2 差异 2 |
//! | `能力层不依赖_nexus_repo` | §4.1.2（防环） |
//!
//! 设计文档 §5.2 还要求一类**负向测试**（用 `trybuild` 断言某段代码不能编译），
//! 用于验证「取消语义靠类型保证」。那部分属于 P1（`nexus-core` 类型落地后）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
fn nexus_core_不依赖任何内部_crate() {
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
fn sqlx_只出现在数据访问层() {
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
fn nexus_model_不含数据库痕迹() {
    // 设计文档 §4.2 差异 2：nexus-model 只放纯 DTO，
    // 不带 FromRow / sqlx::Type —— 领域概念与数据库列布局分离。
    const FORBIDDEN: &[&str] = &["FromRow", "sqlx::", "Type> for", "sqlx_macros"];

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
        for pat in FORBIDDEN {
            if content.contains(pat) {
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
fn workspace_锁定文件存在且同步() {
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
