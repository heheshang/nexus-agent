//! 负向测试入口：断言「写错的代码**编译不过**」。
//!
//! 架构文档 §5.2 —— 类型层的检验。
//!
//! `trybuild` 的工作方式：把 `tests/compile_fail/*.rs` 里的样例当作独立 crate 编译，
//! 断言它们**编译失败**，且错误信息与 `.stderr` 快照一致。
//!
//! 快照入库意味着：**编译器行为变化会立刻让测试失败** —— 这是刻意的，
//! 它保证「负向断言」不会因为工具链升级而悄悄失效。
//!
//! # 当前状态
//!
//! P1 尚未落地 `nexus-core` 的取消语义类型（`RequestScope` / `DetachedScope`），
//! 因此这里只保留一个**框架自检**用例：它只依赖 `std`，与业务类型无关，
//! 用来证明负向测试链路本身是通的。
//!
//! P1 落地后，本文件追加真正的主张：
//!
//! | 用例 | 检验的设计条款 |
//! |---|---|
//! | `detached_cannot_cancel` | §2.1.3 规则 A —— `DetachedScope` 上不存在 `cancel()` |
//! | `detached_not_request_scope` | §2.1.3 规则 A —— `DetachedScope` 不能当 `RequestScope` 用 |
//!
//! 可参考的现成实现：`examples/demo-cancel-typestate/`。

/// 框架自检：确认负向测试链路本身可用。
///
/// 这条样例故意写错类型，**必须编译失败**。它与业务语义无关，
/// 存在的意义是：若哪天 `trybuild` 因依赖或配置问题静默跳过全部用例，
/// 本用例会先失败，把问题暴露出来。
#[test]
fn 负向测试框架自检() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/framework_selfcheck.rs");
}
