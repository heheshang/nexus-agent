//! 负向样例：直接构造 `RunLease`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.5.4 的主张：**租约只能通过领取获得**。
//!
//! 如果 `RunLease::new` 是 `pub`，调用方可以随手写：
//!
//! ```ignore
//! let lease = RunLease::new(run_id, 0, "worker");
//! ```
//!
//! 一个**从未领取过**的租约 —— 它的 `generation = 0` 会匹配到
//! 还没被任何人领取过的 run，于是「fencing」形同虚设。
//!
//! 正确路径只有一条：先 `ExecutionLeaseRepository::claim` 领取。
//!
//! 期望的错误：
//! ```text
//! error[E0624]: associated function `new` is private
//! ```

use nexus_core::id::RunId;
use nexus_repo::RunLease;

fn main() {
    // ❌ 违反设计：租约不可伪造。
    let _lease = RunLease::new(RunId::new(1), 0, "worker");
}
