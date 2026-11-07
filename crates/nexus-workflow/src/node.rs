//! 节点执行契约 —— **节点是工作流的唯一扩展点**。
//!
//! # 本模块负责什么
//!
//! | 关注点 | 类型 |
//! |---|---|
//! | **节点怎么被调用** | [`NodeInvoke`] |
//! | **失败怎么分类** | [`NodeError`]（三分类） |
//! | **产出长什么样** | [`NodeOutput`] |
//! | **入参长什么样** | [`NodeInput`] |
//! | **节点能看见什么** | [`NodeContext`] |
//! | **并发怎么限流** | [`ConcurrencyGate`] |
//!
//! # 为什么错误必须**三分类**（架构文档 §2.6.2）
//!
//! 领域错误是 **enum**，不是字符串。三类的**处置完全不同**：
//!
//! | 分类 | 处置 |
//! |---|---|
//! | [`NodeError::Retryable`] | 自动重试（带退避） |
//! | [`NodeError::Permanent`] | 终态失败，结束 |
//! | [`NodeError::Interrupt`] | 挂起，等人工审批 |
//!
//! 把三者合并成一个字符串 = 「到底该不该重试」变成**运行时猜** ——
//! 而「猜错」在支付类工具上就是**重复扣费**。
//!
//! # 为什么 `NodeInvoke` 用 `#[async_trait]` 而不是 AFIT
//!
//! AFIT（`async fn` in trait）**不能 `dyn` 化** ——
//! `async fn` 的 future 没有 `Send` 约束，无法放进 `tokio::spawn`。
//!
//! 而节点注册表是**异构集合**（不同类型的节点放进同一张表）——
//! 架构文档 §2.3.3 把「异构集合」列为明确保留 `dyn` 的场景。
//!
//! # ⚠️ 为什么 `dyn` 只出现在**装载处**（AC #12）
//!
//! 节点执行是**逐节点**的热点路径 —— 泛型 + 静态分发（零虚调用开销）。
//! `Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**（[`NodeRegistry`]）——
//! 那是「异构集合」的唯一合法场景。
//!
//! 热路径上用 `Box<dyn>` = 每个节点一次堆分配 + 一次虚调用 ——
//! 而这条断言由 `tests/architecture.rs` 的**结构层**扫描钉死。

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state::GraphState;

/// [`ErrorCode`] 的再导出 —— 让调用方不必再引 `nexus-core`。
pub use nexus_core::error::ErrorCode;

/// **默认并发槽数** —— 参考架构 §3.5.2 照抄 Go 版 `compiler.slots = 4`
/// （`internal/workflow/graph.go:37`）。
pub const DEFAULT_MAX_CONCURRENCY: usize = 4;

/// **节点入参** —— 节点执行时拿到的**上游产出**。
///
/// # 为什么是「键 → JSON 值」而不是一个裸 `Value`
///
/// 图状态的形状（[`GraphState`]）就是「键 → JSON 值」——
/// 入参与状态**同构**时，「取上游某个节点的产出」不需要额外的转换层。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeInput {
    /// 入参键值表。
    values: BTreeMap<String, Value>,
}

impl NodeInput {
    /// 构造（空入参）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 放入一个键值。
    pub fn with(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    /// 取一个键。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// 键值表。
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.values
    }

    /// 键的数量。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// **节点产出** —— 节点执行后写回图状态的内容。
///
/// # 为什么 `Option<NodeOutput>` 而不是「空产出」
///
/// `Ok(None)` 与 `Ok(Some(空表))` **语义不同**：
///
/// - `Ok(None)` = 「**无产出但正常**」—— 分支节点、纯判定节点走这条路；
/// - `Ok(Some(空表))` = 「产出了，但内容是空的」。
///
/// 合并成一个会丢掉「这个节点本来就不产出东西」这条信息。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeOutput {
    /// 产出键值表。
    values: BTreeMap<String, Value>,
}

impl NodeOutput {
    /// 构造（空产出）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 放入一个键值。
    pub fn with(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    /// 取一个键。
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// 键值表。
    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.values
    }

    /// 键的数量。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// **中断信号** —— 节点表示「我需要挂起等人工审批」。
///
/// # ⚠️ 这里**只有**最小骨架
///
/// 完整的 [`Interrupt`]（含 `args_hash` / `resume_key`）属 **NEX-40**。
/// 本任务（AC #2）只要求三分类 enum 存在，
/// 「审批指纹」那层语义由 NEX-40 落地。
///
/// 之所以**先放一个骨架**而不是等 NEX-40：
/// `NodeError::Interrupt` 必须是三分类之一（AC #2），
/// 而它的载荷类型必须在**本任务**里就有名字 —— 否则 enum 变体没法定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptSignal {
    /// 节点路径 —— 出问题时要能指出「是哪一次调用」。
    pub path: String,
    /// 审批提示。
    pub prompt: String,
}

