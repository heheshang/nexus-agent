//! **图编译** —— 结构化 JSON 图定义 → 可执行图。
//!
//! 参考架构 §3.5.1 / §3.5.2；架构文档 §2.5.3（规范化）、
//! Spec 层 §3.2（图编译）。
//!
//! # 为什么图编译要「环检测」与「递归检测」
//!
//! **编译期发现不了 = 运行时死循环或栈溢出。**
//!
//! | 缺陷 | 运行时后果 |
//! |---|---|
//! | 图里有**环** | 拓扑排序永远排不完 → **死循环** |
//! | 子图**自引用** | 无限展开 → **栈溢出**（进程崩，不是错误返回） |
//!
//! 这两个都不是「跑不出正确结果」，而是「**跑不完**」——
//! 所以它们**必须**在编译期变成**错误**。
//!
//! # 展开上限是**照抄**的，不是本仓库的裁量
//!
//! 参考架构 §3.5.2 明确「**Rust 版**：全部照抄」：
//! 深度 8 / 节点 128 / 并发槽 4 / 状态 4 MiB。
//! 这些上限在参考实现里是**防 DSL 炸弹**的既有设计 ——
//! 另定数字等于「自己发明一套安全阈值」，那是没有依据的。
//!
//! # 触发语义是 `AllPredecessor`
//!
//! 参考架构 §3.5.1 的编译参数 `compose.WithNodeTriggerMode(compose.AllPredecessor)`。
//!
//! 「**所有**前驱完成才触发」与「任一前驱完成就触发」的区别是**语义**的：
//! 后者会让 join 节点在只收到一半输入时就开始跑 ——
//! 那不是「提前执行」，那是**读到未定义的状态**。
//!
//! # 子图是**按名注册**的，不���树内嵌
//!
//! 参考架构 §3.5.1：「`subflow` → 递归 `build` 生成子图
//! （子图 DSL 来自**冻结的** `definition.Subflows`）」。
//!
//! ⚠️ 这决定了**递归检测的判据**：
//!
//! - 若子图是**树内嵌**（`SubGraph { graph: Box<GraphDefinition> }`），
//!   JSON 是**树** —— 它**字面上无法包含自己**，
//!   「递归检测」就成了**永远不触发的死代码**；
//! - 按**名注册**（`subgraphs: BTreeMap<NodeId, GraphDefinition>`）后，
//!   「A 的子图里引用了 A」就是**可能的** ——
//!   展开 A → 展开 A → 无限递归 → **栈溢出**。
//!
//! 参考实现的递归检测正是这么做的（参考架构 §3.5.2：
//! 「子图递归检测 | `c.stack` map | `graph.go:161-168`」）——
//! 那个 `stack` 装的就是**正在展开的子图名**。
//!
//! # 节点键在摊平后是**带路径**的
//!
//! 子图里的节点与父图的节点**可能同名**（「start」很常见）。
//! 摊平进同一张图后它们会撞车 —— 所以摊平时给每个节点**限定路径**
//! （`sub/start`）。
//!
//! 这不只是为了避撞 —— 参考架构 §3.5.3 的审批用 `newApproval(path, …)`，
//! `path` 就是「这个节点在**哪张图**里」的路径。
//! 限定键**天然**就是那个路径。
//!
//! # 子图怎么**摊平**进父图
//!
//! `SubGraph` 节点是父图里的一个**容器**。展开后它变成链上的一个
//! **通道**节点（pass-through），子图的边**穿过**它：
//!
//! ```text
//! 父图:  a ──> [sub] ──> c
//! 子图:        └─> s1 ──> s2
//!
//! 摊平后: a ──> [sub] ──> s1 ──> s2 ──> c
//! ```
//!
//! 三条边分别是：
//!
//! - `a → sub` —— 父图的入边；
//! - `sub → s1` —— **容器 → 子图入口**；
//! - `s2 → c` —— **子图出口 → 父图后继**。
//!
//! # 为什么父图后继挂在**子图出口**而不是容器上
//!
//! ⚠️ 这是本模块**最容易被写错**的一处。
//!
//! 若把 `c` 的前驱设成**容器** `sub`，那么 `sub` 一完成 `c` 就能触发 ——
//! 但 `sub` 只是「子图的**入口**」，它完成**不代表子图跑完了**。
//! `c` 会在子图**还在跑**时就开始 —— 读到未定义的状态。
//!
//! 把 `c` 的前驱设成**子图出口** `s2`，`c` 就必须等子图**真的跑完**。
//!
//! 容器节点本身是**通道**（pass-through）：它一到就绪就完成，
//! 不做实际工作 —— 它的存在是为了**保留子图边界**
//! （审批路径要靠它区分「在子图里」与「在父图里」）。
//!
//! # 为什么摊平后还要**保留**容器节点
//!
//! 摊平的是**边**，不是**节点**。容器节点留在链上，
//! 执行期就能从「当前节点链」推出子图的路径 ——
//! 审批挂起时需要它（`path` 决定「这次审批在哪个图里」）。

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use nexus_core::error::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// **最大嵌套深度** —— 参考架构 §3.5.2（`graph.go:62`）。
///
/// 照抄。超限 → [`ErrorCode::BudgetExceeded`]。
pub const MAX_NESTING_DEPTH: usize = 8;

