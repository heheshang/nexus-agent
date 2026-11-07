---
id: NEX-38
title: "feat(workflow): 落地冻结定义与图编译"
status: In Review
assignee: []
labels: [workflow, graph-engine, core]
priority: high
dependencies: []
references:
  - "docs/specs/p6/spec.md §3.1、§3.2"
  - "docs/specs/p6/tasks.md T-002"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2、§2.5.3、§2.5.5"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.5.1、§3.5.2"
---

## Description

**问题**：Rust **没有** Eino 的对等物 —— 参考架构 §5.3 局限 #13 明写
「自研图引擎是最大工作量」。`crates/nexus-workflow/src/lib.rs` 目前只有 `CRATE_NAME`。

没有冻结定义与图编译，持久执行**无处落脚** —— driver 拿不到「这次跑的到底是什么」。

**修法**：落地两件事：

1. **冻结定义**（架构文档 §2.5.2 的三件套）—— `canonical` / `hash` / `engine_version`；
2. **图编译** —— 结构化 JSON 图定义 → `CompiledGraph`（拓扑排序 + 入度计数 + 展开上限）。

**为什么图编译要做「环检测」与「递归检测」**：编译期发现不了 = 运行时死循环或栈溢出。
参考架构 §3.5.2 的展开上限（深度 8 / 节点 128 / 状态 4 MiB）是**照抄**的，
不是本仓库的裁量。

## 范围边界

### 做

- `nexus-workflow/src/definition.rs`：`FrozenDefinition` + 哈希
- `nexus-workflow/src/graph.rs`：`CompiledGraph` / `CompiledNode` / `NodeKind` + 编译

### 不做

| 项 | 理由 |
|---|---|
| **DSL 文本前端** | 决策 D15（OQ-2）—— 登记为 F4 |
| 节点**执行**逻辑 | 属 NEX-39 |
| 节点**中断**提案 | 属 NEX-40 |
| 持久化 / 租约 / 幂等 | 属 NEX-37 / NEX-41 ~ NEX-45 |

## Acceptance Criteria

- [x] #1 `crates/nexus-workflow/src/definition.rs` 定义 `FrozenDefinition` —— 含 `canonical` / `hash` / `engine_version` **三个**字段
- [x] #2 定义哈希用 `nexus_core::canonical::canonical_hash_hex` —— **不自己实现**规范化
- [x] #3 规范化键序无关 —— 同一份定义、键序不同，`hash` **相同**
- [x] #4 `crates/nexus-workflow/src/graph.rs` 定义 `CompiledGraph` —— 含 `nodes` / `roots` / `name`
- [x] #5 节点种类**全部**存在：`Lambda` / `Branch` / `SubGraph` / `Parallel` / `Human` / `Tool`
- [x] #6 触发语义是 `AllPredecessor` —— 节点**所有**前驱完成后才触发（测试：两个前驱只完成一个时节点**不**执行）
- [x] #7 编译做**拓扑排序 + 入度计数**；图有**环**时编译**失败**（不是死循环）
- [x] #8 **子图递归检测** —— 编译时用 `HashSet<NodeId>` 检测自引用，递归 → **错误**（不是无限展开）
- [x] #9 展开上限**三个各自生效**：嵌套深度 > **8** → 错误；展开节点总数 > **128** → 错误；状态大小 > **4 MiB** → 错误
- [x] #10 超限映射到 `ErrorCode::BudgetExceeded`（30006），**不是** panic
- [x] #11 `crates/nexus-workflow/src/lib.rs` 的 crate 级文档写明「本 crate 负责图引擎 / 不负责持久化」
- [x] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### 验收证据（可复核）

