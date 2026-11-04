//! 负向样例：围栏写入**不要求**租约。
//!
//! **这个文件必须编译失败。** `trybuild` 断言其失败。
//!
//! 检验架构文档 §2.5.4 与 AC #4 的主张：
//! **写入方法签名要求 `&RunLease`（或 `&Fence`）—— 调用方无法绕过 fencing**。
//!
//! 围栏写入的 `WHERE` 子句里有 `generation` —— 它是 fencing 的**唯一依据**。
//! 如果签名不要求租约，调用方就能写出一个**不带围栏**的 UPDATE：
//!
//! ```ignore
//! sqlx::query("UPDATE execution_runs SET status = 'done' WHERE id = $1")
//! ```
//!
//! 僵尸 worker 就能覆盖新 worker 的写入 —— fencing 形同虚设。
//!
//! 期望的错误：
//! ```text
//! error[E0061]: this function takes 3 arguments but 2 arguments were supplied
//! ```

use nexus_repo::ExecutionLeaseRepository;

fn main() {
    let mut tx = unimplemented_tx();

    // ❌ 违反设计：围栏写入不接受「没有租约」的调用。
    let _ = ExecutionLeaseRepository::write_status_fenced(&mut tx, "done");
}

/// 造一个事务 —— **编译不过也没关系**，
/// 因为这条样例的断言点在 `main` 里。
fn unimplemented_tx() -> sqlx::Transaction<'static, sqlx::Postgres> {
    unimplemented!()
}
