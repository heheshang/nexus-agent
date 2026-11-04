//! 负向样例：`ModelDescriptor` 的 `api_key` **不能**作为公开字段访问。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! # 检验的设计条款
//!
//! `api_key` 是**解密后的明文口令**。它必须只能通过 [`expose_api_key`] 取用 ——
//! 这个方法名刻意啰嗦，是为了让「在普通代码里顺手取出口令」这件事
//! 在 review 时一眼可见。
//!
//! 如果 `api_key` 是公开字段，任何人都能写 `desc.api_key` ——
//! 而那会被 `Debug` 之外的路径轻易打到日志里。
//!
//! 期望的错误：
//! ```text
//! error[E0616]: field `api_key` of struct `ModelDescriptor` is private
//! ```

use nexus_llm::types::ModelDescriptor;

fn main() {
    let desc = ModelDescriptor::new("deepseek", "https://api.deepseek.com/v1", "sk-secret", "chat");

    // ❌ 故意写错：`api_key` 不是公开字段。
    let _leaked = desc.api_key;
}