/// **最大展开节点总数** —— 参考架构 §3.5.2（`graph.go:141-142`）。
///
/// 照抄。超限 → [`ErrorCode::BudgetExceeded`]。
pub const MAX_EXPANDED_NODES: usize = 128;

/// **最大状态大小（字节）** —— 参考架构 §3.5.2（`graph.go:367-369`）。
///
/// 照抄。4 MiB。超限 → [`ErrorCode::BudgetExceeded`]。
pub const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;

/// **标识符** —— 图定义**内部**的节点键或子图名。
///
/// # 为什么它**不是**架构文档 §2.8.1 的 18 种 ID
///
/// 那张清单列的是**实体 ID 与技术标识 ID** —— 每一种都对应数据库里的
/// 一个**实体或外部系统标识**（`UserId` / `SessionId` / `RunId` / `TraceId` …）。
///
/// 本类型**不对应任何实体** —— 它是**一次编译内部**的键，
/// 落在 `workflow_step_runs.node_key VARCHAR(64)` 这样的**行内字段**里，
/// 而不是某个实体的主键。
///
/// ⚠️ 因此它**不进** `nexus-core::id`（那里是契约清单，
/// `crates/nexus-core/tests/contract.rs` 的契约 1 **硬断言**恰好 18 种），
/// 也**不进**架构文档 §2.8.1。它是**本 crate 的内部类型**。
///
/// # 为什么**类型化**而不是裸 `String`
///
/// 编译期有大量「键 ↔ 键」的比较与集合运算 ——
///
/// 传错类型（比如把图名当成节点键传进去）编译不过，
/// 比运行时才发现「图里没有这个节点」早得多。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// 构造。
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// 取回底层值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// **节点种类** —— 六种，全部存在。
///
/// 参考架构 §3.5.1 的节点类型 + §3.5.3 的审批节点。
///
/// | 种类 | 语义 |
/// |---|---|
/// | [`NodeKind::Lambda`] | 调用一个函数 |
/// | [`NodeKind::Branch`] | 按**布尔**条件选 yes / no 目标 |
/// | [`NodeKind::SubGraph`] | 挂载一个**子图**（按名引用，编译时摊平） |
/// | [`NodeKind::Parallel`] | 并行分支 |
/// | [`NodeKind::Human`] | 人工节点（挂起等审批） |
/// | [`NodeKind::Tool`] | 工具节点 |
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// 调用一个函数。
    Lambda {
        /// 被调用的标识（执行期由 driver 解析成实际实现）。
        invoke: String,
    },
    /// 按布尔条件选目标。
    Branch {
        /// 取布尔值的**状态键**。
        condition_key: String,
        /// 条件为真时的目标节点。
        yes: NodeId,
        /// 条件为假时的目标节点。
        no: NodeId,
    },
    /// 挂载一个子图 —— **按名**引用 [`GraphDefinition::subgraphs`] 里的子图。
    SubGraph {
        /// 子图名。
        name: NodeId,
    },
    /// 并行分支。
    Parallel {
        /// 参与并行的分支节点。
        branches: Vec<NodeId>,
    },
    /// 人工节点 —— 挂起等审批。
    Human {
        /// 审批提示。
        prompt: String,
    },
    /// 工具节点。
    Tool {
        /// 工具编码。
        tool_code: String,
    },
}

