//! 负向测试：断言租约与围栏的**类型约束**。
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
//! | `run_lease_has_no_generation_accessor` | §2.5.4 —— `RunLease` 上没有 `generation()` |
//! | `run_lease_cannot_be_constructed_directly` | §2.5.4 —— 租约只能通过领取获得 |
//! | `fenced_write_requires_a_lease` | §2.5.4 / AC #4 —— 围栏写入必须带租约 |

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

/// §2.5.4 —— `RunLease` 上**不存在** `generation()`。
///
/// `generation` 是 fencing 的唯一依据。它必须封装在租约对象里，
/// 调用方拿不到裸数字 —— 否则就能写出「手工的围栏条件」，
/// 少一个条件就静默失效。
///
/// 期望的错误：
/// ```text
/// error[E0599]: no method named `generation` found for struct `RunLease` in the current scope
/// ```
#[test]
fn run_lease_has_no_generation_accessor() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/run_lease_has_no_generation_accessor.rs");
}

/// §2.5.4 —— `RunLease` **不能**被直接构造。
///
/// 租约只能通过 `ExecutionLeaseRepository::claim` 领取获得。
/// 直接构造的租约（`generation = 0`）会匹配到还没被领取过的 run，
/// 让 fencing 形同虚设。
///
/// 期望的错误：
/// ```text
/// error[E0624]: associated function `new` is private
/// ```
#[test]
fn run_lease_cannot_be_constructed_directly() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/run_lease_cannot_be_constructed_directly.rs");
}

/// §2.5.4 / AC #4 —— 围栏写入**必须**带租约。
///
/// 签名要求 `&RunLease` —— 调用方无法写出「不带围栏」的 UPDATE。
///
/// 期望的错误：
/// ```text
/// error[E0061]: this function takes 3 arguments but 2 arguments were supplied
/// ```
#[test]
fn fenced_write_requires_a_lease() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/fenced_write_requires_a_lease.rs");
}
