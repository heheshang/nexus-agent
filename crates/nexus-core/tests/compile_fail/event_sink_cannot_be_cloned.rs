//! 负向样例：克隆 `EventSink`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.2.3 —— `EventSink` **不可 `Clone`**，
//! 物理上杜绝多写者。
//!
//! 原因：`mpsc::Sender::send()` 是异步的 —— 如果在多个地方并发发送，
//! **谁先被 poll 到是不确定的**，序号分配与写入顺序就会脱节。
//! 克隆写入器等于凭空造出第二个写者，序号不再与写入顺序一致。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `clone` found for struct `EventSink` in the current scope
//! ```

use nexus_core::EventSink;
use nexus_core::RunId;

fn main() {
    let sink = EventSink::new(RunId::new(1));

    // ❌ 违反设计：克隆会产生第二个写者。
    let _second = sink.clone();
}
