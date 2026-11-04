//! demo-cancel-typestate 的可执行演示。
//!
//! 类型定义在 `lib.rs`（`trybuild` 负向测试需要 lib target）。

use demo_cancel_typestate::{RequestScope, Writer};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let writer = Writer::default();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // ── 场景 1：可取消的作用域 ──
    let flag = Arc::clone(&cancelled);
    let scope = RequestScope::new(move || flag.store(true, std::sync::atomic::Ordering::SeqCst));
    scope.cancel();
    println!(
        "✓ 可取消作用域：cancel() 触发回调 = {}",
        cancelled.load(std::sync::atomic::Ordering::SeqCst)
    );

    // ── 场景 2：不可取消的作用域 ──
    let scope = RequestScope::new(|| unreachable!("取消回调不应被触发"));
    let detached = scope.detach();

    // 落库 —— 唯一的出口
    let w = &writer;
    detached
        .persist(async move {
            w.write("助手消息");
            w.write("配额释放");
            Ok(())
        })
        .await
        .expect("持久化应成功");

    println!("✓ 不可取消作用域：persist() 落库 = {:?}", writer.written());
    println!("✓ DetachedScope 上不存在 cancel() —— 见 tests/compile_fail/");
}