impl InterruptSignal {
    /// 构造。
    pub fn new(path: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            prompt: prompt.into(),
        }
    }
}

/// **节点失败** —— [`NodeError`] 三分类里前两类的载荷。
///
/// # 为什么载荷是一个结构体而不是 `String`
///
/// `String` 只能带一句消息。失败**原因**要能被上层分类：
/// 「是哪个上游的哪个码失败了」——
/// 那需要**业务码**（[`ErrorCode`]）而不只是文本。
///
/// ⚠️ [`ErrorCode`] 是**已定稿**的 42 个码之一 —— 本 crate **不新增**码值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeFailure {
    /// 业务码 —— 决定重试策略的**唯一**依据。
    ///
    /// 复用 [`nexus_core::error::ErrorCode::retry_policy`] 的分类：
    /// 节点层**不另写一套**重试判据。
    pub code: ErrorCode,
    /// 面向用户的描述。
    pub message: String,
}

impl NodeFailure {
    /// 构造。
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// **节点错误** —— **三分类**（AC #2）。
///
/// # 为什么是 enum 而不是字符串
///
/// 架构文档 §2.6.2：领域错误是 **enum**，不是字符串。
/// `match` 穷尽 —— 新增变体时编译器会列出所有需要处理的地方。
///
/// # 三类的处置
///
/// | 分类 | 处置 | 典型场景 |
/// |---|---|---|
/// | [`NodeError::Retryable`] | 自动重试（带退避） | 上游临时不可用 |
/// | [`NodeError::Permanent`] | 终态失败 | 参数非法、图定义错误 |
/// | [`NodeError::Interrupt`] | 挂起等审批 | 人工节点、危险工具 |
///
/// # 为什么 `Permanent` 里也可能带「结果未知」的码
///
/// [`ErrorCode::ToolFailed`] 的重试策略是 `UnknownOutcome`（**不可**自动重试）——
/// 它仍然落在 `Permanent` 这一类里，因为「不可重试」与「不可恢复」是两件事：
/// `Permanent` 说的是「**不要自动重试**」，不是「状态一定干净」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeError {
    /// 明确失败，**可**重试。
    Retryable(NodeFailure),
    /// 明确失败，**不可**重试。
    Permanent(NodeFailure),
    /// 需要挂起等待人工审批。
    Interrupt(InterruptSignal),
}

impl NodeError {
    /// 是否**可**自动重试。
    ///
    /// # ⚠️ 中断**不是**可重试
    ///
    /// 挂起等审批要等人 —— 自动重试会**绕过**审批语义。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }

    /// 是否需要**挂起**。
    pub fn is_interrupt(&self) -> bool {
        matches!(self, Self::Interrupt(_))
    }

    /// 失败载荷（中断时为 `None`）。
    pub fn failure(&self) -> Option<&NodeFailure> {
        match self {
            Self::Retryable(f) | Self::Permanent(f) => Some(f),
            Self::Interrupt(_) => None,
        }
    }
}

impl std::fmt::Display for NodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retryable(failure) => {
                write!(
                    f,
                    "可重试失败（{}）：{}",
                    failure.code.code(),
                    failure.message
                )
            }
            Self::Permanent(failure) => {
                write!(
                    f,
                    "不可重试失败（{}）：{}",
                    failure.code.code(),
                    failure.message
                )
            }
            Self::Interrupt(signal) => {
                write!(f, "中断等待审批：{}（{}）", signal.path, signal.prompt)
            }
        }
    }
}

impl std::error::Error for NodeError {}

impl From<NodeError> for nexus_core::error::AppError {
    /// 把节点错误映射成**应用层**错误。
    ///
    /// # 映射规则
    ///
    /// | 节点错误 | 业务码 |
    /// |---|---|
    /// | `Retryable` | `ServiceUnavailable`（90003）—— 明确失败且**可**重试 |
    /// | `Permanent` | `Internal`（90001）—— 终态失败 |
    /// | `Interrupt` | `RunConflict`（30007）—— 挂起等待外部决策 |
    ///
    /// ⚠️ 中断映射到 `RunConflict` 而不是失败码 ——
    /// 挂起的 run **不是** failed 终态，它是「等待外部输入」。
    fn from(err: NodeError) -> Self {
        use nexus_core::error::AppError;
        let code = match &err {
            NodeError::Retryable(_) => ErrorCode::ServiceUnavailable,
            NodeError::Permanent(_) => ErrorCode::Internal,
            NodeError::Interrupt(_) => ErrorCode::RunConflict,
        };
        AppError::new(code, err.to_string())
    }
}