/// **图定义** —— 结构化 JSON 的形状。
///
/// # 为什么是**结构化 JSON** 而不是 DSL 文本
///
/// 决策 D15（OQ-2）：DSL **文本语法**是**产品选择**（用户写什么），
/// 不是执行语义 —— 它**不**阻塞 P6 的完成判据。
/// 参考架构 §5.3 局限 #5 已指出参考项目**同时存在两套工作流引擎**，
/// Rust 版**只做一套**（编译式）。
///
/// # 为什么子图是**按名注册**的
///
/// 见模块文档「子图是**按名注册**的，不��树内嵌」——
/// 树内嵌会让递归检测成为死代码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphDefinition {
    /// 图名。
    pub name: String,
    /// 节点表。
    pub nodes: Vec<NodeSpec>,
    /// **子图注册表** —— 键是子图名，值是子图定义。
    ///
    /// 子图**自己**的子图会**合并**进这张表 ——
    /// 名字在**整张图**里唯一，递归检测才有意义。
    #[serde(default)]
    pub subgraphs: BTreeMap<NodeId, GraphDefinition>,
}

/// **一个节点的定义**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpec {
    /// 节点键（**本层内**唯一）。
    pub id: NodeId,
    /// 节点种类。
    #[serde(flatten)]
    pub kind: NodeKind,
    /// **后继**节点 —— 该节点完成后，这些节点的前驱计数减一。
    #[serde(default)]
    pub next: Vec<NodeId>,
}

/// **编译后的节点**。
///
/// # 为什么**多**了 `predecessors`
///
/// `AllPredecessor` 触发语义要求「所有前驱完成才触发」——
/// 判定它「现在能不能触发」需要**前驱集合**。
/// 前驱是**从边反推**出来的（编译期一次性算好），
/// 执行期就不必再扫一遍图。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledNode {
    /// 节点键 —— 摊平后的**限定路径**（`sub/start`）。
    pub id: NodeId,
    /// 节点种类。
    pub kind: NodeKind,
    /// 后继节点 —— 出边指向的节点。
    pub next: Vec<NodeId>,
    /// **前驱**节点 —— 触发语义的判据。
    pub predecessors: Vec<NodeId>,
}

/// **编译后的图**。
///
/// # 为什么 `roots` 与 `nodes` **分开**
///
/// `roots` 是**入度为 0** 的节点 —— 也就是「从哪里开始」。
/// 它是 `nodes` 的一个**子集视图**，单独列出来是因为
/// 「图的入口」是执行期**反复**要用的东西（每个入口都要起一个任务），
/// 每次都重算一遍入度是浪费。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledGraph {
    /// 节点表 —— **拓扑序**（前驱总在后继之前）。
    pub nodes: Vec<CompiledNode>,
    /// 入口节点 —— 入度为 0 的那些。
    pub roots: Vec<NodeId>,
    /// 图名。
    pub name: String,
}

impl CompiledGraph {
    /// 按节点键取节点。
    pub fn node(&self, id: &NodeId) -> Option<&CompiledNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// 判定某节点**现在**能否触发（`AllPredecessor` 语义）。
    ///
    /// # 参数
    ///
    /// `completed` —— **已完成**的节点键集合。
    ///
    /// # 语义
    ///
    /// 节点的**所有**前驱都在 `completed` 里 → 可触发。
    ///
    /// ⚠️ 这是**纯判定**，不改变任何状态 —— 执行由 NEX-39 负责。
    /// 本方法存在的意义是：把「`AllPredecessor`」这条语义
    /// 变成**可测的**东西，而不是文档里的一句话。
    pub fn is_ready(&self, id: &NodeId, completed: &BTreeSet<NodeId>) -> bool {
        match self.node(id) {
            Some(node) => node.predecessors.iter().all(|p| completed.contains(p)),
            // ⚠️ 不存在的节点**不**可触发 —— 宁可「不执行」也不要「执行错的」。
            None => false,
        }
    }
}

