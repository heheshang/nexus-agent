//! 负向样例：调用 `RunLease` 上的 `generation()`。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.5.4 的核心主张：**`generation` 封装在租约对象里，
//! 调用方拿不到裸数字**。
//!
//! 如果 `RunLease` 暴露了 `generation()`，调用方就能写出：
//!
//! ```ignore
//! sqlx::query("UPDATE execution_runs SET status = $1 WHERE id = $2 AND generation = $3")
//!     .bind(status).bind(run_id).bind(lease.generation())
//! ```
//!
//! —— 一个**手工**的围栏条件。少一个条件就静默失效，
//! 而调用方**意识不到**。这就是「把不变量塞进类型，而不是靠约定」。
//!
//! 期望的错误：
//! ```text
//! error[E0599]: no method named `generation` found for struct `RunLease` in the current scope
//! ```

use nexus_repo::RunLease;

fn main() {
    // 用一个**真实**的构造路径拿到租约，避免 `todo!()` 带来的
    // `unreachable statement` 警告污染快照。
    //
    // 快照里的噪音会让「错误是否还是预期的那个」更难判断。
    let lease: RunLease = unimplemented_lease();

    // ❌ 违反设计：调用方不该能拿到裸 generation。
    let _generation = lease.generation();
}

/// 造一个 `RunLease` —— **编译不过也没关系**，
/// 因为这条样例的断言点在 `main` 里。
///
/// `RunLease::new` 是 `pub(crate)`，外部 crate 调不到 ——
/// 那正是「租约不可伪造」的意思。
fn unimplemented_lease() -> RunLease {
    unimplemented!()
}
