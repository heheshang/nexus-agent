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

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 为什么用 `#[path]` 而不是抽一个 crate
//
// 抽 crate 会**新增**一条依赖边，而本仓库的依赖方向是**已定稿**条款
// （架构文档 §6.3 第 5 项）。`#[path]` include 只需要**文件系统**上的相对路径，
// **不产生任何依赖边** —— `Cargo.toml` 完全不动。
//
// `nexus-core` 是依赖图的**根**（零内部依赖），
// 由它**提供**共用代码是最安全的方向。
#[path = "../../nexus-core/tests/support/token_scan.rs"]
mod token_scan;

/// 找到 `nexus-api` 的 `src` 目录。
fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 扫描一个源码文件，返回**标识符**集合。
///
/// # 为什么用 token 级扫描而不是「剥注释后搜文本」
///
/// 「剥注释后搜文本」需要自己实现 Rust 的词法 ——
/// 而 Rust 的词法形态很多（raw string / 字符字面量 / 生命周期 / 嵌套块注释），
/// 手写必然漏。实测本仓库曾有三份**手抄**的词法器，
/// 每一份都有各自的失效方式（漏报或假阳性）。
///
/// token 级扫描让「注释不是代码」这件事**自动成立** ——
///
/// 详见 `crates/nexus-core/tests/support/token_scan.rs` 的模块文档。
fn scan_identifiers(path: &Path) -> token_scan::ScanResult {
    let raw = fs::read_to_string(path).expect("读取源码失败");
    token_scan::scan_code(&raw).unwrap_or_else(|e| {
        panic!(
            "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
             那正是「静默失效」的形态（不得静默跳过）",
            path.display()
        )
    })
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
    //
    // ⚠️ 判据是**完整路径** `axum::response::sse` —— 不是「`axum` 和 `sse`
    // 都出现过」（那**太宽松**：本 crate 合法地用了大量 `axum::` 路径）。
    let offenders: Vec<String> = source_files()
        .iter()
        .filter(|p| {
            let scanned = scan_identifiers(p);
            scanned.paths.contains("axum::response::sse")
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
    //
    // ⚠️ **每一条允许的依赖都必须标注条款依据** ——
    // 架构文档 §4.1 的 crate 图是**目标态**，
    // 「允许」不等于「需要」。
    // 没有条款依据的依赖不得放行（那等于 allowlist 变成了「什么都行」）。
    //
    // | 依赖 | 条款依据 |
    // |---|---|
    // | `nexus-core` | §4.4 规则 2（`nexus-core` 是依赖图的根） |
    // | `nexus-observability` | §4.1（协议 → 指标） |
    // | `nexus-agent` | §4.1.1 的 2026-11-05 修正（协议层可借只读类型） |
    // | `nexus-service` | §4.1 crate 图 `API --> SVC`（业务编排归属） |
    // | `nexus-execution` | §4.1.1 的 2026-11-05 修正（协议层可借只读类型） |
    let allowed = [
        "nexus-core",
        "nexus-observability",
        "nexus-service",
        "nexus-agent",
        "nexus-execution",
    ];

    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败");

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
fn the_api_crate_declares_no_zero_reference_internal_dependencies() {
    // ⚠️ **零引用的依赖**是**依赖噪音** —— 它让 crate 图失真，
    // 而架构测试 `the_api_crate_does_not_use_the_service_crate`
    // 只扫 `src/` 的标识符，**不查 manifest**。
    //
    // 依赖已在、闸门只是没检查 —— 真正的违规要等到有人**写了**引用才会红。
    // 那时已经晚了一步。
    //
    // ⚠️ **例外**：`nexus-service` / `nexus-execution` 是
    // §4.1 crate 图**预留**的接缝（P6 接线用），当前 `src/` 里零引用是**预期**的。
    // 那两条在 §4.1 / §4.1.1 有**条款依据**，所以豁免。
    const ZERO_REFERENCE_EXEMPT: &[&str] = &[
        // §4.1 crate 图 `API --> SVC` —— 业务编排归属 P6 接线。
        "nexus-service",
        // §4.1.1 的 2026-11-05 修正 —— 协议层可借执行层的只读类型。
        "nexus-execution",
    ];

    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("读取 Cargo.toml 失败");

    let mut declared = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        let Some((name, _)) = line.split_once(".workspace") else {
            continue;
        };
        let name = name.trim();
        if name.starts_with("nexus-") {
            declared.push(name.to_string());
        }
    }

    // 扫 `src/` 里真正被引用的内部 crate。
    let mut referenced: Vec<String> = Vec::new();
    for path in source_files() {
        let raw = fs::read_to_string(&path).expect("读取源码失败");
        let scanned = token_scan::scan_code(&raw).unwrap_or_else(|e| {
            panic!(
                "解析 {} 失败：{e} —— 解析失败会让扫描结果**不完整**，\
                 那正是「静默失效」的形态",
                path.display()
            )
        });
        for crate_name in &declared {
            // Rust 里 crate 名用下划线：`nexus-core` → `nexus_core`。
            let ident = crate_name.replace('-', "_");
            if scanned
                .paths
                .iter()
                .any(|p| p == &ident || p.starts_with(&format!("{ident}::")))
            {
                referenced.push(crate_name.clone());
            }
        }
    }

    let mut violations = Vec::new();
    for name in &declared {
        if referenced.contains(name) {
            continue;
        }
        if ZERO_REFERENCE_EXEMPT.contains(&name.as_str()) {
            continue;
        }
        violations.push(name.clone());
    }

    assert!(
        violations.is_empty(),
        "nexus-api 不得声明**零引用**的内部依赖（它们让 crate 图失真）：{violations:?}\n\
         ⚠️ 若该依赖是架构文档预留的接缝，请加进 ZERO_REFERENCE_EXEMPT 并注明条款依据。"
    );
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
            let scanned = scan_identifiers(p);
            scanned.identifiers.contains("jsonwebtoken")
                || scanned.identifiers.contains("JwtManager")
                || scanned.identifiers.contains("decode_header")
        })
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "决策 D10：JWT 校验由上层经 trait 注入，nexus-api 不自行实现。违规文件：{offenders:?}"
    );
}