| AC | 证据（文件::测试名） |
|---|---|
| #1 | `tests/definition.rs::frozen_definition_exposes_canonical_hash_and_engine_version` |
| #2 | `tests/definition.rs::hash_matches_the_shared_canonical_implementation`、`::canonical_bytes_match_the_shared_canonical_implementation` |
| #3 | `tests/definition.rs::hash_is_independent_of_key_order`、`::key_order_independence_comes_from_canonicalization` |
| #4 | `tests/graph.rs::compiled_graph_exposes_nodes_roots_and_name` |
| #5 | `tests/graph.rs::all_six_node_kinds_survive_compilation` |
| #6 | `tests/graph.rs::node_waits_for_all_predecessors`（两个前驱只完成一个 → **不**触发） |
| #7 | `tests/graph.rs::cycle_is_rejected_instead_of_hanging`、`::self_loop_is_rejected` |
| #8 | `tests/graph.rs::recursive_subgraph_is_rejected_instead_of_expanding_forever`、`::mutually_recursive_subgraphs_are_rejected` |
| #9 | `tests/graph.rs::each_expansion_limit_triggers_independently`、`::exactly_the_depth_limit_is_accepted`、`::exactly_the_node_limit_is_accepted` |
| #10 | `tests/graph.rs::limit_violations_map_to_budget_exceeded_not_panic`、`::state_size_violation_maps_to_budget_exceeded` |
| #11 | `tests/architecture.rs::the_workflow_crate_documents_its_responsibility_boundary` |
| #12 | 全量 `cargo test --workspace --locked`：**613 通过 / 0 失败 / 0 忽略** |

**三层验证覆盖**：

| 层 | 手段 | 用例 |
|---|---|---|
| 行为层 | 集成 / 不变量测试 | `tests/definition.rs`（10）+ `tests/graph.rs`（21） |
| 结构层 | 架构测试（token 级扫描，剥离注释） | `tests/architecture.rs`（3） |
| 类型层 | trybuild 负向测试 | 本任务**不需要** —— 无「写错编译不过」的新类型约束（`NodeId` 是普通 newtype，`NodeKind` 是公开 enum） |

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「类型 / 常量存在」「返回值可观测」
「哈希相等 / 不等」「错误码可断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.1、§3.2` —— 存在
- `docs/specs/p6/tasks.md T-002` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2、§2.5.3` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.5.1、§3.5.2` —— 存在
- `nexus_core::canonical::canonical_hash_hex` —— 存在于 `crates/nexus-core/src/canonical.rs:100`

### #4 范围有界

**单 crate**（`nexus-workflow`）的**两个**新模块 + 测试。
纯内存计算，**无**数据库、**无**并发写、**不新增** crate 依赖方向。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-workflow/src/definition.rs` | **新增** |
| `crates/nexus-workflow/src/graph.rs` | **新增** |
| `crates/nexus-workflow/src/lib.rs` | **修改**（模块声明 + crate 级文档） |
| `crates/nexus-workflow/tests/definition.rs` | **新增** |
| `crates/nexus-workflow/tests/graph.rs` | **新增** |
| `crates/nexus-workflow/tests/architecture.rs` | **新增** |
| `crates/nexus-workflow/Cargo.toml` | **修改**（见「实施偏离」—— **只**加 `syn` dev-dependency） |

### #6 完成态可描述

调用方可以把一份结构化 JSON 图定义**编译**成 `CompiledGraph`；
环、递归、超限三种非法输入**全部被编译期拒绝**并返回明确错误；
同一份定义的哈希**与键序无关**。

### #7 无隐形依赖

- 无前置任务（`nexus_core::canonical` 已在 P3 落地）
- `evalexpr` 与 `serde_json` 已在 `crates/nexus-workflow/Cargo.toml` 声明 —— **不引入新依赖**
- DSL 文本前端**已明确划出范围**（决策 D15）—— 不是隐形依赖

### 结论

**pass** —— 七项全过，可以开工。

## 实施偏离

> ⚠️ DoR 的 #5 与 #7 写「`Cargo.toml` **不修改**」「**不引入新依赖**」。
> 实施中**偏离**了这一点 —— 记录在此并说明理由。

