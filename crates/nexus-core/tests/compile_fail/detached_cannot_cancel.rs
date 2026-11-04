//! 负向样例：不可取消作用域上调用 `cancel()`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.1.3 的核心主张：**`DetachedScope` 上根本不存在 `cancel` 方法**。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `cancel` found for struct `DetachedScope`
//! ```

use nexus_core::RequestScope;

#[tokio::main]
async fn main() {
    let detached = RequestScope::root().detach();

    // ❌ 违反设计：不可取消的作用域上不该有取消入口。
    detached.cancel();
}