/// **节点执行上下文** —— 节点能看见的东西。
///
/// # 为什么**只读**
///
/// 节点**不直接改**图状态 —— 它返回 [`NodeOutput`]，
/// 由驱动层（`nexus-execution`）合并进 [`GraphState`]。
///
/// 理由：并发节点同时改同一份状态 = 要么加锁（热路径上的锁），
/// 要么靠「合并顺序」碰运气。**产出即合并**让并发是**无锁**的。
///
/// # 为什么上下文**不含**审批能力
///
/// 审批能力**不走 context** —— 那是 NEX-40 的**类型隔离**条款。
/// 本类型里没有「批准 / 放行」这类字段，
/// 将来也**不会**有 —— 越权路径要在**编译期**就编译不过。
#[derive(Debug, Clone, Copy)]
pub struct NodeContext<'a> {
    /// 节点路径 —— 出错时要能指出「是哪一次调用」。
    pub path: &'a str,
    /// 图状态的**只读**视图。
    pub state: &'a GraphState,
}

impl<'a> NodeContext<'a> {
    /// 构造。
    pub fn new(path: &'a str, state: &'a GraphState) -> Self {
        Self { path, state }
    }

    /// 节点路径。
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// 图状态的只读视图。
    pub fn state(&self) -> &'a GraphState {
        self.state
    }
}

/// **节点执行器** —— 节点扩展点的契约（AC #1）。
///
/// # 返回值语义（AC #3）
///
/// | 返回 | 语义 |
/// |---|---|
/// | `Ok(Some(out))` | 本节点**正常产出** |
/// | `Ok(None)` | **无产出但正常** |
/// | `Err(NodeError::Retryable(..))` | 明确失败，**可**重试 |
/// | `Err(NodeError::Permanent(..))` | 明确失败，**不可**重试 |
/// | `Err(NodeError::Interrupt(..))` | 需要**挂起**等待审批 |
///
/// # ⚠️ 为什么用 `#[async_trait]`
///
/// AFIT（`async fn` in trait）**不能 `dyn` 化** ——
///
/// 它的 future 没有 `Send` 约束，**不能**放进 `tokio::spawn`。
/// 节点任务**必须**能 `spawn` —— 所以这里用 `#[async_trait]`。
///
/// 代价是**一次虚调用 + 一次堆分配**（future 被装箱）——
/// 这就是为什么 `Arc<dyn NodeInvoke>` 只允许出现在**装载处**（AC #12）：
/// 热路径上用泛型（[`invoke_node`]）拿静态分发。
#[async_trait]
pub trait NodeInvoke: Send + Sync {
    /// 执行节点。
    ///
    /// # 错误
    ///
    /// 返回 [`NodeError`] 的三分类之一。
    async fn invoke(
        &self,
        ctx: &NodeContext<'_>,
        input: &NodeInput,
    ) -> Result<Option<NodeOutput>, NodeError>;
}

/// **泛型调用** —— 静态分发，**零**虚调用开销。
///
/// # 为什么需要这个函数
///
/// 架构文档 §2.3.2：泛型优先，`dyn` 兜底。
/// 当调用方**已知**具体节点类型时（编译期决定），
/// 走这里就没有装箱与虚调用。
///
/// # ⚠️ 这个函数**不是**必须的
///
/// 它是「**可以**静态分发」的接缝 —— 节点实现方仍可自由选。
#[allow(clippy::manual_async_fn)]
pub async fn invoke_node<N: NodeInvoke>(
    node: &N,
    ctx: &NodeContext<'_>,
    input: &NodeInput,
) -> Result<Option<NodeOutput>, NodeError> {
    node.invoke(ctx, input).await
}

/// **节点注册表** —— **图定义装载处**。
///
/// # ⚠️ 这是 `Arc<dyn NodeInvoke>` 的**唯一**合法位置（AC #12）
///
/// 装载处是「**异构集合**」—— 不同类型的节点必须放进同一张表，
/// 这正是架构文档 §2.3.3 明确保留 `dyn` 的场景。
///
/// 但装载**只发生一次**，执行**可能发生无数次** ——
/// 所以这里用 `Arc`（共享所有权）而不是 `Box`（独占所有权）：
/// `Arc` 让注册表与执行侧**共享**同一份实现，不需要克隆。
///
/// # ⚠️ `Debug` 是**手工**实现的
///
/// `Arc<dyn NodeInvoke>` **不**实现 `Debug` ——
/// trait 对象没有统一的「怎么打印」语义。
/// 手工实现只打印**键**（实现类型的名字拿不到，也不该拿）。
#[derive(Default)]
pub struct NodeRegistry {
    /// 节点键 → 实现。
    nodes: BTreeMap<String, Arc<dyn NodeInvoke>>,
}