### 偏离 1：`Cargo.toml` 新增 `syn`（dev-dependency）

- **偏离内容**：新增 `[dev-dependencies] syn.workspace = true`。
- **理由**：结构层断言需要 **token 级扫描器**（`crates/nexus-core/tests/support/token_scan.rs`），
  而它基于 `syn`。**字符串查找做不到** —— crate 级文档里**恰恰要**出现
  `sqlx` 一词（「本 crate 不需要 `sqlx`」），字符串查找会把它算进去 → 假阳性。
  这正是 NEX-48 建立的纪律：**源码扫描类断言必须剥离注释**。
- **为什么是 dev-dependency**：它**只**作用于测试编译，**不影响生产依赖方向** ——
  `scripts/check-deps.sh` 仍绿，`nexus-workflow` 的**生产**依赖图**未变**。
- **可逆性**：高 —— 删掉这三行 + 删掉 `tests/architecture.rs` 即可。
- **类别**：decide-and-proceed。

### 偏离 2：子图**按名注册**而非**树内嵌**

- **偏离内容**：`NodeKind::SubGraph { name: NodeId }` +
  `GraphDefinition::subgraphs: BTreeMap<NodeId, GraphDefinition>`，
  而非 `NodeKind::SubGraph { graph: Box<GraphDefinition> }`。
- **理由**：**树内嵌的话，JSON 是树 —— 它字面上无法包含自己**，
  「子图递归检测」就成了**永不触发的死代码**（AC #8 形同虚设）。
  参考架构 §3.5.1 明确「子图 DSL 来自**冻结的** `definition.Subflows`」——
  那是**按名注册的表**；参考实现的递归检测用 `c.stack`
  （参考架构 §3.5.2），那个 stack 装的就是**正在展开的子图名**。
  按名注册后，递归判据是「**子图名**在展开路径上」——
  这与参考实现**同构**。
- **附带影响**：摊平后的节点键**带限定路径**（`sg/start`）——
  子图节点与父图节点可能同名，且这个路径**天然**就是参考架构 §3.5.3
  审批用的 `path`。
- **可逆性**：中 —— 数据形状变了，但 `GraphDefinition` 是**本任务新定义**的，
  没有既有调用方。
- **类别**：decide-and-proceed。

### 偏离 3：父图后继挂在**子图出口**而非容器

- **偏离内容**：子图摊平时，父图声明的后继节点的前驱是**子图出口**，
  不是**子图容器**。
- **理由**：容器只是子图的**入口** —— 它完成**不代表子图跑完**。
  挂在容器上会让父图后继在子图**还在跑**时就绪 = **读到未定义的状态**。
  有专门的测试 `parent_successor_is_gated_by_subgraph_exit_not_by_container` 守住，
  且该测试经**变异测试**验证过会红。
- **可逆性**：中。
- **类别**：decide-and-proceed。

### 偏离 4：状态大小上限是**独立判据函数**

- **偏离内容**：`check_state_size(size) -> Result<(), CompileError>`，
  而非编译期检查。
- **理由**：状态是**运行期**的东西 —— **编译期还没有状态**。
  参考架构 §3.5.2 的检查也在运行期（`graph.go:367-369`）。
  本任务给出**判据与错误**，NEX-39 的 `GraphState` 在写入时调用它。
  上限值（4 MiB）与错误映射（`BudgetExceeded`）都有测试。
- **可逆性**：高。
- **类别**：decide-and-proceed。

## 决策记录

### 类别：decide-and-proceed

- **决策**：图定义的输入是**结构化 JSON**，**不含** DSL 文本解析器。
- **依据**：DSL **文本语法**是产品选择（用户写什么），不是执行语义 ——
  它不阻塞 P6 的完成判据「审批恢复有测试」。
  参考架构 §5.3 局限 #5 已指出参考项目**同时存在两套工作流引擎**，
  Rust 版**只做一套**（编译式）—— 少一个前端不增加「两套路径」的维护负担。
  `crates/nexus-workflow/Cargo.toml` 已声明 `evalexpr`，脚手架本身预告了这个落点。
