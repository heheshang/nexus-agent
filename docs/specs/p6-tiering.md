# P6 工作流层 — 分层判定（S1）

> 判据见 `.cursor/rules/ai-sdlc-workflow.mdc` §3 与 `ai-sdlc-autonomy.mdc` §2.1。
> 本文件是**判定记录**，不是产出物 —— 产出物见 [`p6/`](p6/spec.md)（Tier 2）。

## 诉求

架构文档 §6.1 的 **P6 阶段**：

| 项 | 内容 |
|---|---|
| 阶段 | **P6 工作流** |
| crate | `nexus-workflow` + `nexus-execution` |
| 范围 | 租约、fencing、审批恢复 |
| 完成判据 | **租约、fencing、审批恢复有测试** |
| 当前状态 | 🟡 **未开工** —— `nexus-workflow` / `nexus-execution` 均为**空壳** |

> 本段记录的是**判定时点**的事实；进度以 `backlog/tasks/` 的任务状态为准。

## 判定时的事实核查（先查清「已有什么」，再谈「要做什么」）

这是本判定**最关键**的一步 —— 阶段表的措辞容易让人以为 P6 要从零造租约，
而仓库里**已经有**了。

### 已由 P2 交付（**不在 P6 的工作量里**）

| 已有物 | 位置 | 证据 |
|---|---|---|
| `RunLease` / `Fence` / `LeaseOutcome` 类型 | `crates/nexus-repo/src/lease.rs` | NEX-12 |
| 租约**领取**（`FOR UPDATE SKIP LOCKED` + `generation + 1`） | `crates/nexus-repo/src/leases.rs::claim` | NEX-12 |
| **围栏写入**（六个条件一处定义，`clock_timestamp()`） | `crates/nexus-repo/src/leases.rs::write_status_fenced` | NEX-12 |
| 围栏自增（`next_seq` / `state_version`） | `crates/nexus-repo/src/leases.rs::increment_counter_fenced` | NEX-12 |
| 三条 **trybuild 负向测试**（`RunLease` 不可伪造 / 无 `generation()` / 围栏写必须带租约） | `crates/nexus-repo/tests/compile_fail.rs` | NEX-12 |
| 围栏**变异测试**（删 `generation` 条件必须变红） | `crates/nexus-repo/src/lease.rs` 模块文档 + `tests/lease_fencing.rs` | NEX-12 |
| 幂等提交（`(owner_id, request_key)` 唯一）+ 事件序号原子分配 | `crates/nexus-repo/src/outbox_repo.rs` | NEX-13 |
| 引擎侧全部表 | `crates/nexus-infra/migrations/4_execution.up.sql`、`5_execution_support.up.sql` | NEX-9 |

### 确实**没有**的（P6 真正的工作量）

| 缺什么 | 证据 |
|---|---|
| `nexus-workflow` 的**全部内容** | `crates/nexus-workflow/src/lib.rs` 只有 `CRATE_NAME` |
| `nexus-execution` 的**全部内容** | `crates/nexus-execution/src/lib.rs` 只有 `CRATE_NAME` |
| 图引擎（冻结定义 → 编译 → 执行） | 无 `CompiledGraph` / `NodeInvoke` / `Interrupt` 的任何痕迹 |
| 操作幂等账本（`execution_operations` / `execution_attempts`） | 表存在（迁移 5），**无任何读写代码** |
| 检查点版本三元组（`execution_checkpoints`） | 表存在，**无任何读写代码** |
| 审批挂起 / 决策 / 恢复（`execution_approvals`） | 表存在，**无任何读写代码** |
| `workflow_runs` / `workflow_step_runs` 的写入 | 迁移 4 建了表，**没有任何 crate 写它** |
| `needs_attention` 人工核对（`resolve_operation`） | 无 |
| 取消时把在途 manual 操作标 `unknown` | 无 |
| `nexus-service` 的接线 | `crates/nexus-service/src/lib.rs` **7 行** |

