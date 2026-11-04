//! demo-cancel-typestate 的**库**部分 —— 类型定义。
//!
//! 验证设计文档 §2.1.3 的核心主张：
//! **「写错取消语义的代码编译不过」**。
//!
//! 关键点：`DetachedScope` 上**不存在** `cancel` 方法 —— 这不是运行时检查，
//! 是类型系统保证。`tests/compile_fail/` 里的样例**故意编译不过**，
//! 由 `tests/compile_fail.rs` 用 `trybuild` 断言其失败。

use std::marker::PhantomData;
use std::sync::Arc;

/// 可取消的作用域。
///
/// 由「父作用域」派生而来，随父取消而取消。
pub struct RequestScope {
    cancel: Arc<dyn Fn() + Send + Sync>,
    _p: PhantomData<*const ()>,
}

/// 不可取消的作用域。
///
/// **没有 `cancel` 方法** —— 这是设计的全部要点。
///
/// 只能由 `RequestScope::detach()` 构造，因此「想要不可取消」的意图必须显式表达。
#[derive(Debug)]
pub struct DetachedScope {
    _p: PhantomData<*const ()>,
}

impl std::fmt::Debug for RequestScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestScope").finish_non_exhaustive()
    }
}

impl RequestScope {
    /// 创建作用域。`on_cancel` 在取消时被调用。
    pub fn new(on_cancel: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            cancel: Arc::new(on_cancel),
            _p: PhantomData,
        }
    }

    /// 派生出**不可取消**的作用域。
    ///
    /// 类型变化即承诺：此后不再响应任何取消。
    pub fn detach(self) -> DetachedScope {
        // 关键：没有把 `self.cancel` 带过去 —— 新的 scope 与取消信号无关。
        DetachedScope { _p: PhantomData }
    }

    /// 取消当前作用域。可取消作用域才有的能力。
    pub fn cancel(&self) {
        (self.cancel)();
    }
}

impl DetachedScope {
    /// 唯一的操作：执行一次有预算上限的写入。
    ///
    /// 注意**没有** `cancel()`，也**没有**任何暴露取消信号的逃生口。
    pub async fn persist<F, T>(self, fut: F) -> Result<T, PersistError>
    where
        F: std::future::Future<Output = Result<T, PersistError>>,
    {
        fut.await
    }
}

/// 持久化失败。
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("写入失败：{0}")]
    Failed(String),
}

/// 一个「写库」动作 —— 演示里用计数器代替真实数据库。
#[derive(Debug, Default)]
pub struct Writer {
    written: std::sync::Mutex<Vec<&'static str>>,
}

impl Writer {
    pub fn write(&self, what: &'static str) {
        self.written.lock().expect("锁中毒").push(what);
    }
    pub fn written(&self) -> Vec<&'static str> {
        self.written.lock().expect("锁中毒").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 可取消作用域的取消信号被触发() {
        let hit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let h = Arc::clone(&hit);
        let scope = RequestScope::new(move || h.store(true, std::sync::atomic::Ordering::SeqCst));
        scope.cancel();
        assert!(hit.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn 脱离后取消回调不再触发() {
        let hit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let h = Arc::clone(&hit);
        let _detached =
            RequestScope::new(move || h.store(true, std::sync::atomic::Ordering::SeqCst)).detach();

        // 外部「取消」发生时，已脱离的作用域不受影响
        assert!(!hit.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn persist_是_不可取消作用域_唯一_的出口() {
        let writer = Writer::default();
        let w = &writer;
        RequestScope::new(|| {})
            .detach()
            .persist(async move {
                w.write("x");
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(writer.written(), vec!["x"]);
    }
}
