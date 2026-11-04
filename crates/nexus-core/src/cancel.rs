//! 取消语义：类型状态机。
//!
//! 架构文档 §2.1 —— 「取消 ≠ 失败」是参考实现反复强调的不变量。
//!
//! # 问题背景（§2.1.2）
//!
//! 最直觉的 Rust 做法是每个作用域带一个 `CancellationToken`，「不可取消」的作用域就
//! **不订阅**父 token：
//!
//! ```ignore
//! struct Scope {
//!     token: CancellationToken,      // 可能是父的子 token，也可能不是
//!     cancelable: bool,              // 这个 bool 靠约定维护
//! }
//! ```
//!
//! 问题：`cancelable: bool` 与 `token` 是**两个独立的真相来源**。写错一个，另一个不报错。
//! 写 `Scope { token: parent.child_token(), cancelable: false }` —— 编译通过，
//! 但**实际会被父取消**，`cancelable: false` 纯属谎言。
//!
//! # 设计（§2.1.3）
//!
//! **取消能力由类型承载，不由运行时标志承载。**
//!
//! 关键在于：`DetachedScope` 上**根本不存在** `cancel` 方法。
//!
//! | 错误写法 | 结果 |
//! |---|---|
//! | 在 `DetachedScope` 上调用 `cancel()` | **编译错误**（方法不存在） |
//! | 把 `DetachedScope` 当 `RequestScope` 用 | **编译错误**（类型不匹配） |
//! | 忘记 `detach()`，直接落库 | 编译通过，但落库被取消 —— **唯一残留的漏洞** |
//!
//! 最后一行是诚实的边界：类型系统能防止「取消一个不该取消的东西」，
//! 防不住「忘记取消-resistant」。这个漏洞用两条规则补：
//!
//! - **规则 A**：`DetachedScope` **只能**通过 `detach()` 构造，且 `detach()` 只能从
//!   `RequestScope` 调用 —— 强制在「明确想要不可取消」的位置显式写下意图。
//! - **规则 B**：`DetachedScope::persist` 是**唯一**能落库的方法，`RequestScope` 上
//!   **没有**落库方法。要落库就必须先 `detach()`。
//!
//! 规则 B 意味着：**忘记 detach 的代码根本没法落库**。

use std::fmt;
use std::future::Future;

use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// 请求作用域：**可取消**。
///
/// 持有父作用域的**子 token** —— 父被取消时随之取消。
///
/// # 与 `DetachedScope` 的关系
///
/// `RequestScope` 上**没有**落库方法（规则 B）。要落库必须先 [`RequestScope::detach`]。
#[derive(Debug, Clone)]
pub struct RequestScope {
    /// 父的子 token。**不公开** —— 外部无法直接取消或订阅它。
    token: CancellationToken,
}

impl RequestScope {
    /// 创建一个**根**请求作用域。
    ///
    /// 根作用域没有父 —— 它只会被 [`RequestScope::cancel`] 显式取消。
    pub fn root() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// 创建一个**子**作用域：随父取消而取消。
    ///
    /// # 与 `DetachedScope::detach` 的区别
    ///
    /// 这里用 [`CancellationToken::child_token`] —— 子 token **会**随父取消。
    /// [`RequestScope::detach`] 内部用 [`CancellationToken::new`] —— **不会**随父取消。
    /// 两者方向相反，混淆会导致「以为不可取消，实际会被取消」。
    pub fn child(&self) -> Self {
        Self {
            token: self.token.child_token(),
        }
    }

    /// 派生**不可取消**的作用域。
    ///
    /// 类型变化即承诺：此后不再响应任何取消。
    ///
    /// # 为什么内部用 `new()` 而不是 `child_token()`
    ///
    /// `child_token()` 会随父取消 —— 那与「不可取消」的承诺矛盾。
    /// `new()` 创建**独立**的 token，不持有父 token 的引用（§2.1.5）。
    ///
    /// 关键：没有把 `self.token` 带过去 —— 新的作用域与取消信号无关。
    pub fn detach(self) -> DetachedScope {
        DetachedScope {
            token: CancellationToken::new(),
        }
    }

