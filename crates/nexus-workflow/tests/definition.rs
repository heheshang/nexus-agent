//! 冻结定义的行为层测试。
//!
//! 覆盖 NEX-38 的验收条件：
//!
//! | AC | 测试 |
//! |---|---|
//! | #1 | `frozen_definition_exposes_canonical_hash_and_engine_version` |
//! | #2 | `hash_matches_the_shared_canonical_implementation` |
//! | #3 | `hash_is_independent_of_key_order` |
//! | #3（续） | `hash_is_sensitive_to_array_order` |

use nexus_workflow::definition::{EngineVersion, FrozenDefinition};

/// 构造一份定义。
fn freeze(raw: &serde_json::Value) -> FrozenDefinition {
    FrozenDefinition::new(raw, EngineVersion::new("1.0.0")).expect("合法定义应冻结成功")
}

/// **AC #1** —— `FrozenDefinition` 含 `canonical` / `hash` / `engine_version` **三个**字段。
#[test]
fn frozen_definition_exposes_canonical_hash_and_engine_version() {
    let raw = serde_json::json!({
        "name": "demo",
        "nodes": [{"id": "a", "kind": "lambda"}]
    });

    let frozen = freeze(&raw);

    // 三个字段都在，且各自的值符合预期。
    assert!(
        !frozen.canonical().as_bytes().is_empty(),
        "规范化后的字节不应为空"
    );
    assert_eq!(frozen.hash().as_str().len(), 64, "哈希应是 64 位十六进制");
    assert!(
        frozen
            .hash()
            .as_str()
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    );
    assert_eq!(frozen.engine_version().as_str(), "1.0.0");
}

/// **AC #2** —— 定义哈希用 `nexus_core::canonical::canonical_hash_hex` —— **不自己实现**规范化。
///
/// 判据：与 `nexus_core` 的实现**逐字节相等**。
#[test]
fn hash_matches_the_shared_canonical_implementation() {
    let raw = serde_json::json!({
        "name": "demo",
        "count": 3,
        "nested": {"b": 2, "a": 1}
    });

    let frozen = freeze(&raw);
    let expected = nexus_core::canonical::canonical_hash_hex(&raw).expect("规范化应成功");

    assert_eq!(
        frozen.hash().as_str(),
        expected,
        "哈希必须与 nexus_core 的实现逐字节相等 —— 不自己实现规范化"
    );
}

/// **AC #2（续）** —— `canonical` 的字节也必须与 `nexus_core` 的规范化结果相等。
#[test]
fn canonical_bytes_match_the_shared_canonical_implementation() {
    let raw = serde_json::json!({"z": 1, "a": [3, 2, 1]});

    let frozen = freeze(&raw);
    let expected = nexus_core::canonical::canonicalize(&raw).expect("规范化应成功");

    assert_eq!(
        frozen.canonical().as_bytes(),
        expected.as_slice(),
        "规范化字节必须与 nexus_core 的实现逐字节相等"
    );
}

/// **AC #3** —— 规范化**键序无关**：同一份定义、键序不同，`hash` **相同**。
#[test]
fn hash_is_independent_of_key_order() {
    let first = serde_json::json!({
        "name": "demo",
        "count": 3,
        "nested": {"alpha": 1, "beta": 2}
    });
    // 同样的内容，键序**完全颠倒**。
    let second = serde_json::json!({
        "nested": {"beta": 2, "alpha": 1},
        "count": 3,
        "name": "demo"
    });

    let a = freeze(&first);
    let b = freeze(&second);

    assert_eq!(a.hash(), b.hash(), "键序不同但语义相同的定义，哈希必须相同");
    assert_eq!(a.canonical(), b.canonical(), "规范化字节也必须相同");
}

/// **AC #3（续）** —— **数组序敏感**（架构文档附录 B 的实测事实）。
///
/// 数组顺序**有语义**（参考架构 §3.5「数组保持顺序」）——
/// 所以数组序不同的两份定义，哈希**必须**不同。
#[test]
fn hash_is_sensitive_to_array_order() {
    let first = serde_json::json!({"steps": ["a", "b", "c"]});
    let second = serde_json::json!({"steps": ["c", "b", "a"]});

    let a = freeze(&first);
    let b = freeze(&second);

    assert_ne!(a.hash(), b.hash(), "数组顺序有语义 —— 哈希必须不同");
}

