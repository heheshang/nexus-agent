//! 负向样例：从 `DetachedScope` 拿取消 token。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.1.3 —— `DetachedScope` 上**没有** `token()` / `subscribe()`
//! 这类「暴露取消信号」的逃生口。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `token` found for struct `DetachedScope`
//! ```

use nexus_core::RequestScope;

fn main() {
    let detached = RequestScope::root().detach();

    // ❌ 违反设计：不可取消的作用域不该暴露取消信号。
    let _token = detached.token();
}
