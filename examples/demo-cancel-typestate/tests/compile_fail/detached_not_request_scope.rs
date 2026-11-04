//! 负向样例：不可取消作用域不能当作可取消作用域使用。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 期望的错误：
//! ```text
//! error[E0308]: mismatched types
//! ```

use demo_cancel_typestate::RequestScope;

fn takes_request_scope(scope: RequestScope) {
    let _ = scope;
}

fn main() {
    let detached = RequestScope::new(|| {}).detach();

    // ❌ 违反设计：类型不同，不能隐式转换。
    takes_request_scope(detached);
}