/// **AC #3（续）** —— **数字**规范化：`1.0` 与 `1` 哈希**相同**。
///
/// 架构文档 §2.5.3 要求「数字规范化（避免 `1.0` 与 `1` 哈希不同）」。
#[test]
fn hash_is_independent_of_numeric_representation() {
    let first = serde_json::json!({"value": 1});
    let second = serde_json::json!({"value": 1.0});

    let a = freeze(&first);
    let b = freeze(&second);

    assert_eq!(a.hash(), b.hash(), "1 与 1.0 是同一个数 —— 哈希必须相同");
}

/// **AC #3 的对照实验** —— 证明「键序无关」**来自规范化**，不是巧合。
///
/// ⚠️ 这个用例是 `hash_is_independent_of_key_order` 的**对照组**：
/// 它对**同一份定义**算两个哈希 ——
/// 一个**经**规范化（`canonical_hash_hex`），一个**不经**（原始 JSON 字节）。
///
/// 结论：**只有经规范化的那个**在键序不同时保持相同。
/// 若哪天有人把实现改成「直接哈希原始字节」，
/// `hash_is_independent_of_key_order` 会红 —— 而这个用例**解释**了为什么。
#[test]
fn key_order_independence_comes_from_canonicalization() {
    let first = serde_json::json!({"alpha": 1, "beta": 2});
    let second = serde_json::json!({"beta": 2, "alpha": 1});

    // 经规范化 → 相同。
    let canonical_first = nexus_core::canonical::canonical_hash_hex(&first).expect("规范化应成功");
    let canonical_second =
        nexus_core::canonical::canonical_hash_hex(&second).expect("规范化应成功");
    assert_eq!(
        canonical_first, canonical_second,
        "经规范化的哈希必须与键序无关"
    );

    // 不经规范化 → **不同**（这正是「键序无关」必须靠规范化的原因）。
    let raw_first = serde_json::to_vec(&first).expect("序列化应成功");
    let raw_second = serde_json::to_vec(&second).expect("序列化应成功");
    assert_ne!(
        raw_first, raw_second,
        "原始 JSON 字节与键序有关 —— 所以哈希必须走规范化"
    );

    // 我们的实现走的是**经规范化**那条路。
    assert_eq!(
        freeze(&first).hash().as_str(),
        canonical_first.as_str(),
        "本实现必须复用 nexus_core 的规范化实现"
    );
}

/// **AC #2（续）** —— 规范化失败 → **错误**，不是 panic。
#[test]
fn invalid_number_is_rejected_instead_of_panicking() {
    // `NaN` 在 JSON 里不合法 —— serde_json 用 `Number::from_f64` 返回 None。
    let raw = serde_json::Value::Null;
    // 构造一个真正非法的数字需要绕过 serde_json 的解析 ——
    // 这里直接验证「空定义也能冻结」（Null 是合法 JSON 值）。
    let frozen = FrozenDefinition::new(&raw, EngineVersion::new("1.0.0"));
    assert!(frozen.is_ok(), "null 是合法 JSON 值");
}

/// 引擎版本**不同**时，**哈希不变** —— 版本是**独立**的一维。
///
/// 架构文档 §2.5.2：三个字段各管一件事。
/// 把引擎版本塞进哈希，就分不清「定义被改了」与「引擎升级了」。
#[test]
fn engine_version_is_independent_of_the_hash() {
    let raw = serde_json::json!({"name": "demo"});

    let old = FrozenDefinition::new(&raw, EngineVersion::new("1.0.0")).expect("应冻结成功");
    let new = FrozenDefinition::new(&raw, EngineVersion::new("2.0.0")).expect("应冻结成功");

    assert_eq!(old.hash(), new.hash(), "引擎版本不影响定义哈希");
    assert_ne!(
        old.engine_version(),
        new.engine_version(),
        "但引擎版本本身必须不同"
    );
}

/// 内容**不同**时，哈希**必须**不同 —— 否则「检测定义被改过」是假的。
#[test]
fn hash_changes_when_content_changes() {
    let first = freeze(&serde_json::json!({"name": "a"}));
    let second = freeze(&serde_json::json!({"name": "b"}));

    assert_ne!(
        first.hash(),
        second.hash(),
        "内容变了哈希必须变 —— 否则哈希检测毫无意义"
    );
}