#[test]
fn the_api_crate_does_not_use_the_service_crate() {
    // AC #6：`nexus-api` **不引用** `nexus-service` 的**具体类型**。
    //
    // 同 P4 的 D4 纪律 —— `nexus-api` 依赖的是 **trait**（消费方定义 trait），
    // 不是 `nexus-service` 的具体实现。
    //
    // 为什么这条重要：`nexus-service` 是 7 行空壳，且 P5 的边界是
    // **传输层**。一旦 `nexus-api` 引用了它的具体类型，
    // P5 就**阻塞于** P6 的业务编排 —— 决策 D9 会被静默推翻。
    //
    // ⚠️ 只扫 `src/`（不含 `tests/`）——
    // 测试里用 `nexus-service` 造假数据是允许的。
    let offenders: Vec<String> = source_files()
        .iter()
        .filter(|p| {
            let scanned = scan_identifiers(p);
            // ⚠️ 判据是**完整路径** `nexus_service::` ——
            // 「`nexus_service` 这个词出现过」太宽松
            // （变量名、注释里的词都会命中）。
            scanned
                .paths
                .iter()
                .any(|path| path == "nexus_service" || path.starts_with("nexus_service::"))
        })
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "AC #6：nexus-api 不得引用 nexus-service 的具体类型，只依赖 trait 接缝。违规文件：{offenders:?}"
    );
}

#[test]
fn the_bridge_does_not_carry_a_heartbeat_timer() {
    // AC #9：心跳是**路由**的职责 —— 桥接**不自带**定时器（spec §3.2）。
    //
    // 为什么这条要单独钉住：把心跳做成桥接的**可配置项**
    // 会让桥接承担**两条**职责（事件 → 帧 + 保活），
    // 而心跳频率实际上由**路由**的读超时配置决定。
    //
    // ⚠️ 桥接里**允许**出现 `ping` —— 它只是帧的**字节内容**。
    // 被禁止的是**定时器**（`interval` / `sleep` / `tokio::time`）。
    let offenders: Vec<String> = source_files()
        .iter()
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name != "bridge.rs" {
                return false;
            }
            let scanned = scan_identifiers(p);
            // ⚠️ `sleep` / `interval` 按**函数调用**判定 ——
            // 「源码里出现过 `sleep` 这个词」太宽松。
            scanned.calls.contains("sleep")
                || scanned.calls.contains("interval")
                || scanned.paths.contains("tokio::time")
        })
        .map(|p| p.display().to_string())
        .collect();

    assert!(
        offenders.is_empty(),
        "心跳是路由的职责 —— 桥接不得自带定时器（spec §3.2）。违规文件：{offenders:?}"
    );
}
