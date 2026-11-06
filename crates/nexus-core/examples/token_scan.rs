//! **token 级**源码扫描器 —— 命令行入口。
//!
//! # 为什么需要它
//!
//! `scripts/check-specs.sh` 与 `scripts/check-deps.sh` 需要扫描源码里的
//! **错误码使用**与**数据库痕迹**。
//!
//! 旧做法是 `sed -e 's://.*::'` 剥掉注释后 `grep` ——
//! 而 `sed` 的替换是**纯文本**的，**不理解字符串**：
//!
//! ```text
//! let u = "http://x";   ← sed 把这行 `//` 之后的部分全删了
//! ```
//!
//! 结果是「**同一行**里出现过 `//`（哪怕在字符串里）的真实代码，
//! **完全不进门禁的视野**」。
//!
//! # ⚠️ 为什么是 `examples/` 而不是 `src/bin/`
//!
//! 这个工具依赖 `syn` —— 而 `syn` 是 **dev-dependency**
//! （架构文档 §4.4 规则 2：基础层不得引入重依赖）。
//!
//! `examples/` 里的目标**可以**用 dev-dependency，
//! 而 `src/bin/` 里的**生产二进制**不行。
//!
//! 它是**工具**，不是**产品**的一部分 —— 放在 `examples/` 语义上也更对。
//!
//! # 形态
//!
//! ```text
//! cargo run -p nexus-core --example token_scan -- <子命令> [参数...]
//! ```
//!
//! | 子命令 | 作用 |
//! |---|---|
//! | `error-codes <目录>...` | 列出源码里用到的 `ErrorCode::` 变体（**一个**一行） |
//! | `db-traces <目录>...` | 列出源码里的数据库痕迹标识符（**一个**一行） |
//! | `mentions <词> <目录>...` | 列出**提到**某个词的源码文件（**一个**一行） |
//!
//! # ⚠️ 输出契约
//!
//! - **一个发现一行** —— shell 侧可以逐行处理，不需要解析格式。
//! - **解析失败 → 退出码非零** —— 绝**不**把语法错误的文件当成空文件扫
//!   （那会让坏掉的源文件悄悄通过门禁 —— 又一次「看起来覆盖了，实际没有」）。
//! - **没有发现 → 空输出 + 退出码 0**。

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 这个工具与 `tests/` **共用**同一份实现 ——
// 那是**有意**的：门禁脚本用的扫描器与结构层断言用的扫描器
// **必须**是**同一份** —— 否则两份实现会**各自漂移**，
// 而「结构层断言绿、门禁红」或反之，都很难查。
#[path = "../tests/support/token_scan.rs"]
mod token_scan;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("用法：token_scan <error-codes|db-traces|mentions> [参数...]");
        return ExitCode::from(2);
    };

    let result = match command.as_str() {
        "error-codes" => error_codes(rest),
        "db-traces" => db_traces(rest),
        "mentions" => mentions(rest),
        other => {
            eprintln!("未知子命令：{other}");
            eprintln!("用法：token_scan <error-codes|db-traces|mentions> [参数...]");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// 收集**数据库痕迹**的标识符。
const DB_TRACE_IDENTIFIERS: &[&str] = &["FromRow", "sqlx", "sqlx_macros"];

/// `error-codes` —— 列出源码里用到的 `ErrorCode::` 变体。
///
/// # ⚠️ 判据是**类型限定**的路径
///
/// `ErrorCode::Forbidden` 是一条 `syn::Path`。
/// 只匹配「`ErrorCode` 这个标识符出现过」会**过宽** ——
/// 注释里提到 `ErrorCode` 也算。
///
/// # ⚠️ `ErrorCode` **不必是第一段**
///
/// 真实代码里的形态是**全限定路径**：
///
/// ```text
/// nexus_core::error::ErrorCode::Forbidden
/// ```
///
/// 只认「第一段是 `ErrorCode`」会**漏掉**这种形态 ——
///
/// 而那正是**生产代码**里最常见的写法。
///
/// 所以判据是「**某一段**是 `ErrorCode`，且它**后面**还有一段」——
/// 那一段就是**变体名**。
///
/// ⚠️ 顺带排除了 `ErrorCode::from_code(..)` 这类**关联函数** ——
/// 它不是**变体**。判据是「`ErrorCode` 后面那段**首字母大写**」
/// （Rust 的变体命名约定），
/// 而 `from_code` 是小写开头的函数名。
fn error_codes(dirs: &[String]) -> Result<Vec<String>, String> {
    let mut found = BTreeSet::new();
    for dir in dirs {
        let scanned = scan(Path::new(dir), true)?;
        for path in &scanned.paths {
            let segments: Vec<&str> = path.split("::").collect();
            for (index, segment) in segments.iter().enumerate() {
                if *segment != "ErrorCode" {
                    continue;
                }
                let Some(variant) = segments.get(index + 1) else {
                    // `ErrorCode` 自身（没有变体）—— 跳过。
                    continue;
                };
                // ⚠️ 变体名**首字母大写** —— 排除 `from_code` 等关联函数。
                if !variant.starts_with(|c: char| c.is_uppercase()) {
                    continue;
                }
                found.insert((*variant).to_string());
            }
        }
    }
    Ok(found.into_iter().collect())
}

/// `db-traces` —— 列出源码里的数据库痕迹。
fn db_traces(dirs: &[String]) -> Result<Vec<String>, String> {
    let mut found = BTreeSet::new();
    for dir in dirs {
        let scanned = scan(Path::new(dir), false)?;
        for identifier in &scanned.identifiers {
            if DB_TRACE_IDENTIFIERS.contains(&identifier.as_str()) {
                found.insert(identifier.clone());
            }
        }
    }
    Ok(found.into_iter().collect())
}

/// `mentions` —— 列出提到某个词的源码文件。
fn mentions(args: &[String]) -> Result<Vec<String>, String> {
    let Some((term, dirs)) = args.split_first() else {
        return Err("用法：token_scan mentions <词> <目录>...".to_string());
    };
    let mut found = Vec::new();
    for dir in dirs {
        let dir_path = Path::new(dir);
        for file in token_scan::walk_rs(dir_path) {
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            // ⚠️ 同时扫**代码**与**字符串** —— 「提到」这个词
            // 既可能是标识符，也可能是 SQL 字符串里的表名。
            let Ok(scanned) = token_scan::scan_code_and_strings(&source) else {
                return Err(format!(
                    "解析 {} 失败 —— 解析失败会让扫描结果**不完整**，\
                     那正是「静默失效」的形态",
                    file.display()
                ));
            };
            if scanned.mentions(term) {
                found.push(file.display().to_string());
            }
        }
    }
    Ok(found)
}

/// 扫描目录，**报告**解析失败。
fn scan(dir: &Path, collect_strings: bool) -> Result<token_scan::ScanResult, String> {
    let scanned = token_scan::scan_dir(dir, collect_strings);
    if scanned.has_parse_errors() {
        let details: Vec<String> = scanned
            .parse_errors
            .iter()
            .map(|(path, err)| format!("  {}：{err}", path.display()))
            .collect();
        return Err(format!(
            "扫描 {} 时有 {} 个文件解析失败：\n{}",
            dir.display(),
            scanned.parse_errors.len(),
            details.join("\n")
        ));
    }
    Ok(scanned)
}