impl std::fmt::Debug for NodeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeRegistry")
            .field("keys", &self.nodes.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl NodeRegistry {
    /// 构造（空注册表）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个节点实现。
    pub fn register(
        &mut self,
        key: impl Into<String>,
        node: Arc<dyn NodeInvoke>,
    ) -> Option<Arc<dyn NodeInvoke>> {
        self.nodes.insert(key.into(), node)
    }

    /// 取一个节点实现。
    pub fn get(&self, key: &str) -> Option<&Arc<dyn NodeInvoke>> {
        self.nodes.get(key)
    }

    /// 节点数量。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// **并发闸门** —— 限制**同时**在跑的节点任务数（AC #5）。
///
/// # 为什么用 `Semaphore` 而不是 `Mutex` 或「固定线程池」
///
/// 参考架构 §3.5.2 照抄 Go 版 `compiler.slots = 4`：
/// Go 用的是**信号量**语义（`graph.go:37`）。
/// Rust 的对等物就是 [`tokio::sync::Semaphore`]。
///
/// `Mutex` 是**独占**的（同时只有一个持有者）—— 语义完全不同。
/// 固定线程池会**绑死**线程数，且节点任务可能长时间 await。
///
/// # 为什么用 `acquire_owned()`
///
/// 节点任务会被 `tokio::spawn` —— 它 `'static`，
/// 因此**不能**借用闸门里的信号量。
/// [`tokio::sync::Semaphore::acquire_owned`] 消费 `Arc<Semaphore>`，
/// 产出 [`OwnedSemaphorePermit`]（`'static`）——
/// 这正是 spawn 场景需要的形态。
#[derive(Debug, Clone)]
pub struct ConcurrencyGate {
    /// 信号量 —— `Arc` 让闸门可以被克隆并共享。
    semaphore: Arc<tokio::sync::Semaphore>,
    /// 槽数。
    max: usize,
}

impl Default for ConcurrencyGate {
    /// 默认 **4** 个槽 —— 见 [`ConcurrencyGate::new`]。
    fn default() -> Self {
        Self::new()
    }
}

impl ConcurrencyGate {
    /// 构造 —— 默认 **4** 个槽（AC #5）。
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_MAX_CONCURRENCY)
    }

    /// 构造 —— 指定槽数。
    ///
    /// # ⚠️ 槽数**不能**是 0
    ///
    /// 0 个槽 = 任何节点都拿不到许可 = **永久挂起**。
    /// 那是一个「看起来能跑、实际全卡住」的状态 ——
    /// 所以这里**拒绝**它，而不是让调用方踩坑。
    ///
    /// # 错误
    ///
    /// 槽数为 0 → 返回 `None`（调用方必须处理）。
    pub fn try_with_limit(max: usize) -> Option<Self> {
        if max == 0 {
            return None;
        }
        Some(Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(max)),
            max,
        })
    }

    /// 构造 —— 指定槽数（**至少** 1）。
    ///
    /// 0 会被**抬到** 1 —— 见 [`ConcurrencyGate::try_with_limit`]。
    pub fn with_limit(max: usize) -> Self {
        Self::try_with_limit(max)
            .unwrap_or_else(|| Self::try_with_limit(1).expect("1 个槽必定合法"))
    }

    /// 槽数。
    pub fn max(&self) -> usize {
        self.max
    }

    /// 当前**可用**槽数。
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// 拿一个许可。
    ///
    /// # 为什么是 `acquire_owned`
    ///
    /// 节点任务会被 `tokio::spawn`（`'static`）——
    /// 借来的许可（`SemaphorePermit<'_>`）活不过 spawn 边界。
    /// [`OwnedSemaphorePermit`] 是 `'static` 的。
    ///
    /// # ⚠️ 闸门被**克隆**后许可仍然共享
    ///
    /// 闸门内部是 `Arc<Semaphore>` —— 克隆闸门**不**增加槽数。
    /// 这正是「4 个槽」的语义：整个图**同时**最多 4 个节点在跑。
    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, GateClosed> {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| GateClosed)
    }
}

/// 许可获取失败 —— 信号量已被**关闭**。
///
/// `tokio::sync::Semaphore::close` 只在显式调用时发生；
/// 它意味着「闸门不再接受新的节点」—— 上层应当**停止调度**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateClosed;

impl std::fmt::Display for GateClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "并发闸门已关闭")
    }
}

impl std::error::Error for GateClosed {}

/// [`OwnedSemaphorePermit`] 的再导出 —— 省得调用方自己引 `tokio`。
pub use tokio::sync::OwnedSemaphorePermit;
