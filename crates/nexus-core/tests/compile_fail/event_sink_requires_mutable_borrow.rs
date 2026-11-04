//! 负向样例：绕过 `&mut self` 写事件。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.2.3 —— `emit` 取 `&mut self`，写入需要**独占借用**，
//! 编译期串行化。
//!
//! 独占借用意味着：不可能同时持有两个对写入器的可变借用，
//! 所以序号分配与写入顺序天然一致。
//!
//! 期望的错误：
//! ```text
//! error[E0499]: cannot borrow `sink` as mutable more than once at a time
//! ```

use nexus_core::EventSink;
use nexus_core::RunId;

fn main() {
    let mut sink = EventSink::new(RunId::new(1));

    // ❌ 违反设计：两个重叠的可变借用。
    let first = &mut sink;
    let second = &mut sink;

    let _ = (first, second);
}