/// **编译错误** —— 领域层（§2.6.2：领域错误是 enum，不是字符串）。
///
/// # 为什么编译失败**不用** `AppError`
///
/// `AppError` 是**应用层**错误（用例层 → 协议层）。
/// 图编译是**领域内部**的事 —— 它的失败原因（环、递归、超限）
/// 需要**结构化**地携带信息（「哪个节点在哪条边上构成的环」），
/// 而 `AppError` 的 `cause` 是 `Arc<dyn Error>` —— 那只能进日志，
/// **出不来**结构化信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// 图里有**环**。
    ///
    /// 携带环上的节点键 —— 报错要能指出**哪条链**构成了环，
    /// 否则用户面对一个「有环」只能自己找。
    Cycle {
        /// 环上的节点键（首尾相接）。
        cycle: Vec<NodeId>,
    },

    /// **子图递归**（自引用）。
    ///
    /// 携带展开路径 —— 「A → A」比「有递归」有用得多。
    RecursiveSubGraph {
        /// 展开路径（子图名）。
        path: Vec<NodeId>,
    },

    /// **嵌套深度**超限。
    DepthExceeded {
        /// 实际深度。
        depth: usize,
        /// 上限。
        limit: usize,
    },

    /// **展开节点总数**超限。
    NodeCountExceeded {
        /// 实际节点数。
        count: usize,
        /// 上限。
        limit: usize,
    },

    /// **状态大小**超限。
    StateSizeExceeded {
        /// 实际字节数。
        size: usize,
        /// 上限。
        limit: usize,
    },

    /// 引用了**不存在**的节点。
    UnknownNode {
        /// 发起引用的节点。
        node: NodeId,
        /// 被引用但不存在的节点。
        missing: NodeId,
    },

    /// 节点键**重复**。
    DuplicateNode {
        /// 重复的节点键。
        id: NodeId,
    },

    /// 子图名**重复**。
    DuplicateSubGraph {
        /// 重复的子图名。
        name: NodeId,
    },

    /// 图里**没有节点**。
    EmptyGraph,
}

/// 把节点键列表渲染成 `a → b → a` 的形状。
fn render_path(path: &[NodeId]) -> String {
    path.iter()
        .map(|id| id.as_str())
        .collect::<Vec<_>>()
        .join(" → ")
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cycle { cycle } => write!(f, "图存在环：{}", render_path(cycle)),
            Self::RecursiveSubGraph { path } => {
                write!(f, "子图递归引用：{}", render_path(path))
            }
            Self::DepthExceeded { depth, limit } => {
                write!(f, "嵌套深度 {depth} 超过上限 {limit}")
            }
            Self::NodeCountExceeded { count, limit } => {
                write!(f, "展开节点总数 {count} 超过上限 {limit}")
            }
            Self::StateSizeExceeded { size, limit } => {
                write!(f, "状态大小 {size} 字节超过上限 {limit}")
            }
            Self::UnknownNode { node, missing } => {
                write!(f, "节点 {node} 引用了不存在的节点 {missing}")
            }
            Self::DuplicateNode { id } => write!(f, "节点键重复：{id}"),
            Self::DuplicateSubGraph { name } => write!(f, "子图名重复：{name}"),
            Self::EmptyGraph => write!(f, "图里没有任何节点"),
        }
    }
}

impl std::error::Error for CompileError {}

impl From<CompileError> for AppError {
    /// 把编译错误映射成**应用层**错误。
    ///
    /// # 映射规则
    ///
    /// | 编译错误 | 业务码 | 理由 |
    /// |---|---|---|
    /// | 超限（深度 / 节点数 / 状态） | `BudgetExceeded`（30006） | 超出**预算** —— 照抄的展开上限 |
    /// | 环 / 递归 / 未知节点 / 重复 / 空图 | `WorkflowDsl`（30005） | 定义**本身非法** |
    ///
    /// ⚠️ **不是** panic —— 非法输入是**预期的**失败路径。
    fn from(err: CompileError) -> Self {
        let code = match &err {
            CompileError::DepthExceeded { .. }
            | CompileError::NodeCountExceeded { .. }
            | CompileError::StateSizeExceeded { .. } => ErrorCode::BudgetExceeded,
            CompileError::Cycle { .. }
            | CompileError::RecursiveSubGraph { .. }
            | CompileError::UnknownNode { .. }
            | CompileError::DuplicateNode { .. }
            | CompileError::DuplicateSubGraph { .. }
            | CompileError::EmptyGraph => ErrorCode::WorkflowDsl,
        };
        AppError::new(code, err.to_string())
    }
}

