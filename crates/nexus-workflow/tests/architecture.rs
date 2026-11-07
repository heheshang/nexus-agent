//! `nexus-workflow` 的**结构层**断言。
//!
//! # 为什么这些断言不可省略
//!
//! 行为测试只能证明「**当前实现**产出了正确的结果」——
//! 它拦不住「有人加了一条 `sqlx::query!`，结果仍然对，但依赖方向已经坏了」。
//!
//! 本 crate 的**边界**是**已定稿**条款（架构文档 §6.3 第 5 项）：
//!
//! - `nexus-workflow` 是**能力层** —— **不得**依赖 `nexus-repo`（数据层）；
//! - `nexus-workflow` **不负责持久化** —— 源码里**不该**出现 `sqlx`。
//!
//! # 检验的设计条款
//!
//! | 测试 | 检验的条款 |
//! |---|---|
//! | `the_workflow_crate_does_not_depend_on_the_data_layer` | §4.1.2 能力层不得反向依赖数据层 |
//! | `the_workflow_crate_contains_no_database_access` | §4.4 规则 3（`sqlx` 只允许出现在三个 crate） |
//! | `the_workflow_crate_documents_its_responsibility_boundary` | AC #11（crate 级文档写明职责边界） |
//!
//! # 为什么用 **token 级扫描**而不是字符串查找
//!
//! 字符串查找会把**文档注释**里的 `sqlx` 一词也算进去 ——
//! 而 crate 级文档**恰恰要**说明「本 crate 不需要 `sqlx`」。
//!
//! 那样断言就会**恒红**（或被迫写成「注释里不算」的特殊逻辑）。
//! token 级扫描只认**标识符**与**路径** —— 注释里的词**不是**标识符。
//!
//! 这正是 NEX-48 建立的纪律：**源码扫描类断言必须剥离注释**。

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

/// `nexus-workflow` 的 `src` 目录。
fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 遍历 `src` 下的所有 `.rs` 文件。
fn source_files() -> Vec<PathBuf> {
    token_scan::walk_rs(&src_dir())
}

/// 读一个源文件的内容。
fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("读取源文件失败")
}

/// **§4.1.2** —— 能力层**不得**反向依赖数据层。
///
/// `nexus-workflow` 是图引擎（**能力层**），`nexus-repo` 是数据层。
/// 能力层依赖数据层就是**反向依赖** —— 它会让「纯内存的图编译」
/// 变成「必须有数据库才能编译图」。
///
/// ⚠️ 断言的是 **Cargo.toml 的依赖声明** ——
/// 那才是依赖方向的**唯一**来源（源码里的 `use` 只是它的结果）。
#[test]
fn the_workflow_crate_does_not_depend_on_the_data_layer() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let content = read(&manifest);

    for forbidden in ["nexus-repo", "nexus-execution", "nexus-infra"] {
        assert!(
            !content.contains(forbidden),
            "nexus-workflow 不得依赖 {forbidden} —— 能力层不得反向依赖数据层（§4.1.2）"
        );
    }
}

/// **§4.4 规则 3** —— `sqlx` 只允许出现在 `nexus-repo` / `nexus-infra` / `nexus-execution`。
///
/// `nexus-workflow` **不负责持久化** —— 源码里**不该**出现任何 `sqlx` 痕迹。
///
/// ⚠️ 用 **token 级扫描**（AST 级）—— 注释里的 `sqlx` 一词**不算**。
#[test]
fn the_workflow_crate_contains_no_database_access() {
    for path in source_files() {
        let source = read(&path);
        let scan = token_scan::scan_code(&source)
            .unwrap_or_else(|e| panic!("{} 解析失败：{e}", path.display()));

        assert!(
            !scan.has_parse_errors(),
            "{} 解析失败 —— 扫描结果不完整，断言会静默失效",
            path.display()
        );
        assert!(
            !scan.mentions("sqlx"),
            "{} 出现了 sqlx —— nexus-workflow 不负责持久化（§4.4 规则 3）",
            path.display()
        );
        assert!(
            !scan.mentions("FromRow"),
            "{} 出现了 FromRow —— 行类型属于 nexus-repo",
            path.display()
        );
    }
}

/// **AC #11** —— crate 级文档写明「本 crate 负责图引擎 / 不负责持久化」。
///
/// ⚠️ 这条断言看似「查字符串」，但它守的是一条**契约**：
/// 职责边界必须**写下来** —— 只在代码里成立、文档里不写的边界，
/// 下一个人会以为「顺手加个持久化也没事」。
#[test]
fn the_workflow_crate_documents_its_responsibility_boundary() {
    let lib = src_dir().join("lib.rs");
    let content = read(&lib);

    assert!(
        content.contains("图引擎"),
        "crate 级文档必须写明本 crate 负责图引擎"
    );
    assert!(
        content.contains("持久化"),
        "crate 级文档必须写明本 crate 不负责持久化"
    );
}