- **可逆性**：高 —— DSL 前端是**增量**（解析器 + 一层编译），不改动已定稿的图语义。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：展开上限（深度 8 / 节点 128 / 状态 4 MiB / 并发槽 4）**照抄**参考架构 §3.5.2。
- **依据**：这些上限在参考实现里是**防 DSL 炸弹**的既有设计，不是本仓库可以另定调的数字。
  超限映射到 `BudgetExceeded`（已定稿业务码），**不是** panic。
- **可逆性**：中 —— 上限值是常量，改动需同步测试与文档。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：定义哈希**复用** `nexus_core::canonical::canonical_hash_hex`，**不自己实现**规范化。
- **依据**：规范化规则（架构文档 §2.5.3）是**已定稿**的；
  `nexus-core` 里已有**实测过**的实现（附录 B：「规范化 JSON 键序无关、数组序敏感」）。
  再写一份 = 两份规范化 = 哈希漂移的入口。
  放在 `nexus-core` 正是因为 `nexus-workflow` 与 `nexus-execution` 互相不能依赖。
- **可逆性**：高 —— 换一个调用点。
- **影响面**：`crates/nexus-workflow/src/definition.rs`。

### 类别：decide-and-proceed

- **决策**：`NodeId` 定义在 `nexus-workflow` **内部**，**不进** `nexus-core::id`，
  也**不进**架构文档 §2.8.1 的 18 种 ID 清单。
- **依据**：§2.8.1 列的是**实体 ID 与技术标识 ID** —— 每一种都对应数据库里的
  一个**实体或外部系统标识**。`NodeId` **不对应任何实体**：
  它是**一次编译内部**的节点键，落在 `workflow_step_runs.node_key VARCHAR(64)`
  这样的**行内字段**里，不是某个实体的主键。
  ⚠️ `crates/nexus-core/tests/contract.rs` 的契约 1
  **硬断言** ID 类型数量恰好是 **18** —— 加进去会**破坏已定稿条款**。
  它**类型化**（而非裸 `String`）的收益仍然保留：
  编译期大量「键 ↔ 键」的比较与集合运算，传错类型编译不过。
- **可逆性**：中 —— 若将来它变成跨系统标识，需要**升格**为契约 ID
  （届时改 §2.8.1 + `contract.rs`）。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：环检测用 **Kahn**（入度计数），**不用** DFS 递归版。
- **依据**：DFS 版的环检测依赖**递归深度** —— 128 个节点的链就是 128 层栈。
  而「图有环」在这里是**预期**的失败路径（恶意输入），
  用递归去处理它等于**让恶意输入决定栈深度**。
  Kahn 版**没有**递归，栈深度是常数。
  它同时给出了 `AllPredecessor` 触发语义需要的**入度**信息。
- **可逆性**：中。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：`FrozenDefinition` 的字段**私有**，只能通过 `new` 构造。
- **依据**：「`hash` **一定**是 `canonical` 的哈希」这条不变量
  由**构造方式**保证，不靠运行时断言 —— 外部**造不出**不配对的实例。
- **可逆性**：高。
- **影响面**：`crates/nexus-workflow/src/definition.rs`。

## 变异测试记录

> 每个新代码至少落在三层验证之一（架构文档 §5）。
> 行为层的断言必须**真的有效** —— 「看起来显然」的不变量用**变异测试**验证断言会红。

**6 次注入**，全部被精确抓住：