/// 检查**状态大小**是否超限（4 MiB）。
///
/// # 为什么是**独立函数**而不是编译期检查
///
/// 状态是**运行期**的东西 —— 编译期还没有状态。
/// 参考架构 §3.5.2 的检查在 `graph.go:367-369`，
/// 用 `serde_json::to_vec(&state).len()`。
///
/// 本函数给出**判据**与**错误**，NEX-39 的 `GraphState` 在写入时调用它。
///
/// # 错误
///
/// 超过 [`MAX_STATE_BYTES`] → [`CompileError::StateSizeExceeded`]。
pub fn check_state_size(size: usize) -> Result<(), CompileError> {
    if size > MAX_STATE_BYTES {
        return Err(CompileError::StateSizeExceeded {
            size,
            limit: MAX_STATE_BYTES,
        });
    }
    Ok(())
}

/// 编译一份图定义。
///
/// # 做了什么（按顺序）
///
/// 1. **汇总子图注册表** —— 递归收集所有层级的子图，名字**全局唯一**；
/// 2. **摊平**子图 —— 同时做**递归检测**与**深度 / 节点数**检查；
/// 3. **建索引** —— 节点键 → 节点；
/// 4. **校验** —— 重复键、悬空引用；
/// 5. **拓扑排序** —— 有环则**报错**（不排序、不死循环）；
/// 6. **算前驱** —— 从边反推。
///
/// # 为什么**先摊平再排序**
///
/// 子图的节点是**摊平后**才存在于同一张图里的 —— 不摊平就没法算前驱。
/// 参考架构 §3.5.1 的 `parallel` / `subflow` 都是「递归 `build` 生成子图」。
///
/// # 为什么摊平时**先查递归**再查上限
///
/// 递归是**结构错误**，上限是**预算问题** —— 递归应该先报，
/// 否则一个无限递归的图会先撞上节点数上限，报出一个**误导性**的错误。
///
/// # 错误
///
/// 见 [`CompileError`]。
pub fn compile(definition: &GraphDefinition) -> Result<CompiledGraph, CompileError> {
    // ── 汇总子图注册表 ──
    let mut registry: BTreeMap<NodeId, GraphDefinition> = BTreeMap::new();
    collect_subgraphs(definition, &mut registry)?;

    let mut expansion = Expansion::default();
    let mut subgraph_path: Vec<NodeId> = Vec::new();
    expand_level(
        definition,
        "",
        &registry,
        &mut subgraph_path,
        &mut expansion,
    )?;

    if expansion.nodes.is_empty() {
        return Err(CompileError::EmptyGraph);
    }

    // ── 建索引 + 校验重复 ──
    let mut index: BTreeMap<NodeId, usize> = BTreeMap::new();
    for (position, node) in expansion.nodes.iter().enumerate() {
        if index.insert(node.id.clone(), position).is_some() {
            return Err(CompileError::DuplicateNode {
                id: node.id.clone(),
            });
        }
    }

    // ── 校验悬空引用 ──
    //
    // ⚠️ 这一步**必须在**拓扑排序**之前** —— 否则拓扑排序会因为
    // 「找不到这个节点」而 panic 或静默跳过，错误就变得不可读了。
    for (from, to) in &expansion.edges {
        if !index.contains_key(to) {
            return Err(CompileError::UnknownNode {
                node: from.clone(),
                missing: to.clone(),
            });
        }
    }

    // ── 拓扑排序（Kahn）──
    let order = topological_order(&expansion, &index)?;

    // ── 算前驱 ──
    let mut predecessors: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for node in &expansion.nodes {
        predecessors.entry(node.id.clone()).or_default();
    }
    for (from, to) in &expansion.edges {
        predecessors
            .entry(to.clone())
            .or_default()
            .insert(from.clone());
    }

    let nodes: Vec<CompiledNode> = order
        .into_iter()
        .map(|id| {
            let node = &expansion.nodes[index[&id]];
            CompiledNode {
                id: node.id.clone(),
                kind: node.kind.clone(),
                next: expansion.successors(&node.id),
                predecessors: predecessors
                    .get(&id)
                    .map(|s| s.iter().cloned().collect())
                    .unwrap_or_default(),
            }
        })
        .collect();

    // ── 入口 = 入度为 0 ──
    let roots: Vec<NodeId> = nodes
        .iter()
        .filter(|n| n.predecessors.is_empty())
        .map(|n| n.id.clone())
        .collect();

    Ok(CompiledGraph {
        nodes,
        roots,
        name: definition.name.clone(),
    })
}

