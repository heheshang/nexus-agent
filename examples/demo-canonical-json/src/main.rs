//! demo-canonical-json
//!
//! 验证设计文档 §3.5：规范化 JSON。
//!
//! 这是 §2.5.3「定义哈希稳定」的基础。如果同一份定义的 JSON 键序不同、
//! 空白不同就产生不同哈希，那么「检测运行中的定义是否被改过」就是假的 ——
//! 会一直误报或一直漏报。
//!
//! 规范化规则：
//! 1. 对象键按 UTF-8 字节序排序
//! 2. 数组**保持顺序**（顺序有语义）
//! 3. 数字用最短往返表示
//! 4. 字符串只做必要转义
//! 5. 丢弃 `null` 值（null 与缺失键等价）

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// 规范化错误。
#[derive(Debug, thiserror::Error)]
pub enum CanonicalizeError {
    #[error("数字无法规范化：{0}")]
    InvalidNumber(String),
}

/// 规范化 JSON 值。
///
/// 保证**语义相同**的值产生**完全相同的字节序列**。
pub fn canonicalize(value: &Value) -> Result<Vec<u8>, CanonicalizeError> {
    let mut out = Vec::with_capacity(256);
    write_canonical(value, &mut out)?;
    Ok(out)
}

/// 规范化并计算哈希。
pub fn canonical_hash(value: &Value) -> Result<[u8; 32], CanonicalizeError> {
    let bytes = canonicalize(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().into())
}

fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), CanonicalizeError> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => {
            // 用最短往返表示：serde_json 的 to_string 已保证往返一致
            let s = n.to_string();
            if s == "NaN" || s == "inf" || s == "-inf" {
                return Err(CanonicalizeError::InvalidNumber(s));
            }
            out.extend_from_slice(s.as_bytes());
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            // ① 对象键按 UTF-8 字节序排序
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push(b'{');
            let mut first = true;
            for k in keys {
                let v = &map[k];
                // ⑤ 丢弃 null 值
                if v.is_null() {
                    continue;
                }
                if !first {
                    out.push(b',');
                }
                first = false;
                write_string(k, out);
                out.push(b':');
                write_canonical(v, out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn write_string(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for ch in s.chars() {
        match ch {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

/// 构造一个「顺序不同但语义相同」的对象。
fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn main() {
    // ── 1. 键序无关 ──
    let a = obj(&[
        ("name", Value::from("wf")),
        ("version", Value::from(1)),
        (
            "nodes",
            Value::Array(vec![Value::from("a"), Value::from("b")]),
        ),
    ]);
    let b = obj(&[
        (
            "nodes",
            Value::Array(vec![Value::from("a"), Value::from("b")]),
        ),
        ("version", Value::from(1)),
        ("name", Value::from("wf")),
    ]);

    let ca = canonicalize(&a).unwrap();
    let cb = canonicalize(&b).unwrap();
    println!("a = {}", String::from_utf8_lossy(&ca));
    println!("b = {}", String::from_utf8_lossy(&cb));
    assert_eq!(ca, cb, "键序不同应产生相同的规范化结果");
    println!("✓ 键序无关\n");

    // ── 2. 数组顺序**有关** ──
    let c = obj(&[(
        "nodes",
        Value::Array(vec![Value::from("b"), Value::from("a")]),
    )]);
    assert_ne!(canonicalize(&a).unwrap(), canonicalize(&c).unwrap());
    println!("✓ 数组顺序敏感\n");

    // ── 3. null 被丢弃 ──
    let with_null = obj(&[("a", Value::from(1)), ("b", Value::Null)]);
    let without = obj(&[("a", Value::from(1))]);
    assert_eq!(
        canonicalize(&with_null).unwrap(),
        canonicalize(&without).unwrap()
    );
    println!("✓ null 值被丢弃\n");

    // ── 4. 哈希一致 ──
    let ha = canonical_hash(&a).unwrap();
    let hb = canonical_hash(&b).unwrap();
    assert_eq!(ha, hb);
    println!("✓ 哈希一致 = {}", hex(&ha));

    // ── 5. 语义变化 → 哈希变化 ──
    let changed = obj(&[
        ("name", Value::from("wf")),
        ("version", Value::from(2)),
        (
            "nodes",
            Value::Array(vec![Value::from("a"), Value::from("b")]),
        ),
    ]);
    assert_ne!(ha, canonical_hash(&changed).unwrap());
    println!("✓ 语义变化 → 哈希变化");
}

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_order_does_not_affect_canonical_result() {
        let a = obj(&[("x", Value::from(1)), ("y", Value::from(2))]);
        let b = obj(&[("y", Value::from(2)), ("x", Value::from(1))]);
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn nested_object_key_order_also_does_not_affect() {
        let a = obj(&[(
            "outer",
            obj(&[("p", Value::from(1)), ("q", Value::from(2))]),
        )]);
        let b = obj(&[(
            "outer",
            obj(&[("q", Value::from(2)), ("p", Value::from(1))]),
        )]);
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn array_order_affects_canonical_result() {
        let a = obj(&[("xs", Value::Array(vec![Value::from(1), Value::from(2)]))]);
        let b = obj(&[("xs", Value::Array(vec![Value::from(2), Value::from(1)]))]);
        assert_ne!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn null_值被丢弃() {
        let a = obj(&[("x", Value::from(1)), ("y", Value::Null)]);
        let b = obj(&[("x", Value::from(1))]);
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn hash_is_stable_for_semantically_equal_input() {
        let a = obj(&[("x", Value::from(1)), ("y", Value::from(2))]);
        let b = obj(&[("y", Value::from(2)), ("x", Value::from(1))]);
        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn semantic_change_changes_hash() {
        let a = obj(&[("x", Value::from(1))]);
        let b = obj(&[("x", Value::from(2))]);
        assert_ne!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn string_escaping_is_correct() {
        let v = Value::from("a\"b\\c\nd");
        let s = String::from_utf8(canonicalize(&v).unwrap()).unwrap();
        assert_eq!(s, r#""a\"b\\c\nd""#);
    }

    #[test]
    fn nested_null_is_also_dropped() {
        let a = obj(&[("outer", obj(&[("p", Value::from(1)), ("q", Value::Null)]))]);
        let b = obj(&[("outer", obj(&[("p", Value::from(1))]))]);
        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }
}
