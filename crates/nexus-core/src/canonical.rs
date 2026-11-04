//! 规范化 JSON —— **定义哈希稳定**的基础。
//!
//! 架构文档 §3.5：「规范化是 §2.5.3 的依赖：一个**独立、可测、无业务含义**的子系统。」
//!
//! # 为什么需要它
//!
//! JSON 的键序、浮点数表示、Unicode 转义方式都会影响哈希。
//! 同一份定义的 JSON 键序不同、空白不同，就产生不同哈希 ——
//! 那么「检测运行中的定义是否被改过」这个功能**就是假的**：
//! 它会一直误报或一直漏报。
//!
//! # 五条规则（§3.5）
//!
//! 1. 对象键按 **UTF-8 字节序**排序（不是插入序）。
//! 2. 数组**保持顺序**（顺序有语义）。
//! 3. 数字用**最短往返**表示（`1.0` → `1`，`1e2` → `100`）。
//! 4. 字符串只做**必要的**转义（`"` `\` 控制字符）。
//! 5. **丢弃 `null`** —— null 与缺失键等价。
//!
//! # 为什么放在 `nexus-core`
//!
//! `nexus-core` 是依赖图的**根**（§4.4 规则 2，零内部依赖）——
//! 所以**所有** crate 都能依赖它，不会有环。
//!
//! 另两个候选位置都不可行：
//!
//! - `nexus-repo` —— `nexus-workflow` 需要它，但 `workflow` **不能**依赖 `repo`
//!   （能力层不得反向依赖数据层，§4.1.2）。
//! - `nexus-workflow` —— `nexus-repo` 需要它，但 `repo` 依赖 `workflow` 就成环
//!   （`workflow → execution → repo → workflow`）。
//!
//! 规范化是**无状态纯函数** —— 放在 `nexus-core` 符合它「纯类型 + 无状态工具」的定位。
//!
//! # 与 `demo-canonical-json` 的关系
//!
//! `examples/demo-canonical-json` 是**设计验证** demo，
//! 本模块是**生产实现** —— 两者是**两份独立的验证**
//! （与 P1 的 `demo-cancel-typestate` / `nexus-core` 关系一致）。

use serde_json::Value;
use sha2::{Digest, Sha256};

/// 规范化错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalizeError {
    /// 数字无法用有限的十进制表示（如 `NaN` / `inf`）。
    #[error("数字无法规范化：{0}")]
    InvalidNumber(String),
}

/// 规范化 JSON 值 —— 保证**语义相同**的值产生**完全相同的字节序列**。
///
/// # 错误
///
/// 数字为 `NaN` / `inf` → [`CanonicalizeError::InvalidNumber`]。
///
/// # 为什么 `NaN` 是错误而不是被丢弃
///
/// `NaN` 在 JSON 里**本就不合法**。把它规范化成 `"NaN"` 会让哈希
/// 稳定但**语义错误** —— 两份不同的非法输入可能产生同一个哈希。
/// 报错比「假装能处理」安全。
pub fn canonicalize(value: &Value) -> Result<Vec<u8>, CanonicalizeError> {
    let mut out = Vec::with_capacity(256);
    write_canonical(value, &mut out)?;
    Ok(out)
}

/// 规范化并计算 **SHA-256** 哈希。
///
/// # 为什么是 SHA-256
///
/// 参考实现用 `crypto/sha256`（参考架构文档的 crate 对照表：
/// `crypto/sha256` → `sha2` crate）。
///
/// # 为什么是 32 字节
///
/// `definition_hash` 列是 `CHAR(64)` —— 64 位十六进制 = 32 字节。
///
/// # 错误
///
/// 同 [`canonicalize`]。
pub fn canonical_hash(value: &Value) -> Result<[u8; 32], CanonicalizeError> {
    let bytes = canonicalize(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().into())
}

