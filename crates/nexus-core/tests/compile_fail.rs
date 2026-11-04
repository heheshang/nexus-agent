//! 负向测试：断言「写错取消语义的代码**编译不过**」。
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
//! | `detached_cannot_cancel` | §2.1.3 —— `DetachedScope` 上不存在 `cancel()` |
//! | `detached_not_request_scope` | §2.1.3 —— `DetachedScope` 不能当 `RequestScope` 用 |
//! | `detached_no_token_escape_hatch` | §2.1.3 —— 没有 `token()` / `subscribe()` 逃生口 |
//! | `detached_not_constructible_directly` | §2.1.3 规则 A —— 只能通过 `detach()` 构造 |
//! | `request_scope_cannot_persist` | §2.1.3 规则 B —— `RequestScope` 上没有落库方法 |
//!
//! 可参考的现成实现：`examples/demo-cancel-typestate/`。

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

/// §2.1.3 —— `DetachedScope` 上不存在 `cancel()`。
///
/// 期望的错误：
/// ```text
/// error[E0599]: no method named `cancel` found for struct `DetachedScope`
/// ```
#[test]
fn detached_cannot_cancel() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_cannot_cancel.rs");
}

/// §2.1.3 —— `DetachedScope` 不能当 `RequestScope` 用。
///
/// 期望的错误：
/// ```text
/// error[E0308]: mismatched types
/// ```
#[test]
fn detached_not_request_scope() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_not_request_scope.rs");
}

/// §2.1.3 —— `DetachedScope` 上没有 `token()` / `subscribe()` 逃生口。
///
/// 期望的错误：
/// ```text
/// error[E0599]: no method named `token` found for struct `DetachedScope`
/// ```
#[test]
fn detached_has_no_token_escape_hatch() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_no_token_escape_hatch.rs");
}

/// §2.1.3 规则 A —— `DetachedScope` 只能通过 `detach()` 构造。
///
/// 字段私有，且没有公开的构造函数 —— 不允许绕过 `detach()` 直接构造。
///
/// 期望的错误：
/// ```text
/// error[E0451]: field `token` of struct `DetachedScope` is private
/// ```
#[test]
fn detached_is_not_directly_constructible() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_not_constructible_directly.rs");
}

/// §2.1.3 规则 B —— `RequestScope` 上**没有**落库方法。
///
/// 要落库必须先 `detach()`。这条断言保证「忘记 detach 的代码根本没法落库」。
///
/// 期望的错误：
/// ```text
/// error[E0599]: no method named `persist` found for struct `RequestScope`
/// ```
#[test]
fn request_scope_cannot_persist() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/request_scope_cannot_persist.rs");
}

/// §2.2.3 —— `EventSink` **不可 `Clone`**，物理上杜绝多写者。
///
/// 克隆写入器等于凭空造出第二个写者，序号不再与写入顺序一致。
///
/// 期望的错误：
/// ```text
/// error[E0599]: no method named `clone` found for struct `EventSink` in the current scope
/// ```
#[test]
fn event_sink_cannot_be_cloned() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/event_sink_cannot_be_cloned.rs");
}

/// §2.2.4 —— 事件负载的字段名写错 → **编译错误**。
///
/// 参考实现用 `map[string]any`，字段名写错不报错，只在前端变成 `undefined`。
///
/// 期望的错误：
/// ```text
/// error[E0559]: variant `EventPayload::ToolResult` has no field named `ms`
/// ```
#[test]
fn event_payload_field_name_typo_is_compile_error() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/event_payload_field_name_typo.rs");
}

/// §2.2.3 —— `emit` 取 `&mut self`，写入需要**独占借用**。
///
/// 独占借用意味着不可能同时持有两个对写入器的可变借用，
/// 序号与写入顺序天然一致。
///
/// 期望的错误：
/// ```text
/// error[E0499]: cannot borrow `sink` as mutable more than once at a time
/// ```
#[test]
fn event_sink_requires_mutable_borrow() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/event_sink_requires_mutable_borrow.rs");
}

/// §2.6.5 —— 内部原因**绝不外泄**。
///
/// `AppError` **刻意不实现 `Serialize`** —— 它带着 `cause`（内部原因，
/// 可能含数据库口令、上游 URL 等）。要输出给客户端，只能给 `UserFacing`。
/// 不实现 `Serialize` 从类型上消灭了「不小心把完整错误发给用户」的可能。
///
/// 期望的错误：
/// ```text
/// error[E0277]: the trait bound `AppError: Serialize` is not satisfied
/// ```
#[test]
fn app_error_is_not_serializable() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/app_error_is_not_serializable.rs");
}

/// §2.6.5 —— `UserFacing` 在**类型上**无法携带内部原因。
///
/// `UserFacing` 的字段是**公有**的（协议层要读它们），但它只有两个字段 ——
/// `code` 与 `message`。想加一个 `cause` 字段？编译器会拒绝。
///
/// 期望的错误：
/// ```text
/// error[E0560]: struct `UserFacing` has no field named `cause`
/// ```
#[test]
fn user_facing_cannot_carry_cause() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/user_facing_cannot_carry_cause.rs");
}
