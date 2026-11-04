//! 负向样例：在 `RequestScope` 上落库。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.1.3 **规则 B** —— `DetachedScope::persist` 是**唯一**能落库的方法，
//! `RequestScope` 上**没有**落库方法。要落库就必须先 `detach()`。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `persist` found for struct `RequestScope`
//! ```

use nexus_core::RequestScope;

#[tokio::main]
async fn main() {
    let scope = RequestScope::root();

    // ❌ 违反规则 B：可取消的作用域上不该有落库入口。
    let _result = scope.persist(async { Ok::<_, nexus_core::PersistError>(()) }).await;
}
