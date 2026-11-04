//! 负向样例：直接构造 `DetachedScope`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.1.3 **规则 A** —— `DetachedScope` **只能**通过
//! `RequestScope::detach()` 构造：没有公开构造函数，字段也全部私有。
//!
//! 期望的错误：
//! ```text
//! error[E0451]: field `token` of struct `DetachedScope` is private
//! ```

use nexus_core::DetachedScope;
use tokio_util::sync::CancellationToken;

fn main() {
    let token = CancellationToken::new();

    // ❌ 违反规则 A：不允许绕过 `detach()` 直接构造 —— 字段是私有的。
    let _detached = DetachedScope { token };
}
