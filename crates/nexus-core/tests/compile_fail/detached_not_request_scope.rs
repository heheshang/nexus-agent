//! 负向样例：把 `DetachedScope` 当 `RequestScope` 用。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.1.3 —— 两种作用域是**独立类型**，不能互换。
//!
//! 期望的错误：
//! ```text
//! error[E0308]: mismatched types
//! ```

use nexus_core::RequestScope;

fn takes_request_scope(_: RequestScope) {}

fn main() {
    let detached = RequestScope::root().detach();

    // ❌ 违反设计：`DetachedScope` 不是 `RequestScope`。
    takes_request_scope(detached);
}