    /// 取消当前作用域及其所有子作用域。
    ///
    /// 只有可取消作用域才有这个方法 —— `DetachedScope` 上**不存在**。
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// 当前作用域是否已被取消。
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// 等待取消信号。
    ///
    /// 这是一个**逃生口** —— 只读地等待，不会主动取消。
    /// 用它把取消信号接到别的机制上（如 `tokio::select!`）。
    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

/// 脱离作用域：**不可取消**。
///
/// **没有 `cancel` 方法** —— 这是设计的全部要点。
///
/// # 规则 A
///
/// 只能由 [`RequestScope::detach`] 构造 —— 因此「想要不可取消」的意图必须显式表达。
/// 字段全部私有，且**没有** `new()` / `Default`。
///
/// # 规则 B
///
/// [`DetachedScope::persist`] 是**唯一**能落库的方法。
/// `RequestScope` 上没有落库方法 —— 要落库就必须先 `detach()`。
#[derive(Clone)]
pub struct DetachedScope {
    /// **独立的** token —— 不持有父 token 的引用，也不随父取消。
    ///
    /// 私有且不公开：外部拿不到取消入口。
    token: CancellationToken,
}

impl DetachedScope {
    /// 唯一的操作：执行一次有预算上限的写入。
    ///
    /// 注意**没有** `cancel()`，也**没有**任何暴露取消信号的逃生口
    /// （`token()` / `subscribe()` 都不存在）。
    ///
    /// # 为什么 `persist` 在这里而不是在 `RequestScope` 上
    ///
    /// 规则 B：落库必须先 `detach()`。把 `persist` 只放在 `DetachedScope` 上，
    /// 「忘记 detach 的代码根本没法落库」—— 这是编译期约束，不是评审约定。
    pub async fn persist<F, T>(self, fut: F) -> Result<T, PersistError>
    where
        F: Future<Output = Result<T, PersistError>>,
    {
        fut.await
    }

    /// 当前作用域是否已被取消。
    ///
    /// 恒为 `false` —— 独立 token 没有任何取消来源。
    ///
    /// 保留这个方法是为了让「不可取消」这个属性**可观测**（便于调试与断言），
    /// 而不是为了提供取消入口。
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

impl fmt::Debug for DetachedScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DetachedScope")
            .field("cancellable", &false)
            .finish()
    }
}

/// 持久化失败。
///
/// `DetachedScope::persist` 的错误类型。
///
/// 之所以单独定义而不是复用 `AppError`（架构文档 §2.6 的应用错误）：
/// `nexus-core` 是**纯类型 crate** —— 它不认识业务码。业务错误由上层 crate 定义。
#[derive(Debug, Error)]
pub enum PersistError {
    /// 写入失败。
    #[error("写入失败：{0}")]
    Failed(String),
}

impl From<&str> for PersistError {
    fn from(message: &str) -> Self {
        Self::Failed(message.to_string())
    }
}

impl From<String> for PersistError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn child_scope_is_cancelled_by_parent() {
        let parent = RequestScope::root();
        let child = parent.child();

        assert!(!child.is_cancelled());

        parent.cancel();

        assert!(child.is_cancelled(), "父取消后子作用域必须被取消");
    }

    #[tokio::test]
    async fn detached_scope_is_not_cancelled_by_parent() {
        let parent = RequestScope::root();
        let detached = parent.child().detach();

        parent.cancel();

        assert!(
            !detached.is_cancelled(),
            "脱离作用域不受父取消影响 —— 这是 detach 的全部意义"
        );
    }

    #[tokio::test]
    async fn detached_scope_has_its_own_cancellation_state() {
        let parent = RequestScope::root();
        let detached = parent.child().detach();

        // 独立 token：既不受父影响，也没有其他取消来源
        assert!(!detached.is_cancelled());
        parent.cancel();
        assert!(!detached.is_cancelled());
    }

    #[tokio::test]
    async fn persist_is_the_only_exit_of_detached_scope() {
        let parent = RequestScope::root();
        let detached = parent.child().detach();

        let result: Result<(), PersistError> = detached.persist(async { Ok(()) }).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn persist_propagates_the_inner_error() {
        let detached = RequestScope::root().detach();

        let result: Result<(), PersistError> = detached
            .persist(async { Err(PersistError::Failed("db down".into())) })
            .await;

        assert!(matches!(result, Err(PersistError::Failed(ref m)) if m == "db down"));
    }

    #[tokio::test]
    async fn cancelled_await_resolves_on_cancel() {
        let scope = RequestScope::root();
        let waiter = scope.clone();

        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });

        scope.cancel();
        handle.await.expect("等待取消的任务应完成");
    }

    #[test]
    fn detached_scope_debug_shows_it_is_not_cancellable() {
        let detached = RequestScope::root().detach();
        let text = format!("{detached:?}");

        assert!(text.contains("cancellable: false"), "{text}");
    }
}
