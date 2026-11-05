//! `nexus-agent` 的**结构层**测试（架构文档 §5.3）。
//!
//! # 为什么这些是「结构层」而不是「行为层」
//!
//! 行为层测试能验证「某段代码跑起来是什么结果」，
//! 但**验证不了「某段代码根本不该出现」**。
//!
//! 比如「`nexus-agent` 不用 `Box<dyn ChatModel>`」——
//! 那是一条**源码形状**的约束，只能扫描源码来断言。
//!
//! # 检验的设计条款
//!
//! | 断言 | 条款 |
//! |---|---|
//! | `nexus-agent` 不用 `Box<dyn ChatModel>` | §2.3.2 —— 泛型 + 静态分发 |
//! | `nexus-agent` 不依赖 `nexus-repo` / `nexus-service` / `nexus-api` | §4.1.1 依赖方向 + §4.4 规则 1 / 2 |
//! | `nexus-agent` 不引用 `nexus_llm::Gateway` | 参考架构 §2.2 —— 不依赖具体实现类型 |

use std::path::{Path, PathBuf};

/// crate 源码目录。
fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 递归收集 crate 内所有 `.rs` 文件。
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // 目录不存在 —— 不是错误，只是没有文件。
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `nexus-agent` 的全部源码文件。
fn source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files(&crate_root().join("src"), &mut files);
    files.sort();
    files
}

/// 剥掉**注释**后的源码行。
///
/// # 为什么必须剥注释
///
/// 架构文档 §5.3 的要求：**结构层扫描源码的断言必须剥离注释**。
///
/// 不剥的话，本文件自己的文档里讨论「`Box<dyn ChatModel>`」就会被误判 ——
/// 那是**说明性文字**，不是「真的用了 dyn」。
///
/// 剥离方法：删掉每行 `//` 之后的内容（覆盖 `//`、`///`、`//!` 三种前缀）。
fn code_lines(file: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(file).unwrap_or_default();
    content
        .lines()
        .map(|line| match line.find("//") {
            Some(idx) => line[..idx].to_string(),
            None => line.to_string(),
        })
        .collect()
}

/// §2.3.2 —— `nexus-agent` **不用** `Box<dyn ChatModel>`。
///
/// # 为什么这条成立而 `InvokableTool` 例外
///
/// 架构文档 §2.3.3 说 `dyn` 的**唯一**保留场景是「异构集合」。
///
/// - `InvokableTool` 的工具注册表装的是内置 / MCP / 用户自定义的工具 ——
///   **异构**，所以必须 `dyn`。
/// - `ChatModel` 的模型实现是「同质的多个实现」——
///   用泛型参数 `<M: ChatModel>` 就能单态化，**没有理由**付虚调用的代价。
///
/// 参考架构 §2.2 的性能提示更直接：
/// 「`nexus-agent` 内部用**泛型 + AFIT** 获得静态分发，
/// 只有在 `nexus-service` 装配层才 `dyn` 化」。
///
/// # 为什么扫源码而不是看类型
///
/// 「`ChatModel` 是不是 dyn」可以直接断言类型。
/// 但「整个 crate 里有没有人在别处偷偷用 `Box<dyn ChatModel>`」
/// 只能扫源码。
#[test]
fn the_agent_crate_does_not_use_dyn_dispatch_for_the_chat_model() {
    let files = source_files();
    assert!(!files.is_empty(), "应至少找到 `src/` 下的源文件");

    let mut violations = Vec::new();
    for file in &files {
        for (idx, line) in code_lines(file).iter().enumerate() {
            if line.contains("Box<dyn ChatModel") || line.contains("dyn ChatModel") {
                violations.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "§2.3.2 规定 `ChatModel` 用泛型 + 静态分发，但这些位置出现了 dyn：\n  {}",
        violations.join("\n  ")
    );
}

/// §4.1.1 / §4.4 规则 1、2 —— `nexus-agent` 不依赖上层与数据层。
///
/// `nexus-agent` 是**执行层**（架构文档 §4.1.1 的四层里）：
///
/// ```
/// 入口 → 协议 → 用例 → 执行 → 数据 → 基础
/// ```
///
/// 所以它**不得**依赖：
///
/// - `nexus-service` / `nexus-api` / `nexus-cli` —— 上层（§4.4 规则 2，能力层不得反向依赖）
/// - `nexus-repo` —— 数据层（§4.4 规则 5，能力层不得依赖 repo，否则成环）
///
/// 这条由 **Cargo 依赖图**保证 —— 不是靠约定，是靠编译。
#[test]
fn the_agent_crate_does_not_depend_on_the_upper_layers() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).unwrap();

    for forbidden in [
        "nexus-service",
        "nexus-api",
        "nexus-cli",
        "nexus-repo",
        "nexus-execution",
    ] {
        assert!(
            !manifest.contains(&format!("{forbidden}.workspace = true")),
            "§4.1.1 依赖方向 —— `nexus-agent` 不得依赖 {forbidden}"
        );
    }
}

