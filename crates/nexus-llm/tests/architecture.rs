//! `nexus-llm` 的**结构层**测试（架构文档 §5.3）。
//!
//! # 为什么这些是「结构层」而不是「行为层」
//!
//! 行为层测试能验证「某段代码跑起来是什么结果」，
//! 但**验证不了「某段代码根本不该出现」**。
//!
//! 比如「`nexus-llm` 不用 `Box<dyn>`」——
//! 那是一条**源码形状**的约束，只能扫描源码来断言。
//!
//! # 检验的设计条款
//!
//! | 断言 | 条款 |
//! |---|---|
//! | `nexus-llm` 不含 `Box<dyn` | §2.3.2 —— enum 优于 dyn |
//! | `nexus-llm` 不依赖 `nexus-repo` | §4.2 差异 1 + 参考架构 §2.4（网关不查库） |

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

/// `nexus-llm` 的全部源码文件。
fn source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files(&crate_root().join("src"), &mut files);
    files.sort();
    files
}

/// §2.3.2 —— `nexus-llm` 不使用 `Box<dyn>`。
///
/// # 为什么这条对 `nexus-llm` 成立而对 `nexus-tool` 不成立
///
/// §2.3.3 说 `dyn` 的**唯一**保留场景是「异构集合」。
///
/// - `nexus-tool` 的 `ToolRegistry` 装的是内置 / MCP / 用户自定义的工具 ——
///   **异构**，所以必须 `dyn`（§2.3.3 的例子就是它）。
/// - `nexus-llm` 的供应商是「同质的多个实现」（都是 OpenAI 兼容协议）——
///   用 `enum` + `match` 装下就行，**没有理由**付虚调用的代价。
///
/// # 为什么扫源码而不是看类型
///
/// 「`ModelBackend` 是不是 enum」可以直接断言类型。
/// 但「整个 crate 里有没有人在别处偷偷用 `dyn`」只能扫源码。
///
/// 扫描时**跳过注释行**是必要的 —— 文档里讨论 `dyn` 是正常的。
/// 所以本断言用「`Box<dyn` 这个精确模式」而不是「`dyn` 这个词」，
/// 避免误伤文档段落。
#[test]
fn nexus_llm_uses_no_box_dyn_trait_objects() {
    let files = source_files();
    assert!(!files.is_empty(), "应至少找到 `src/` 下的源文件");

    let mut violations = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file).unwrap_or_default();
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            // 跳过注释行 —— 文档里讨论 `dyn` 是正常的。
            if trimmed.starts_with("//") {
                continue;
            }
            if line.contains("Box<dyn") {
                violations.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "§2.3.2 规定 `nexus-llm` 用 enum + match 而非 `Box<dyn>`，\
         但这些位置出现了 `Box<dyn`：\n  {}",
        violations.join("\n  ")
    );
}

/// §4.2 差异 1 / 参考架构 §2.4 —— **网关不查库**。
///
/// `nexus-llm` 的 `Cargo.toml` **不出现** `nexus-repo`。
///
/// 这条由 **Cargo 依赖图**保证 —— 不是靠约定，是靠编译。
///
/// # 为什么用「读 `Cargo.toml`」而不是「查依赖图」
///
/// 依赖图是**间接**证据（`nexus-llm → nexus-model → ...` 会不会绕一圈到 repo？）。
/// 直接读 manifest 是**直接**证据：它就是「这个 crate 声明了什么」。
///
/// `scripts/check-deps.sh` 从**另一个角度**覆盖同一条铁律 ——
/// 那是全局的依赖方向检查，本测试是 `nexus-llm` 视角的**局部**保险。
#[test]
fn nexus_llm_does_not_depend_on_nexus_repo() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).unwrap();

    assert!(
        !manifest.contains("nexus-repo"),
        "§4.2 差异 1 规定 `nexus-llm` 不依赖 `nexus-repo` —— 网关不查库，\
         描述符由调用方构造后传入"
    );
}

/// `nexus-llm` 的依赖方向符合 `scripts/check-deps.sh` 的声明。
///
/// 这条断言**不重复** `check-deps.sh` 的全局检查 ——
/// 它在 `nexus-llm` 自己的测试里再钉一道，让「改坏了」在
/// `cargo test -p nexus-llm` 时就能发现，而不必等 CI。
#[test]
fn nexus_llm_dependency_set_matches_the_declared_direction() {
    let manifest = std::fs::read_to_string(crate_root().join("Cargo.toml")).unwrap();

    // `deps_of nexus-llm` 在 `scripts/check-deps.sh` 里声明为：
    // nexus-core nexus-config nexus-model nexus-observability nexus-security
    for allowed in [
        "nexus-core",
        "nexus-config",
        "nexus-model",
        "nexus-observability",
        "nexus-security",
    ] {
        assert!(
            manifest.contains(&format!("{allowed}.workspace = true")),
            "应保留对 {allowed} 的依赖"
        );
    }

    // 反向依赖：能力层不得依赖上层。
    for forbidden in ["nexus-service", "nexus-api", "nexus-cli", "nexus-execution"] {
        assert!(
            !manifest.contains(&format!("{forbidden}.workspace = true")),
            "§4.4 规则 1 —— 能力层不得反向依赖上层（{forbidden}）"
        );
    }
}

/// 熔断器用 `std::sync::Mutex` 而**不是** `tokio::sync::Mutex`。
///
/// # 为什么这条值得断言
///
/// 参考架构文档 §3.1.3 明确要求 `std::sync::Mutex`：
///
/// - 熔断操作是**纯同步**的、不跨 await —— `tokio::sync::Mutex` 的获取是 async 的，
///   在热路径上是纯开销。
/// - 更重要的是 **`std::sync::MutexGuard` 不是 `Send`** ——
///   编译器会**强制**你不在持锁时 await（跨 await 是编译错误）。
///   `tokio::sync::MutexGuard` 反而**允许**跨 await —— 那是隐藏的 bug 来源：
///   锁在 await 期间被释放，另一个 task 拿到锁时状态已经变了。
///
/// 也就是说：用 `tokio::sync::Mutex` 不会立刻报错，但会**悄悄**失去这个保护。
/// 所以这条必须由**断言**钉住，而不是靠代码评审的自觉。
#[test]
fn the_breaker_uses_a_standard_mutex_not_a_tokio_mutex() {
    let breaker_source = std::fs::read_to_string(crate_root().join("src/breaker.rs")).unwrap();

    // ⚠️ 必须**剥离注释**再扫描 ——
    // 文档注释里提到 `tokio::sync::Mutex` 是**说明性文字**
    // （解释「为什么不用它」），不是实际使用。
    // `scripts/check-deps.sh` 规则 6b 用的是同样的剥离手法。
    let code: String = breaker_source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !code.contains("tokio::sync::Mutex"),
        "熔断器必须用 `std::sync::Mutex` —— 它的 guard 非 `Send`，\
         编译器会强制你不在持锁时 await（参考架构文档 §3.1.3）"
    );
    assert!(
        code.contains("use std::sync::{Arc, Mutex}") || code.contains("use std::sync::Mutex"),
        "熔断器应显式使用 `std::sync::Mutex`"
    );
}
