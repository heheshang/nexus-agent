//! 负向测试：断言「写错取消语义的代码编译不过」。
//!
//! 设计文档 §5.2。
//!
//! 这是本设计**最核心**的可执行断言 —— 如果哪天 `DetachedScope` 上
//! 意外长出了 `cancel()` 方法，这个测试会立刻失败。

#[test]
fn cannot_call_cancel_on_non_cancellable_scope() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_cannot_cancel.rs");
}

#[test]
fn non_cancellable_scope_is_not_cancellable_scope() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/detached_not_request_scope.rs");
}