/// 参考架构 §2.2 —— `nexus-agent` **不引用** `nexus-llm::Gateway` 的具体类型。
///
/// # 为什么这条重要
///
/// 参考架构 §2.2 的原文：
/// 「**不要**让 `nexus-agent::host` 依赖 `nexus-llm` 的具体 `Gateway` 类型」。
///
/// 决策 D4 的形态是「**依赖数据、依赖接口，不依赖实现**」：
/// 复用 `nexus-llm` 的**协议数据类型**（`ChatRequest` / `ChatResponse`）是允许的，
/// 但引用 `Gateway` 这个**具体实现类型**是禁止的。
///
/// # 为什么扫源码而不是看类型
///
/// 「`Gateway` 是不是出现在签名里」扫源码更直接 ——
/// 而且能抓到**测试代码**里的误用（测试里引用 `Gateway` 会让 mock 失效）。
#[test]
fn the_agent_crate_does_not_reference_the_concrete_gateway_type() {
    let files = source_files();
    assert!(!files.is_empty(), "应至少找到 `src/` 下的源文件");

    let mut violations = Vec::new();
    for file in &files {
        for (idx, line) in code_lines(file).iter().enumerate() {
            // 剥注释后，`Gateway` 只可能作为类型出现在代码里。
            if line.contains("Gateway") {
                violations.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "参考架构 §2.2 规定 `nexus-agent` 不得依赖 `nexus-llm` 的具体 `Gateway` 类型，\
         但这些位置出现了 Gateway：\n  {}",
        violations.join("\n  ")
    );
}

/// `nexus-agent` 的依赖方向符合 `scripts/check-deps.sh` 的声明。
///
/// 这条断言**不重复** `check-deps.sh` 的全局检查 ——
/// 它在 `nexus-agent` 自己的测试里再钉一道，让「改坏了」在
/// `cargo test -p nexus-agent` 时就能发现，而不必等 CI。
#[test]
fn the_agent_dependency_set_matches_the_declared_direction() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).unwrap();

    // `deps_of nexus-agent` 在 `scripts/check-deps.sh` 里声明为：
    // nexus-core nexus-config nexus-model nexus-llm nexus-tool
    // nexus-observability nexus-security
    for allowed in [
        "nexus-core",
        "nexus-config",
        "nexus-model",
        "nexus-llm",
        "nexus-tool",
        "nexus-observability",
        "nexus-security",
    ] {
        assert!(
            manifest.contains(&format!("{allowed}.workspace = true")),
            "应保留对 {allowed} 的依赖"
        );
    }
}

/// 决策 D4 —— `nexus-agent` **复用** `nexus-llm` 的协议数据类型。
///
/// 这是 D4 的**另一半**：上一条断言说「不依赖具体实现」，
/// 这一条说「**依赖数据**」。
///
/// 两者合起来才是 D4 的完整形态：
/// **依赖数据、依赖接口，不依赖实现**。
///
/// # 为什么这条值得断言
///
/// 如果有人「为了不依赖 `nexus-llm`」而**自建**一套 `Message` / `ToolDef`，
/// 那会造成两套语义漂移 —— 改一处忘另一处是**必然**的。
///
/// 这条断言守住「复用」这半边。
#[test]
fn the_agent_crate_reuses_the_llm_protocol_types() {
    let files = source_files();
    assert!(!files.is_empty(), "应至少找到 `src/` 下的源文件");

    let mut uses_llm_types = false;
    for file in &files {
        for line in code_lines(file) {
            if line.contains("nexus_llm::types::") {
                uses_llm_types = true;
            }
        }
    }

    assert!(
        uses_llm_types,
        "决策 D4 —— `nexus-agent` 应复用 `nexus_llm::types` 的协议数据类型，\
         而不是自建一套（自建会导致两套语义漂移）"
    );
}
