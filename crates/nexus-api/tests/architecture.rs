//! `nexus-api` 的**结构层**断言。
//!
//! # 为什么结构层断言不可省略
//!
//! 行为测试只能证明「**当前实现**产出了正确的结果」——
//! 它**拦不住**「有人换了个实现，结果仍然对，但契约已经变了」。
//!
//! 例如：把 SSE 输出从手写 `Body::from_stream` 换成 axum 的 `Sse`，
//! 现有的字节级测试**大部分仍会通过**（因为 axum 的 `Sse` 也能输出
//! `event:` / `data:` 行），但**字节输出不再与前端一致**。
//!
//! 结构层断言直接扫**源码**，把「不变量」钉在**实现形态**上。
//!
//! # 扫描源码时的纪律
//!
//! **必须剥离注释** —— 否则文档里提到 `axum::response::sse` 就会误报。
//! 这与 P1 定的「结构层扫描必须剥离注释」是同一条纪律。

use std::fs;
use std::path::{Path, PathBuf};

/// 找到 `nexus-api` 的 `src` 目录。
fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 读一个源码文件，**剥离注释**。
///
/// # 为什么必须剥离
///
/// 文档注释里提到 `axum::response::sse` 是**正常的**（说明「为什么不用它」），
/// 但那不该触发「源码里用了它」的告警。
fn read_stripped(path: &Path) -> String {
    let raw = fs::read_to_string(path).expect("读取源码失败");
    strip_comments(&raw)
}

/// 剥离行注释与块注释。
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_block = false;
    let mut in_string = false;
    let mut in_char = false;

    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            continue;
        }
        if in_char {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
                continue;
            }
            if c == '\'' {
                in_char = false;
            }
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => {
                // 行注释 —— 跳过到行尾
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                in_block = true;
            }
            '"' => {
                in_string = true;
                out.push(c);
            }
            '\'' => {
                in_char = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
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
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_api_crate_does_not_use_the_axum_sse_type() {
    // 决策 D8：SSE 输出**手写** `Body::from_stream`。
    //
    // axum 的 `Sse` 会自动转义 `data:` —— 与前端 `lib/sse.ts` 的
    // 字节解析不一致（参考架构 §1.5 已定稿）。
    let offenders: Vec<String> = source_files()
        .iter()
        .filter(|p| {
            let stripped = read_stripped(p);
            stripped.contains("axum::response::sse")
                || stripped.contains("response::sse::Sse")
                || stripped.contains("use axum::response::sse")
        })
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "决策 D8 要求手写 Body::from_stream，不得使用 axum 的 Sse 类型。违规文件：{offenders:?}"
    );
}

#[test]
fn the_api_crate_does_not_depend_on_the_data_layer() {
    // `nexus-api` 是**协议层** —— 它不查库。
    //
    // 架构文档 §1.4：协议层「不能感知业务规则、事务边界」。
    // 参考架构 §0.3 的 crate 职责表：`nexus-api` 不该做的事是「查库拼业务」。
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败");

    for forbidden in ["nexus-repo", "nexus-infra"] {
        assert!(
            !manifest.contains(forbidden),
            "nexus-api 不得依赖 {forbidden} —— 协议层不查库（架构文档 §1.4）"
        );
    }
}

#[test]
fn the_api_crate_adds_no_internal_dependencies() {
    // P5 **不新增**任何内部 crate 依赖方向（Spec §2.4）。
    //
    // `scripts/check-deps.sh` 规则 7 校验 `deps_of` ↔ `Cargo.toml` 一致性 ——
    // 那条规则会拦住「新增了依赖但没改声明表」。
    //
    // 本断言拦的是**另一件事**：在**不**改声明表的前提下偷偷加依赖 ——
    // 那样规则 7 会红，但红的原因（表不一致）**掩盖**了真正的问题（依赖方向变了）。
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败");

    let allowed = [
        "nexus-core",
        "nexus-config",
        "nexus-model",
        "nexus-observability",
        "nexus-security",
        "nexus-service",
        "nexus-agent",
        "nexus-execution",
        "nexus-llm",
    ];

    for line in manifest.lines() {
        let line = line.trim();
        // 只看形如 `nexus-xxx.workspace = true` 的行
        let Some((name, _)) = line.split_once(".workspace") else {
            continue;
        };
        let name = name.trim();
        if !name.starts_with("nexus-") {
            continue;
        }
        assert!(
            allowed.contains(&name),
            "P5 不新增内部 crate 依赖，发现未授权的依赖：{name}"
        );
    }
}

#[test]
fn the_api_crate_does_not_implement_jwt_verification() {
    // 决策 D10：鉴权的**机制**在 P5，**策略**在上层。
    //
    // `nexus-security` 是 7 行空壳 —— JWT 校验的具体形态**还没定**。
    // P5 若自己实现 JWT，就是**顺手决定**了一个还没定稿的设计。
    let offenders: Vec<String> = source_files()
        .iter()
        .filter(|p| {
            let stripped = read_stripped(p);
            stripped.contains("jsonwebtoken")
                || stripped.contains("JwtManager")
                || stripped.contains("decode_header")
        })
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "决策 D10：JWT 校验由上层经 trait 注入，nexus-api 不自行实现。违规文件：{offenders:?}"
    );
}