/// 把哈希渲染成 **64 位小写十六进制**（`CHAR(64)` 列的形状）。
///
/// # 为什么不在 `canonical_hash` 里直接返回字符串
///
/// 返回**字节**让调用方自己决定编码 ——
/// 有的地方要字节（写二进制），有的地方要十六进制（写 `CHAR(64)` 列）。
/// 把编码决策塞进哈希函数，就多了一种「同一个哈希有两种表示」的坑。
///
/// # 错误
///
/// 数字为 `NaN` / `inf` → [`CanonicalizeError::InvalidNumber`]。
pub fn canonical_hash_hex(value: &Value) -> Result<String, CanonicalizeError> {
    let hash = canonical_hash(value)?;
    Ok(hex(&hash))
}

/// 字节数组 → 小写十六进制。
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // `write!` 对 String 不会失败 —— 写字符串的返回值是 Result 但恒为 Ok。
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 递归写出规范化形式。
fn write_canonical(value: &Value, out: &mut Vec<u8>) -> Result<(), CanonicalizeError> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => {
            let s = canonical_number(n)?;
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
            // ① 对象键按 UTF-8 字节序排序。
            //
            // `sort_unstable` 对 `&String` 比较的是**字节序** ——
            // Rust 的 `str` 序就是 UTF-8 字节序，符合要求。
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();

            out.push(b'{');
            let mut first = true;
            for k in keys {
                let v = &map[k];

                // ⑤ 丢弃 `null` 值 —— null 与缺失键等价。
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

/// 数字的**最短往返**表示。
///
/// # 三类数字，三种处理
///
/// | 类别 | 处理 | 例子 |
/// |---|---|---|
/// | 整数（`i64` / `u64`） | 直接输出十进制 | `1` → `1`，`100` → `100` |
/// | 浮点且**值恰为整数** | 输出**整数**形式 | `1.0` → `1`，`1e2` → `100` |
/// | 其他浮点 | Rust `Display`（最短往返） | `0.1` → `0.1` |
///
/// # 为什么「值恰为整数」要输出整数形式
///
/// `serde_json` 会把 `1.0` 存成浮点，`1` 存成整数。
/// 如果只按「存的是什么类型」输出，`1.0` 与 `1` 的规范化结果就**不同** ——
/// 而它们在语义上是**同一个数**。
///
/// 实测（`serde_json` 1.x）：
/// ```text
/// "1.0" → is_i64=false  is_f64=true
/// "1"   → is_i64=true   is_f64=false
/// "1.0" == "1" ? false          ← serde_json 认为它们不等
/// ```
///
/// 所以规范化必须**自己**做「浮点整数化」——
/// 这正是 §3.5 规则 3「数字用最短往返表示（`1.0` → `1`，`1e2` → `100`）」的含义。
///
/// # 为什么浮点用 Rust 的 `Display`
///
/// Rust 的 `f64` `Display` 就是**最短往返**表示 ——
/// 解析回来得到**同一个** `f64`。这正是我们要的语义。
fn canonical_number(n: &serde_json::Number) -> Result<String, CanonicalizeError> {
    // 整数路径：直接十进制。
    if let Some(i) = n.as_i64() {
        return Ok(i.to_string());
    }
    if let Some(u) = n.as_u64() {
        return Ok(u.to_string());
    }

    let f = n
        .as_f64()
        .ok_or_else(|| CanonicalizeError::InvalidNumber(format!("{n} 既不是整数也不是浮点数")))?;

    if !f.is_finite() {
        return Err(CanonicalizeError::InvalidNumber(f.to_string()));
    }

    // 浮点且恰为整数 → 输出整数形式。
    //
    // `fract() == 0.0` 判定「值是整数」。
    // 范围检查：超出 i64/u64 范围的整数（如 1e30）不能转成整数类型 ——
    // 那种情况保留浮点表示（`1e30` 的 Display 是 `1000000000000000000000000000000`）。
    if f.fract() == 0.0 && f.is_finite() {
        if f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            return Ok((f as i64).to_string());
        }
        if f > 0.0 && f <= u64::MAX as f64 {
            return Ok((f as u64).to_string());
        }
    }

    // 其他浮点：用 `serde_json` 自己的最短往返表示。
    //
    // ⚠️ 为什么**不用** Rust 的 `f64` `Display`：
    // Rust 的 Display **从不使用指数形式** ——
    // `1.7976931348623157e308` 会被写成 309 位十进制数字。
    // 那个字符串**解析不回来**（serde_json 报 "number out of range"）。
    //
    // `serde_json` 内部用 ryu，能正确产出指数形式。
    // 我们借用它 —— 不重复造轮子，也不承担「自己写的不对」的风险。
    //
    // 注意 `serde_json::Number::from_f64` 对非有限值返回 `None`，
    // 上面的 `is_finite` 检查已经排除了那种情况。
    let n = serde_json::Number::from_f64(f)
        .ok_or_else(|| CanonicalizeError::InvalidNumber(f.to_string()))?;

    Ok(n.to_string())
}

/// 写出字符串的规范化形式。
///
/// # 转义规则（§3.5 规则 4）
///
/// 只转义**必要的**：
///
/// - `"` → `\"`
/// - `\` → `\\`
/// - 控制字符（`U+0000`–`U+001F`）→ `\n` `\r` `\t` 或 `\uXXXX`
/// - 其余字符**原样**输出 UTF-8 字节
///
/// # 为什么不做 Unicode 转义归一
///
/// §2.5.3 规则 3：「字符串按 UTF-8 原始字节，**不做转义差异归一**」。
///
/// 也就是说 `"\u4e2d"`（转义形式）与 `"中"`（字面形式）在 JSON 里
/// **语义相同**，但规范化后**不同**。
///
/// 这看起来违反「语义相同 → 规范化相同」，但 §2.5.3 **明文规定**如此 ——
/// 理由是 Unicode 转义归一化有**规范歧义**（哪些等价形式算「相同」？
/// NFC 还是 NFKC？），不同实现会得出不同结果，
/// 那会让「哈希稳定」变成「哈希依赖实现」。
///
/// 保守地按**原始字节**处理，是唯一能保证**跨实现一致**的选择。
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 构造一个「顺序不同但语义相同」的对象。
    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut m = serde_json::Map::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), v.clone());
        }
        Value::Object(m)
    }

    #[test]
    fn object_key_order_does_not_affect_canonical_result() {
        let a = obj(&[("x", json!(1)), ("y", json!(2))]);
        let b = obj(&[("y", json!(2)), ("x", json!(1))]);

        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn nested_object_key_order_also_does_not_affect() {
        let a = obj(&[("outer", obj(&[("p", json!(1)), ("q", json!(2))]))]);
        let b = obj(&[("outer", obj(&[("q", json!(2)), ("p", json!(1))]))]);

        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn array_order_affects_canonical_result() {
        // 数组顺序**有语义** —— 必须保持。
        let a = obj(&[("xs", json!([1, 2]))]);
        let b = obj(&[("xs", json!([2, 1]))]);

        assert_ne!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn null_values_are_discarded() {
        let a = obj(&[("x", json!(1)), ("y", Value::Null)]);
        let b = obj(&[("x", json!(1))]);

        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn nested_null_is_also_dropped() {
        let a = obj(&[("outer", obj(&[("p", json!(1)), ("q", Value::Null)]))]);
        let b = obj(&[("outer", obj(&[("p", json!(1))]))]);

        assert_eq!(canonicalize(&a).unwrap(), canonicalize(&b).unwrap());
    }

    #[test]
    fn hash_is_stable_for_semantically_equal_input() {
        let a = obj(&[("x", json!(1)), ("y", json!(2))]);
        let b = obj(&[("y", json!(2)), ("x", json!(1))]);

        assert_eq!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn semantic_change_changes_hash() {
        let a = obj(&[("x", json!(1))]);
        let b = obj(&[("x", json!(2))]);

        assert_ne!(canonical_hash(&a).unwrap(), canonical_hash(&b).unwrap());
    }

    #[test]
    fn string_escaping_is_correct() {
        let v = json!("a\"b\\c\nd");
        let s = String::from_utf8(canonicalize(&v).unwrap()).unwrap();

        assert_eq!(s, r#""a\"b\\c\nd""#);
    }

    #[test]
    fn integer_valued_floats_normalize_to_integer_form() {
        // ⚠️ 这是 §3.5 规则 3 的**核心**用例。
        //
        // `serde_json` 认为 `1.0` 与 `1` 是**不同的值**
        // （实测：`"1.0" == "1"` 为 false）。
        // 但规范化必须让它们**产生相同的字节** ——
        // 否则同一份定义的两种写法会得到不同的哈希。
        let as_float = serde_json::from_str::<Value>("1.0").unwrap();
        let as_int = serde_json::from_str::<Value>("1").unwrap();

        assert_ne!(as_float, as_int, "serde_json 认为它们不等（前提）");
        assert_eq!(
            canonicalize(&as_float).unwrap(),
            canonicalize(&as_int).unwrap(),
            "整数值的浮点必须规范化成整数形式"
        );
    }

    #[test]
    fn exponent_notation_normalizes_to_plain_integer() {
        let as_exp = serde_json::from_str::<Value>("1e2").unwrap();
        let as_int = serde_json::from_str::<Value>("100").unwrap();

        assert_eq!(
            canonicalize(&as_exp).unwrap(),
            canonicalize(&as_int).unwrap()
        );
    }

    #[test]
    fn non_integer_floats_keep_their_fractional_form() {
        let v = json!(0.5);
        let s = String::from_utf8(canonicalize(&v).unwrap()).unwrap();

        assert_eq!(s, "0.5");
    }

    #[test]
    fn float_precision_round_trips() {
        // 最短往返：解析回来必须是**同一个** f64。
        for raw in ["0.1", "0.3333333333333333", "1.7976931348623157e308", "2.5"] {
            let v = serde_json::from_str::<Value>(raw).unwrap();
            let canonical = String::from_utf8(canonicalize(&v).unwrap()).unwrap();
            let reparsed: Value = serde_json::from_str(&canonical).unwrap();

            assert_eq!(v, reparsed, "{raw} 规范化后往返不一致：{canonical}");
        }
    }

    #[test]
    fn hash_hex_is_64_lowercase_hex_characters() {
        let hex = canonical_hash_hex(&json!({"a": 1})).unwrap();

        assert_eq!(hex.len(), 64, "应为 64 位十六进制：{hex}");
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "应为小写十六进制：{hex}"
        );
    }

    #[test]
    fn hash_hex_matches_the_byte_hash() {
        let v = json!({"a": 1});
        let bytes = canonical_hash(&v).unwrap();
        let hex = canonical_hash_hex(&v).unwrap();

        assert_eq!(hex, super::hex(&bytes));
    }

    #[test]
    fn empty_object_and_empty_array_are_distinct() {
        assert_ne!(
            canonicalize(&json!({})).unwrap(),
            canonicalize(&json!([])).unwrap()
        );
    }

    #[test]
    fn control_characters_are_escaped_as_unicode() {
        // 控制字符必须转义成 `\uXXXX` —— 否则规范化结果里会出现
        // 裸控制字节，那在 JSON 文本里是**非法**的。
        let v = json!("\u{0001}");
        let s = String::from_utf8(canonicalize(&v).unwrap()).unwrap();

        assert_eq!(s, r#""\u0001""#);
    }

    #[test]
    fn canonicalization_is_idempotent() {
        // 规范化一个**已规范化的**结果，必须得到同样的字节 ——
        // 这保证「哈希的输入是稳定的」，即使有人把规范化结果再存一次。
        let v = obj(&[("b", json!(2)), ("a", json!(1)), ("c", Value::Null)]);
        let once = canonicalize(&v).unwrap();
        let twice = canonicalize(&serde_json::from_slice(&once).unwrap()).unwrap();

        assert_eq!(once, twice);
    }
}