/// 递归收集所有层级的子图到**一张**注册表。
///
/// # 为什么名字要**全局唯一**
///
/// 子图名是**引用**的键（`SubGraph { name }`）——
/// 名字在两层里各指一张不同的表，引用就**含义不明**了。
///
/// # 为什么**合并**子图自己的子图
///
/// 子图的子图也**按名**被引用。名字在**整张图**里唯一，
/// 递归检测（按名字查展开路径）才有意义。
fn collect_subgraphs(
    definition: &GraphDefinition,
    registry: &mut BTreeMap<NodeId, GraphDefinition>,
) -> Result<(), CompileError> {
    for (name, subgraph) in &definition.subgraphs {
        if registry.insert(name.clone(), subgraph.clone()).is_some() {
            return Err(CompileError::DuplicateSubGraph { name: name.clone() });
        }
    }
    // 递归收集子图的子图 —— 用**快照**避免借用冲突。
    let children: Vec<GraphDefinition> = definition.subgraphs.values().cloned().collect();
    for subgraph in children {
        collect_subgraphs(&subgraph, registry)?;
    }
    Ok(())
}

/// 摊平过程中的中间产物。
#[derive(Default)]
struct Expansion {
    /// 摊平后的节点（按摊平顺序）。
    nodes: Vec<FlatNode>,
    /// 摊平后的边。
    edges: Vec<(NodeId, NodeId)>,
}

/// 摊平后的一个节点。
struct FlatNode {
    id: NodeId,
    kind: NodeKind,
}

impl Expansion {
    /// 某节点的出边目标（去重且**有序**）。
    fn successors(&self, id: &NodeId) -> Vec<NodeId> {
        let mut out: Vec<NodeId> = Vec::new();
        for (from, to) in &self.edges {
            if from == id && !out.contains(to) {
                out.push(to.clone());
            }
        }
        out
    }
}

/// 限定路径 —— 给节点键加上**父路径**前缀。
fn qualify(prefix: &str, id: &NodeId) -> NodeId {
    if prefix.is_empty() {
        id.clone()
    } else {
        NodeId::new(format!("{prefix}/{}", id.as_str()))
    }
}

