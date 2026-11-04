//! 负向样例：`StreamEvent` 的 `err` **不能**被反序列化还原。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! # 检验的设计条款
//!
//! `err` 标注了 `#[serde(skip)]` —— 它**永远不在 wire 上**。
//!
//! 但 `#[serde(skip)]` 有一个副作用：反序列化时该字段取默认值（`None`）。
//! 这意味着「从 JSON 里塞一个错误进来」在**运行期是静默失效的** ——
//! 而静默失效比编译失败更危险（你以为失败了，其实没有）。
//!
//! 所以额外提供 `Deserialize` 的实现是错的：正确的做法是让
//! 「从 wire 上还原一个带错误的 `StreamEvent`」**在编译期就不可能**。
//!
//! 期望的错误：
//! ```text
//! error[E0277]: the trait bound `StreamEvent: Deserialize` is not satisfied
//! ```

use nexus_llm::types::StreamEvent;

fn main() {
    // ❌ 故意写错：`StreamEvent` 不实现 `Deserialize`。
    let _ev: StreamEvent = serde_json::from_str(r#"{"err":"boom"}"#).unwrap();
}
