//! 负向测试：断言「写错网关核心类型用法的代码**编译不过**」。
//!
//! 架构文档 §5.2 —— 类型层的检验。
//!
//! `trybuild` 的工作方式：把 `tests/compile_fail/*.rs` 里的样例当作独立 crate 编译，
//! 断言它们**编译失败**，且错误信息与 `.stderr` 快照一致。
//!
//! 快照入库意味着：**编译器行为变化会立刻让测试失败** —— 这是刻意的，
//! 它保证「负向断言」不会因为工具链升级而悄悄失效。
//!
//! # 检验的设计条款
//!
//! | 用例 | 检验的设计条款 |
//! |---|---|
//! | `framework_selfcheck` | 负向测试链路本身可用 |
//! | `descriptor_api_key_is_not_a_public_field` | §2.6.5 —— 内部敏感信息不出现在类型表面 |
//! | `stream_event_cannot_be_deserialized` | §2.6.5 —— 内部错误原因绝不出现在对外响应 |

/// 框架自检：确认负向测试链路本身可用。
///
/// 这条样例故意写错类型，**必须编译失败**。它与业务语义无关，
/// 存在的意义是：若哪天 `trybuild` 因依赖或配置问题静默跳过全部用例，
/// 本用例会先失败，把问题暴露出来。
#[test]
fn framework_self_check() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/framework_selfcheck.rs");
}

/// `ModelDescriptor` 的 `api_key` 不是公开字段。
///
/// 期望的错误：
/// ```text
/// error[E0616]: field `api_key` of struct `ModelDescriptor` is private
/// ```
#[test]
fn descriptor_api_key_is_not_a_public_field() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/descriptor_api_key_is_not_a_public_field.rs");
}

/// `StreamEvent` 不实现 `Deserialize`。
///
/// 期望的错误：
/// ```text
/// error[E0277]: the trait bound `StreamEvent: Deserialize` is not satisfied
/// ```
#[test]
fn stream_event_cannot_be_deserialized() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/stream_event_cannot_be_deserialized.rs");
}