/// 摊平**一层**图。
///
/// # 参数
///
/// - `definition` —— 这一层的定义；
/// - `prefix` —— 本层的**限定路径前缀**（顶层是 `""`）；
/// - `registry` —— 子图注册表；
/// - `subgraph_path` —— **正在展开**的子图名（**有序**）——
///   递归检测的判据，也是报错里的路径；
/// - `expansion` —— 累积的摊平结果。
///
/// # 返回
///
/// `(本层入口, 本层出口)` —— 入口是本层内**没有入边**的节点，
/// 出口是本层内**没有出边**的节点。
///
/// # 为什么同时要 `HashSet` 与 `Vec`
///
/// 判据用 `Vec` 的 `contains` 就够（层数 ≤ 8，代价可忽略），
/// 而 `Vec` **有序** —— 报错要能报出「A → A」这样的路径。
/// 一个 `HashSet` 做不到有序。
fn expand_level(
    definition: &GraphDefinition,
    prefix: &str,
    registry: &BTreeMap<NodeId, GraphDefinition>,
    subgraph_path: &mut Vec<NodeId>,
    expansion: &mut Expansion,
) -> Result<(Vec<NodeId>, Vec<NodeId>), CompileError> {
    // ── 深度检查 ──
    //
    // ⚠️ 深度是「**子图嵌套**的层数」——顶层是 **0**，
    // 进入第一个子图是 **1**。
    //
    // 这正是参考实现防「DSL 炸弹」要防的东西 ——
    // 它决定的是**展开时的递归深度**，
    // 所以「顶层图有几层节点」与它无关。
    if subgraph_path.len() > MAX_NESTING_DEPTH {
        return Err(CompileError::DepthExceeded {
            depth: subgraph_path.len(),
            limit: MAX_NESTING_DEPTH,
        });
    }

    // 本层的节点键集合 —— 校验「引用必须落在本层内」。
    let level_ids: HashSet<&NodeId> = definition.nodes.iter().map(|n| &n.id).collect();

    // 本层的边与出口 —— 分开收集，「出口」不能从最终的边集反推
    // （容器节点的出边指向子图，它**不是**本层的出口）。
    let mut level_edges: Vec<(NodeId, NodeId)> = Vec::new();
    let mut exits: Vec<NodeId> = Vec::new();

    for node in &definition.nodes {
        // ── 节点总数检查 ──
        if expansion.nodes.len() >= MAX_EXPANDED_NODES {
            return Err(CompileError::NodeCountExceeded {
                count: expansion.nodes.len() + 1,
                limit: MAX_EXPANDED_NODES,
            });
        }

        let qualified = qualify(prefix, &node.id);

        // ── 出边目标 —— 必须落在**本层**内 ──
        //
        // ⚠️ 这一条校验很重要：子图的节点引用了**父图**的节点时，
        // 那个引用会绕过容器 —— 父图后继的控制流就断了。
        let successors: Vec<NodeId> = declared_successors(node)
            .into_iter()
            .map(|target| {
                if !level_ids.contains(target) {
                    return Err(CompileError::UnknownNode {
                        node: node.id.clone(),
                        missing: target.clone(),
                    });
                }
                Ok(qualify(prefix, target))
            })
            .collect::<Result<Vec<_>, CompileError>>()?;

        match &node.kind {
            NodeKind::SubGraph { name } => {
                // ── 递归检测 ──
                //
                // ⚠️ 判据是「**这个子图名**已经在展开路径上」——
                // 参考实现用 `c.stack`（参考架构 §3.5.2）做的正是这件事。
                if let Some(start) = subgraph_path.iter().position(|id| id == name) {
                    let mut path = subgraph_path[start..].to_vec();
                    path.push(name.clone());
                    return Err(CompileError::RecursiveSubGraph { path });
                }

                let child = registry
                    .get(name)
                    .ok_or_else(|| CompileError::UnknownNode {
                        node: node.id.clone(),
                        missing: name.clone(),
                    })?;

                // 摊平子图 —— 递归。
                subgraph_path.push(name.clone());
                let (child_roots, child_exits) = expand_level(
                    child,
                    qualified.as_str(),
                    registry,
                    subgraph_path,
                    expansion,
                )?;
                subgraph_path.pop();

                // 容器 → 子图入口。
                for root in &child_roots {
                    level_edges.push((qualified.clone(), root.clone()));
                }

                if child_roots.is_empty() {
                    // 子图为**空** —— 容器直接连本层后继。
                    for target in &successors {
                        level_edges.push((qualified.clone(), target.clone()));
                    }
                    exits.push(qualified.clone());
                } else {
                    // 子图出口 → 本层后继。
                    //
                    // ⚠️ 这是「父图后继由**子图出口**把关」那条规则的落点 ——
                    // 挂在容器上会让 `c` 在子图跑完**之前**就绪。
                    for exit in &child_exits {
                        for target in &successors {
                            level_edges.push((exit.clone(), target.clone()));
                        }
                    }
                }
            }
            _ => {
                for target in &successors {
                    level_edges.push((qualified.clone(), target.clone()));
                }
                if successors.is_empty() {
                    exits.push(qualified.clone());
                }
            }
        }

        expansion.nodes.push(FlatNode {
            id: qualified,
            kind: node.kind.clone(),
        });
    }

    // ── 本层入口 = 本层内没有入边的节点 ──
    //
    // ⚠️ 只看**本层**的节点 —— 子图的节点不算本层的入口。
    // 判据用「本层的入边集合」，而不是扫描全图 ——
    // 全图里有子图的边，混进来会把子图的入口也当成父图的入口。
    let level_incoming: HashSet<&NodeId> = level_edges.iter().map(|(_, to)| to).collect();
    let roots: Vec<NodeId> = level_ids
        .iter()
        .map(|id| qualify(prefix, id))
        .filter(|id| !level_incoming.contains(id))
        .collect();

    expansion.edges.extend(level_edges);

    Ok((roots, exits))
}

/// 一个节点**声明**的出边目标（`next` + 分支的两条路 + 并行的分支）。
fn declared_successors(node: &NodeSpec) -> Vec<&NodeId> {
    let mut targets: Vec<&NodeId> = node.next.iter().collect();
    match &node.kind {
        NodeKind::Branch { yes, no, .. } => {
            targets.push(yes);
            targets.push(no);
        }
        NodeKind::Parallel { branches, .. } => {
            targets.extend(branches.iter());
        }
        _ => {}
    }
    targets
}

