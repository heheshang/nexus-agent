//! 负向样例：不可取消作用域上调用 `cancel()`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `cancel` found for struct `DetachedScope`
//! ```

use demo_cancel_typestate::RequestScope;

fn main() {
    let detached = RequestScope::new(|| {}).detach();

    // ❌ 违反设计：不可取消的作用域上不该有取消入口。
    detached.cancel();
}