> **结论（对 P6 范围的第一处裁剪）**：
> 阶段表写的「租约、fencing、审批恢复有测试」里，**租约与 fencing 已经由 P2 交付并有测试**。
> P6 **不重复造** 租约与 fencing。
> P6 真正缺的是 **① 自研图引擎**（`nexus-workflow`）+ **② 持久执行的其余部分**（`nexus-execution`）：
> 幂等账本、检查点、审批挂起与恢复、`needs_attention` 人工核对、取消语义、`workflow_runs` 独占写入。
>
> 但 P6 **必须**承接「租约/fencing 的**回归证据**」：
> 改依赖方向后（见 [OQ-1](#注册的开放问题)），围栏语义必须**在新形态下仍然成立** ——
> 测试不能因为「代码换了个 crate」就丢掉。

## 判据回顾

`ai-sdlc-workflow.mdc` §3 的判据：

> 存在合理工程师会就正确做法产生分歧 → 升到 RFC；
> 一个特性能拆成多个任务 → 升到 Spec；否则直接进 Task。

## 判定：Tier 2 · Spec

### 不是 Tier 1 · RFC —— 主干已定稿

逐项检查 P6 的技术选择：

| 技术选择 | 会不会有分歧 | 依据 |
|---|---|---|
| `service → execution` 的依赖形态 | ❌ **不会** | 架构文档 §4.3.2 **已定稿**：选项 A，`nexus-execution` 暴露 `RunSubmitter` trait |
| `workflow_runs` 的写入权归属 | ❌ **不会** | 架构文档 §4.2 差异 1 **已定稿**（2026-11-06 修订）：写操作由 `nexus-execution` 独占 |
| 取消语义 | ❌ 不会 | 架构文档 §2.1 **已定稿**（类型状态机，P1 已落地） |
| 事件清单与负载 | ❌ 不会 | 架构文档 §2.2.4.1 **已定稿**（12 个事件，P1 已落地） |
| 错误码表 | ❌ 不会 | 架构文档 §2.6.4.1 **已定稿**（42 个业务码，P1 已落地） |
| ID 类型表 | ❌ 不会 | 架构文档 §2.8.1 **已定稿**（18 种，P1 已落地） |
| 围栏条件的形状 | ❌ 不会 | 架构文档 §2.5.4 **已定稿**（六个条件），P2 已落地并变异验证 |
| 图引擎的展开上限 | ❌ 不会 | 参考架构 §3.5.2 **已定稿**（深度 8 / 节点 128 / 状态 4 MiB / DSL 1 MiB） |
| `replay_policy` 三种取值 | ❌ 不会 | 参考架构 §3.4.1 **已定稿**（`readonly` / `idempotent` / `manual`） |
| 审批指纹 `ArgsHash` 的构造 | ❌ 不会 | 参考架构 §3.5.3 **已定稿**（`sha256({path, prompt, parameters})`） |
| 检查点版本三元组 | ❌ 不会 | 参考架构 §3.4.3 **已定稿**（`definition_hash` / `engine_version` / `graph_schema`） |

**结论**：P6 的**主干**（自研图引擎 + 持久执行 + 审批恢复）在架构文档与参考架构里
**已经定稿**，不构成 RFC 级分歧。

> **唯一的例外**是 [OQ-1](#注册的开放问题)（引擎侧持久化的 crate 归属）——
> 它不是「技术选择不同」，而是**权威文档内部自相矛盾**（§4.1 的 crate 图画 `EXE → REPO`，
> 而 §4.2 差异 1 与 §4.3 的表格**修订前**说「`nexus-execution` 不依赖 `nexus-repo`」）。
> 这条**不升 RFC**，而是在 Spec 里登记为**带时间盒的决策**（类别 (b)），
> 理由见 OQ-1。
>
> ✅ **OQ-1 已由操作者于 2026-11-06 裁决为选项 B**（见 OQ-1）。
> 该裁决**修订了架构文档 §4.2 / §4.3 / §4.3.2 / §4.4 的已定稿条款** ——
> 因此 T-001 需要**人工评审**，AI 产出的改动需**额外一名评审**。

### 是 Tier 2 · Spec —— 不是 Task 层

P6 **明显**能拆成多个可独立评审的任务：

| 子能力 | 可独立评审 | 依赖 |
|---|---|---|
| 引擎侧持久化归属落地（新增依赖 + 修订已定稿条款） | ✅ | 无 |
| 冻结定义与图编译 | ✅ | 无 |
| 节点执行契约、状态、模板与条件求值 | ✅ | 图编译 |
| 中断提案与审批指纹 | ✅ | 图编译 |
| driver 与版本三元组校验 | ✅ | 持久化基座 |
| 操作幂等账本与三种 `replay_policy` | ✅ | driver |
| 检查点版本三元组 | ✅ | 持久化基座 |
| 审批挂起与恢复 | ✅ | 中断提案 + 持久化基座 |
| 取消与人工核对（`needs_attention`） | ✅ | 幂等账本 |
| 收口（架构文档同步 + 契约升格 + 故障注入） | ✅ | 全部 |

每个子能力都有**独立的**可二元测试的验收条件，
且**互不阻塞**（除了显式的依赖顺序）。

按 `ai-sdlc-workflow.mdc` §3 ——「一个特性能拆成多个任务 → 升到 Spec」。

## 操作者已作出的决策（开工前）

### 决策 OP-A —— 本轮**只交付 S0→S3**，不写实现代码

| 项 | 内容 |
|---|---|
| **类别** | (a) decide-and-proceed |
| **决策** | 本轮产出**分层判定 + Spec 三件套 + backlog 任务清单 + 门禁全绿**；**不写任何实现代码**、不 commit、不 push、不开 PR |
| **依据** | 操作者 2026-11-06 的明确指令；且 P6 触及 crate 边界与定稿条款，需要操作者先看到范围与判定 |
| **可逆性** | 高 —— 产出物全是文档，可直接修改 |
| **影响面** | `docs/specs/p6*`、`backlog/tasks/NEX-37..46`、`docs/specs/README.md`、`docs/README.md` |

### 决策 OP-B —— P6 沿用 **P5 的产出物形态**（Tier 2 三件套 + 契约）

| 项 | 内容 |
|---|---|
| **类别** | (a) decide-and-proceed |
| **决策** | 不新造产出物格式 —— `docs/specs/p6/` 用 `spec.md` + `tasks.md` + `contracts/`，`invariants.json` 的 `verified_by` 用 `status: "planned"` 并遵守 `docs/specs/README.md` 规则 3b |
| **依据** | P3/P4/P5 已形成**一致**的三件套形态与门禁；`scripts/check-specs.sh` 规则 1/2/3/3b/4/5 直接适用 |
| **可逆性** | 高 |
| **影响面** | `docs/specs/p6/` |

### 决策 OP-C —— 工作区已有的**未提交改动**不属于本轮产出

| 项 | 内容 |
|---|---|
| **类别** | (a) decide-and-proceed |
| **决策** | `crates/nexus-api/src/sse.rs` 的既有改动与 `crates/nexus-api/tests/zz_probe.rs` **不动、不提交、不 restore** |
| **依据** | 操作者明确说明那是别的工作；且 `git restore .` / `git reset --hard` 是仓库明令禁止的 |
| **可逆性** | n/a（不动作） |
| **影响面** | 无 |

## 复杂度打分（`ai-sdlc-autonomy.mdc` §2.1）

| 维度 | 分数 | 依据 |
|---|---|---|
| 契约影响 | **5** | 复用 42 个业务码、12 个事件、18 个 ID —— **不新增、不改值**；但要写 `workflow_runs` / `execution_*` 表，**跨 crate 数据契约** |
| 依赖面 | **7** | 触及 **crate 依赖方向**（§4.1 与 §4.3 的冲突，见 OQ-1）；`nexus-workflow` 与 `nexus-execution` 两个 crate + `nexus-execution → nexus-repo` 的新增依赖边 |
| 可逆性 | **5** | 纯新增代码可回滚；但**引擎侧持久化的 crate 归属**一旦落地就影响后续所有阶段的 import 路径 |
| 探索性 | **6** | 图引擎是**自研**（Rust 无 Eino 对等物，参考架构 §5.3 局限 #13 明说「自研图引擎是最大工作量」）；持久执行是「最难返工的部分」（参考架构 §5.4 第 6 项） |
| 并发/一致性 | **8** | **正中靶心**：分布式一致、fencing、幂等账本、审批挂起与恢复、在途操作的「结果未知」保留 |

**取最高分 = 8** → 高复杂度 → 路由策略 **`ai-assisted`**（人主导，AI 辅助）。

### 8.1 逐任务复杂度与路由（路由是**按任务**判定的）

| 任务 | 复杂度 | 路由 | 理由 |
|---|---|---|---|
| T-002 / T-003 / T-004（`nexus-workflow` 纯内存图引擎） | **4** | `ai-with-review` | 单 crate、无并发、无数据契约变更、路径已被参考架构 §3.5 定稿 |
| T-001（持久化归属落地 · 新增依赖 + 修订已定稿条款） | **8** | `ai-assisted` | 触及 crate 边界与**已定稿条款**；需人工评审 + 额外一名评审 |
| T-005 ~ T-009（持久执行本体） | **8** | `ai-assisted` | 分布式一致 + fencing + 幂等 |
| T-010（收口） | **3** | `ai-with-review` | 文档 + 契约升格 + 故障注入 |

> **明确不满足** `fully-autonomous` 的两个前提 ——
> 既不是低复杂度（8），也不在「本仓库默认的 Level 2（AI 产出 + 人工批准）」之外做自动合并。
> P6 的**合并**永远需要人工批准；`onTimeout: auto-approve` 在本仓库不使用。

## 注册的开放问题

### OQ-1 —— 引擎侧持久化的 **crate 归属**（`nexus-execution` vs `nexus-repo`）

**性质**：**已由操作者裁决**（2026-11-06）—— 选 **选项 B**。**不再是待决项**。

**当时的矛盾**（已消解）：权威文档**自相矛盾**。

| 位置 | 说法（**修订前**） |
|---|---|
| 架构文档 §4.1 crate 图 | `EXE --> REPO`（执行 → 数据，§4.1.1 的层次表也允许「执行 → 数据」） |
| 架构文档 §4.2 差异 1 | 「`nexus-execution` **不依赖** `nexus-repo`」，「`nexus-repo` **不允许**包含 `workflow_runs` 的写操作」 |
| 架构文档 §4.3 目标态表 | `nexus-execution → nexus-repo`：设计目标「不依赖」，当前「不依赖」，✅ |
| `scripts/check-deps.sh` 的 `deps_of` | `nexus-execution` 的内部依赖表**不含** `nexus-repo`（规则 7 强制与 `Cargo.toml` 一致） |
| `crates/nexus-execution/Cargo.toml` | **没有** `nexus-repo` |
| 现状代码 | `nexus-repo` **持有** `execution_runs` 的租约/fencing/outbox 写入（`lease.rs` / `leases.rs` / `outbox_repo.rs`） |

**为什么不能靠「实现里顺手定」**：这是 **crate 边界** —— §6.3 已定稿条款清单里的一项。
在实现里选一边 = 把一条**未经决策的**架构选择固化成既成事实。

**两个候选**：

| 选项 | 做法 | 代价 | 与权威文档的关系 |
|---|---|---|---|
| **A** | 把引擎侧持久化（`lease` / `leases` / `outbox_repo` + 执行侧行类型）**迁移**到 `nexus-execution`，`nexus-execution` 不依赖 `nexus-repo` | 大（模块 + 测试迁移）；**无新依赖边** | 与 §4.2 差异 1、§4.3 表格、`check-deps` 现状**一致**；与 §4.1 的 `EXE --> REPO` 箭头不一致 |
| **B** | 保留 `nexus-repo` 的引擎侧仓储，给 `nexus-execution` 加 `nexus-repo` 依赖 | 小（一行 `Cargo.toml` + 一行 `deps_of`）；**需修订已定稿条款** | 与 §4.1 crate 图 + §4.1.1 层次一致；与 §4.2 差异 1、§4.3 表格**不一致**（需架构文档同步修订） |

#### 操作者的裁决 —— **选项 B**（2026-11-06）

**决策 D12 = 选项 B**：**不迁移**。
`nexus-execution` **新增**对 `nexus-repo` 的依赖；
引擎侧持久化能力（`lease` / `leases` / `outbox_repo` + 引擎侧行类型）
**留在** `crates/nexus-repo/`。

**类别判定**：**(b) timeboxed**。

- **推导顺序**（`ai-sdlc-decision-authority.mdc` §2.1，取最严）：
  1. **可逆性** —— 依赖边是**加法**，文档修订可 `git revert` → 可低成本回滚，
     这本身**不构成** (b) 的充分理由；
  2. **影响面** —— 修订的是**架构文档的已定稿条款**（§4.2 / §4.3 / §4.3.2 / §4.4），
     且改动的是**crate 依赖方向** → **至少 (b)**；
  3. **控制变更** —— 「`workflow_runs` 写入权唯一」这条**控制**在修订后**仍然成立**
     （判定手段从「类型归属」改为「写操作归属」，强度**不降**）——
     **收紧不算削弱**，不额外升级；
  4. **operator-only** —— 不涉及法律、许可、金钱、凭据 → **不是 (c)**。

**依据**：

1. **§4.1 的 crate 图本就画着 `EXE --> REPO`**，
   §4.1.1 的层次表也允许「执行 → 数据」—— 选 B 与**目标态总览一致**。
2. **围栏条件「只有一处定义」的纪律不受影响** ——
   实现仍在 `crates/nexus-repo/src/lease.rs` 一处，
   NEX-12 的变异测试**不用搬家、不会失效**。
3. **迁移是纯 churn**：迁移要重跑全部围栏测试、动 `nexus-repo` 里其他模块的依赖，
   收益只是「让实现与某一条表述对齐」。
4. 选 B 要改的是**文档表述**，不是**代码位置** —— 成本远低于迁移一个已被变异测试焊死的模块。

**「写入权唯一」这条控制在选项 B 下的新落点**：

| 项 | 原（选项 A 下） | 新（选项 B 下） |
|---|---|---|
| 条款 | §4.2 差异 1 / §4.3.2 约束 2 | §4.2 差异 1 / §4.3.2 约束 2（**已修订**） |
| 判定手段 | `nexus-repo` **不得引用** `workflow_runs` 的**类型** | `workflow_runs` / `workflow_step_runs` 的**写操作**（`INSERT` / `UPDATE` / `DELETE`）**只允许**出现在 `nexus-execution` |
| 对应测试 | `workflow_runs_appears_only_in_execution_owned_crates`（允许集断言） | **已落地**（NEX-37 / T-001）：改为「写操作归属」判定 —— `nexus-core` 的 `workflow_runs_appears_only_in_execution_owned_crates` + `nexus_repo_does_not_touch_workflow_runs` + `nexus-repo` 的 `nexus_repo_must_not_write_execution_owned_tables` |
| 强度 | 可被「把类型下沉到 `nexus-repo`」绕过 | 管住**任何 crate 的任何写操作** —— **不降** |

**时间盒与自主回退**（类别 (b) 的强制内容）：

| 项 | 内容 |
|---|---|
| **时间盒截止** | **2026-11-13** |
| **时间盒到期前的行为** | **不实施** T-001；其余任务（T-002 ~ T-010）照常推进 |
| **无人干预时的自主回退** | 按**选项 B** 执行 T-001 —— 依赖边是**加法**、文档修订可 `git revert`，且与 §4.1 的目标态图一致 |
| **影响面** | `crates/nexus-execution/Cargo.toml`、`scripts/check-deps.sh` 的 `deps_of`、`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`（§4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 / §4.4 规则 4 / §5.3 / §6.3） |

> ⚠️ **本项修订了已定稿条款** —— T-001 需要**人工评审**，
> 且 AI 产出的改动需**额外一名评审**（`ai-sdlc-quality-gates.mdc`）。

### OQ-2 —— 图定义的**输入形态**：结构化图 vs DSL 文本

**性质**：**非阻塞性**。

参考实现有 DSL 文本前端（`internal/workflow/dsl.go`，限制 1 MiB），
但**条件表达式方言**在 Go（`expr-lang/expr`）与 Rust（`evalexpr`）之间**不兼容**
（参考架构 §3.5.1 原文：「需要改写条件表达式 DSL，或在 DSL 层做兼容层」）。

**采用的假设**：P6 的图编译**输入是结构化的 JSON 图定义**（节点 / 边 / 分支条件 / 子图引用），
**不含** DSL 文本解析器。条件表达式用 `evalexpr` 求值，语义限定为**布尔判断**，
变量从图状态取值。

**为什么这样假设**：

1. DSL **文本语法**是**产品选择**（用户写什么），不是执行语义 ——
   它不阻塞「审批恢复有测试」这个完成判据。
2. 参考架构 §5.3 局限 #5 已指出参考项目**同时存在两套工作流引擎**，
   Rust 版**只做一套**（编译式）。少一个前端不增加「两套路径」的维护负担。
3. `crates/nexus-workflow/Cargo.toml` 已声明 `evalexpr` —— 脚手架本身就预告了
   「条件求值在 `nexus-workflow`、用 `evalexpr`」这个落点。

**处置**：DSL 文本前端登记为**后续任务**，触发条件 =「产品侧需要用户手写工作流定义」。
P6 的 Spec §1.2 显式把它划出范围。

### OQ-3 —— `replay_policy` 是否按参考架构的「可以简化」只做 `readonly`

**性质**：**非阻塞性**。

参考架构 §5.2 的 Phase 9 行写「**可以简化**：可以先只做 `readonly` 一种 `replay_policy`」。

**采用的假设**：P6 **三种全做**（`readonly` / `idempotent` / `manual`）。

**理由**：架构文档附录 C 的未验证项里，`UnknownOutcome` 分类被标注为
「🟡 **部分验证**（P3：`nexus-llm` 断言超时不被自动重试；但『端到端不重复副作用』
**需要 P6 的持久执行来验证**）」。

「不重复副作用」的**唯一**实现路径就是 `manual` 策略 ——
把在途操作标 `unknown`、**不自动重试**、转 `needs_attention` 等人工核对。
只做 `readonly` 的话，附录 C 这一项**无法在 P6 关闭**。

**处置**：D14 记录该决策。若操作者要「先只做 `readonly`」，
则 T-006 缩小、T-009（取消与人工核对）**后移**，且附录 C 的 `UnknownOutcome` 项**保持未验证**。

### OQ-4 —— 版本不匹配 / 审批指纹不匹配映射到哪个业务码

**性质**：**非阻塞性**。

架构文档 §6.3 已定稿 42 个业务码，**没有**「版本不匹配」专用码
（参考实现的 `ErrVersionMismatch` 在 Rust 版**无对应码**）。

**采用的假设**：统一映射到 **`RunConflict`（30007，HTTP 409，终止型）**。

**理由**：

1. §6.3 是**前后端契约**，新增码 = 破坏性变更，需要前后端同步 —— 不属于 P6 的裁量范围。
2. 这三类情形（`definition_hash` 不一致 / `engine_version` 不一致 /
   `graph_schema` 不一致 / 审批 `args_hash` 不一致）的共同点是
   「**当前操作与已固化的记录冲突**」—— `RunConflict` 语义吻合。
3. `RunConflict` 已经是**终止型**（`nexus-repo` 的 `LeaseLost` 已用它，
   见 `crates/nexus-repo/src/lease.rs`）—— 重试无意义，符合语义。

**处置**：D16 记录该决策；P6 的错误码白名单**不含**任何新码。

### OQ-5 —— `workflow_runs` 的外键依赖（`workflow_id` / `definition_release_id`）

**性质**：**非阻塞性**。

迁移 4 里 `workflow_runs.workflow_id` 是 `NOT NULL REFERENCES workflows(id)`，
`definition_release_id` 引用 `workflow_versions(id)`。
但**工作流定义的 CRUD 不在 P6**（P6 只做执行侧），`workflows` 表由 `nexus-repo` 的
`WorkflowRow` / `WorkflowVersionRow` 描述，**尚无写入方**。

**采用的假设**：P6 **不交付** `workflows` / `workflow_versions` 的写入；
P6 的 `workflow_runs` 写入路径要求调用方**传入已存在的 `workflow_id`**，
集成测试**自己造种子数据**（参考架构 §2.5 的 Go 版流程本来就是
「提交时带 `workflow_id`」，参考架构 §5.2 Phase 0 的坑也提到
「demo 自己 INSERT（注意外键链 `runs → workflows → users`）」）。

**处置**：D13 记录该边界；T-008 的 AC 要求「审批恢复测试**自带**种子数据，
不依赖 `workflows` 的写入方」。

### OQ-6 —— 审批决策的**HTTP 端点**是否在 P6

**性质**：**非阻塞性**。

参考实现的审批端点是 `POST /api/v1/workflows/runs/:id/approve`（属 service 层）。
P6 的 `nexus-service` 是 **7 行空壳**，而 P5 已明确「业务编排属 `nexus-service`」。

**采用的假设**：P6 **只交付审批的仓储语义与恢复语义**（挂起 / 决策 / 恢复 / 过期 / 重复决策幂等），
**不交付** HTTP 端点、不接线 `nexus-service`。

**处置**：D17 记录该边界；审批端点登记为后续任务（触发条件 = `nexus-service` 落地业务编排）。

### OQ-7 —— P6 的 run 指标口径是否在本阶段

**性质**：**非阻塞性**。

架构文档 §5.6 的性能与指标基线属 **P9**。

**采用的假设**：P6 **不交付** run 生命周期的 Prometheus 指标口径；
只保证**日志与事件**里能看出 run 的状态迁移（事件走 `execution_events` 表，已有）。

**处置**：D18 记录该边界。

## 判定结论

| 项 | 结论 |
|---|---|
| **层级** | **Tier 2 · Spec** |
| **产出物** | `docs/specs/p6/spec.md` + `tasks.md` + `contracts/` |
| **决策编号** | 从 **D12** 起续（P5 用到 D11） |
| **复杂度 / 路由** | 阶段级 8 / `ai-assisted`；**逐任务**路由见 §8.1 |
| **OQ** | OQ-1（**已由操作者裁决**，2026-11-06，选选项 B）；OQ-2 ~ OQ-7（非阻塞性 —— 记录假设，继续） |

## 相关文件

| 文件 | 作用 |
|---|---|
| [`p6/spec.md`](p6/spec.md) | P6 的契约（Tier 2 产出物） |
| [`p6/tasks.md`](p6/tasks.md) | 任务索引与依赖图 |
| [`p6/contracts/invariants.json`](p6/contracts/invariants.json) | 不变量契约（`status: planned`） |
| [`p6/contracts/error-codes.json`](p6/contracts/error-codes.json) | P6 允许触碰的业务码白名单 |
| [`p5/spec.md`](p5/spec.md) | 前序阶段的契约（协议层边界，P6 不重复） |
| [`../architecture/2026-11-04-nexusagent-rust-architecture-design.md`](../architecture/2026-11-04-nexusagent-rust-architecture-design.md) | 架构与契约的**唯一权威** |