/// Kahn 拓扑排序。
///
/// # 为什么用**入度计数**
///
/// 入度为 0 的节点可以**立即**执行 —— 这正是 `AllPredecessor` 的判据。
/// 一轮下来能排出的节点数 < 总节点数 ⇒ **有环**。
///
/// # 为什么**不用** DFS 递归版
///
/// DFS 版的环检测依赖递归深度 —— 128 个节点的链在递归版里就是 128 层栈。
/// 而「图有环」在这里是**预期**的失败路径（恶意输入），
/// 用递归去处理它等于让恶意输入决定栈深度。
/// Kahn 版**没有**递归，栈深度是常数。
fn topological_order(
    expansion: &Expansion,
    index: &BTreeMap<NodeId, usize>,
) -> Result<Vec<NodeId>, CompileError> {
    let mut in_degree: BTreeMap<&NodeId, usize> =
        expansion.nodes.iter().map(|n| (&n.id, 0usize)).collect();

    for (_, to) in &expansion.edges {
        *in_degree.entry(to).or_insert(0) += 1;
    }

    let mut queue: VecDeque<NodeId> = expansion
        .nodes
        .iter()
        .filter(|n| in_degree.get(&n.id).copied().unwrap_or(0) == 0)
        .map(|n| n.id.clone())
        .collect();

    let mut order = Vec::with_capacity(expansion.nodes.len());
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        let node = &expansion.nodes[index[&id]];
        for target in expansion.successors(&node.id) {
            let degree = in_degree.get_mut(&target).expect("目标节点必然在索引里");
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(target);
            }
        }
    }

    if order.len() != expansion.nodes.len() {
        // 有环 —— 找出环上的节点报给用户。
        return Err(CompileError::Cycle {
            cycle: find_cycle(expansion, index),
        });
    }
    Ok(order)
}

/// 找出环上的节点（用于报错信息）。
///
/// # 为什么**要**找
///
/// 只报「图有环」不够用 —— 用户面对一个几十个节点的图，
/// 不知道**哪几个**节点构成了环。报出环本身，用户能**直接**看到问题。
///
/// # 怎么找
///
/// 先跑一遍拓扑排序找出**排不完**的节点 —— 它们必然在环上。
/// 再从其中任一个出发沿边走，直到走回起点 —— 那就是环。
fn find_cycle(expansion: &Expansion, index: &BTreeMap<NodeId, usize>) -> Vec<NodeId> {
    let mut in_degree: BTreeMap<&NodeId, usize> =
        expansion.nodes.iter().map(|n| (&n.id, 0usize)).collect();
    for (_, to) in &expansion.edges {
        *in_degree.entry(to).or_insert(0) += 1;
    }

    let mut remaining = in_degree.clone();
    let mut queue: VecDeque<NodeId> = expansion
        .nodes
        .iter()
        .filter(|n| remaining.get(&n.id).copied().unwrap_or(0) == 0)
        .map(|n| n.id.clone())
        .collect();
    let mut ordered: Vec<NodeId> = Vec::with_capacity(expansion.nodes.len());
    while let Some(id) = queue.pop_front() {
        ordered.push(id.clone());
        let node = &expansion.nodes[index[&id]];
        for target in expansion.successors(&node.id) {
            let degree = remaining.get_mut(&target).expect("目标节点必然在索引里");
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(target);
            }
        }
    }

    // 剩下的就是环上的节点 —— 从其中一个出发找回路。
    let Some(start) = expansion
        .nodes
        .iter()
        .find(|n| !ordered.contains(&n.id))
        .map(|n| n.id.clone())
    else {
        return Vec::new();
    };

    let mut path = vec![start.clone()];
    let mut seen: HashSet<NodeId> = HashSet::new();
    seen.insert(start.clone());
    let mut current = start.clone();

    while let Some(node) = expansion.nodes.get(index[&current]) {
        let Some(next) = expansion
            .successors(&node.id)
            .into_iter()
            .find(|t| seen.contains(t))
        else {
            break;
        };
        path.push(next.clone());
        if next == start {
            break;
        }
        current = next;
    }

    path
}