| # | 注入 | 变红的测试 |
|---|---|---|
| 1 | `AllPredecessor` → `AnyPredecessor` | `node_waits_for_all_predecessors` |
| 2 | 去掉环检测 | `cycle_is_rejected_instead_of_hanging`、`self_loop_is_rejected`、`invalid_definition_maps_to_workflow_dsl` |
| 3 | 去掉递归检测 | `recursive_subgraph_is_rejected_instead_of_expanding_forever`、`mutually_recursive_subgraphs_are_rejected` |
| 4 | 父图后继改挂**容器**（不挂出口） | `parent_successor_is_gated_by_subgraph_exit_not_by_container` |
| 5 | 深度上限 8 → 100 | `each_expansion_limit_triggers_independently`、`limit_violations_map_to_budget_exceeded_not_panic` |
| 6 | 节点数上限 128 → 1000 | **第一次未被抓住** → 断言改用**字面量**后抓住 |

**注入 6 的教训（已写进架构文档 §2.5.5）**：
上限断言**引用常量**时是**恒真**的 —— 改了常量，断言跟着一起变，永远成立。
上限断言必须用**字面量**（`128` / `8` / `4 MiB`）钉死「照抄」这个事实。

**注入 4 的第一版也未被抓住**：先把出口的边**保留**、再**额外**挂一条到容器 ——
前驱是**集合**（AND 语义），多挂一条边只会**更严格**，不会提前触发。
真正要抓的错误是「**删掉**出口的边、**只**挂容器」。修正后抓住。

**注入 3 的额外发现**：去掉递归检测**没有**导致栈溢出 ——
深度上限（8）兜住了。递归检测的价值是**错误信息的精确性**（「loop → loop」），
不是防崩溃。**两者都要**：上限防崩溃，检测给可诊断的错误。

## 交付物

| 文件 | 说明 |
|---|---|
| `crates/nexus-workflow/src/definition.rs` | `FrozenDefinition` / `CanonicalJson` / `DefinitionHash` / `EngineVersion` |
| `crates/nexus-workflow/src/graph.rs` | `CompiledGraph` / `CompiledNode` / `NodeKind` / `NodeId` / `compile` / `check_state_size` |
| `crates/nexus-workflow/src/lib.rs` | crate 级文档（职责边界） + 模块声明 |
| `crates/nexus-workflow/tests/definition.rs` | 冻结定义的**行为层**测试（10 个） |
| `crates/nexus-workflow/tests/graph.rs` | 图编译的**行为层**测试（21 个） |
| `crates/nexus-workflow/tests/architecture.rs` | **结构层**断言（3 个，token 级扫描） |
| `docs/architecture/...rust-architecture-design.md` | 新增 §2.5.5「图编译」+ §2.5.2 的冻结定义落地记录 |
| `docs/specs/p6/tasks.md` | T-002 落地记录 |
| `docs/specs/p6/contracts/invariants.json` | P6-I09 ~ I15 升格为 `verified`（`verified_by` 对齐到真实测试名） |

## 遗留与后续

| 项 | 归属 |
|---|---|
| 节点**执行**（`NodeInvoke` trait / `GraphState` / 条件求值 / 模板渲染） | NEX-39（T-003） |
| **中断提案**与审批指纹 | NEX-40（T-004） |
| driver 与**版本三元组**校验（`definition_hash` / `engine_version` / `graph_schema`） | NEX-41（T-005） |
| **DSL 文本前端** | 决策 D15（OQ-2）—— 登记为 F4 |
| `check_state_size` 的**调用点**（`GraphState` 写入时） | NEX-39（T-003） |

> ⚠️ **本任务不阻塞后续任务**，但 T-003 依赖本任务产出的
> `CompiledGraph`（节点表 / 前驱集合 / `is_ready`）。
>
> ⚠️ **待人工评审** —— 按 `ai-sdlc-quality-gates.mdc`，
> AI 产出的改动需人工评审，**且** AI 作者的改动需**额外一名评审**。
> 本任务**未**触及架构文档 §6.3 的**已定稿条款**（新增 §2.5.5 是**追加**，
> §2.5.2 的落地记录是**记录**而非修订），故不触发契约级变更的加强门禁。
