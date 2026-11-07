# P6 工作流层 — Tasks

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 契约见 `spec.md`；机器可读的不变量见 `contracts/`。
>
> 布局遵循 spec-kit `v0.8-headings`（每个任务一个 `### T-NNN — <title>` 标题）。
> `taskId` 形如 `T-NNN`，与 `backlog/tasks/` 下的 `NEX-nn` 一一对应。

## 依赖图

```
T-001（持久化归属落地：新增 nexus-execution → nexus-repo 依赖 + 修订已定稿条款，OQ-1 timeboxed）
  ├──> T-005（driver 与版本三元组）──> T-006（操作幂等账本）──> T-008（审批挂起与恢复）
  │                                      └──> T-009（取消与人工核对）
  └──> T-007（检查点）

T-002（冻结定义与图编译）──> T-003（节点执行契约与图状态）──> T-004（中断提案与审批指纹）
T-003 ──> T-006
T-004 ──> T-008
T-010（收口）依赖全部
```

**可并行**：`T-002` / `T-003` / `T-004`（纯内存图引擎，`nexus-workflow` 内）
与 `T-001` / `T-005` / `T-007`（持久化，`nexus-execution` 内）**互不阻塞**。

**必须串行**：`T-006` → `T-008` / `T-009`（审批与取消都依赖幂等账本的语义）；
`T-008` 依赖 `T-004`（中断提案）与 `T-005`（driver）；
`T-010` 依赖全部。

> ⚠️ **T-005 ~ T-009 的持久化落点**：引擎侧持久化能力**留在** `nexus-repo`
> （决策 D12 选项 B）—— 这五个任务在 `nexus-execution` 里写**执行语义**，
> **经由** `nexus-repo` 的仓储 API 落库，**不自己**拼 SQL。

## Tasks

### T-001 — 引擎侧持久化归属落地（新增依赖 + 修订已定稿条款）

**P6 的第一块地基**：把「谁拥有引擎侧持久化」这件事**落到唯一的 crate**。

架构文档 §4.1 的 crate 图画着 `EXE --> REPO`，而 §4.2 差异 1 与 §4.3 表格
**原先**说「`nexus-execution` 不依赖 `nexus-repo`」—— **权威文档自相矛盾**。

本任务按**决策 D12（选项 B，操作者 2026-11-06 裁决）**落地：
**不迁移**。`nexus-execution` **新增**对 `nexus-repo` 的依赖，
引擎侧持久化能力（`lease` / `leases` / `outbox_repo` + 引擎侧行类型）
**留在** `crates/nexus-repo/`。

> ⚠️ **本任务修订了架构文档的已定稿条款**（§6.3 第 5 项「crate 边界与 trait 归属」）
> —— 涉及 §4.2 差异 1、§4.3 目标态表、§4.3.2 约束 2、§4.4 规则 4。
> 按 `ai-sdlc-quality-gates.mdc`，契约级变更必须走**人工评审**，
> 且 **AI 产出的改动需额外一名评审**。
>
> ⚠️ **本任务同时是决策 D12 的落点**，D12 是**类别 (b) timeboxed**：
> 时间盒截止 **2026-11-13**；到期前**不实施**；
> 无人干预时的自主回退 = 按选项 B 执行（依赖边是加法、文档修订可 `git revert`）。

对应 backlog 任务：`backlog/completed/NEX-37 - feat(execution) 引擎侧持久化归属落地 —— 新增依赖与已定稿条款修订.md`
依赖：无
对应设计条款：架构文档 §4.1、§4.2 差异 1、§4.3 目标态表、§4.3.2 约束 2、§4.4 规则 4、§6.3

- AC: `crates/nexus-execution/Cargo.toml` **新增** `nexus-repo.workspace = true`；`scripts/check-deps.sh` 的 `deps_of nexus-execution` **新增** `nexus-repo`
- AC: 引擎侧持久化模块**留在** `nexus-repo` —— `crates/nexus-repo/src/lease.rs` / `leases.rs` / `outbox.rs` / `outbox_repo.rs` **继续存在**（**不迁移**、**不删除**）
- AC: 架构文档 §4.2 差异 1 已修订为「`nexus-execution` 依赖 `nexus-repo`，但 `workflow_runs` 的**写入权**仍由执行层独占」
- AC: 架构文档 §4.3 目标态表的 `nexus-execution → nexus-repo` 行已修订（设计目标改为「**依赖**」，并注明 P6 · D12 选项 B 的裁决）
- AC: 架构文档 §4.3.2 约束 2 已修订为「`workflow_runs` / `workflow_step_runs` 的**写操作**只允许发生在 `nexus-execution`」
- AC: 架构文档 §4.4 已新增**规则 4**（`nexus-execution → nexus-repo` 允许；`nexus-repo → nexus-execution` 禁止）
- AC: 架构文档 §5.3 的 `workflow_runs` 那一行与 §6.3 的「crate 边界与 trait 归属」条目**同步更新**（不留半截矛盾）
- AC: 架构测试 `workflow_runs_appears_only_in_execution_owned_crates` 已修订 —— 判定从「**类型**只允许出现在某处」改为「**写操作**只允许出现在 `nexus-execution`」（`nexus-infra` 只允许建表 DDL）
- AC: 新增架构测试断言 `nexus-repo` **不得**依赖 `nexus-execution`（反向会成环）
- AC: 架构测试断言 `nexus-execution` **不依赖** `nexus-service` / `nexus-api` / `nexus-cli` **仍然成立**（不因新增依赖而放宽）
- AC: 架构测试断言 `nexus-model` **无**数据库 schema 痕迹 **仍然成立**（不因新增依赖而放宽）
- AC: `crates/nexus-execution` 的**围栏写**必须**经由** `nexus-repo` 的带租约 API —— 执行层**不自己**拼围栏 SQL（结构层断言）
- AC: NEX-12 的三条 **trybuild 负向测试**（`RunLease` 无 `generation()` / 不可直接构造 / 围栏写必须带 `&RunLease`）**留在** `crates/nexus-repo/tests/`，且**仍然编译失败**
- AC: 围栏的**变异测试仍然会红** —— 删掉 `generation` 条件时 `lease_fencing` 的对应测试**必须失败**（断言被改弱即为回归）
- AC: `Fence::CONDITIONS_TEMPLATE` 的**文本逐字节不变** —— 围栏条件的六个子句与绑定顺序**不改**
- AC: `bash scripts/check-deps.sh` 通过（规则 7：`deps_of` ↔ `Cargo.toml` 一致）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

> ✅ **T-001 已落地**（2026-11-06，NEX-37）——
> 依赖边、文档修订与架构断言**全部**完成；`docs/specs/p6/contracts/invariants.json`
> 的 P6-I01 ~ I08 / I49 / I50 / I51 共 **11 条**不变量已由 `planned` 升格为 `verified`。
>
> ⚠️ **剩余**：**人工评审**（AC #17）—— 触及**已定稿条款**（架构文档 §6.3 第 5 项），
> 需人工批准**且** AI 产出的改动需**额外一名评审**。
> 该门槛**阻塞合并**，不阻塞后续任务的推进。

### T-002 — 落地冻结定义与图编译

**`nexus-workflow` 的地基**：定义冻结 + 结构化图 → 可执行图。

对应 backlog 任务：`backlog/tasks/NEX-38 - feat(workflow) 落地冻结定义与图编译.md`
依赖：无
对应设计条款：架构文档 §2.5.2、§2.5.3；参考架构 §3.5.1、§3.5.2

- AC: `crates/nexus-workflow/src/definition.rs` 定义 `FrozenDefinition` —— 含 `canonical` / `hash` / `engine_version` **三个**字段
- AC: 定义哈希用 `nexus_core::canonical::canonical_hash_hex` —— **不自己实现**规范化
- AC: 规范化键序无关 —— 同一份定义、键序不同，`hash` **相同**
- AC: `crates/nexus-workflow/src/graph.rs` 定义 `CompiledGraph` —— 含 `nodes` / `roots` / `name`
- AC: 节点种类**全部**存在：`Lambda` / `Branch` / `SubGraph` / `Parallel` / `Human` / `Tool`
- AC: 触发语义是 `AllPredecessor` —— 节点**所有**前驱完成后才触发（测试：两个前驱只完成一个时节点**不**执行）
- AC: 编译做**拓扑排序 + 入度计数**；图有**环**时编译**失败**（不是死循环）
- AC: **子图递归检测** —— 编译时用 `HashSet<NodeId>` 检测自引用，递归 → **错误**（不是无限展开）
- AC: 展开上限**三个各自生效**：嵌套深度 > **8** → 错误；展开节点总数 > **128** → 错误；状态大小 > **4 MiB** → 错误
- AC: 超限映射到 `ErrorCode::BudgetExceeded`（30006），**不是** panic
- AC: `crates/nexus-workflow/src/lib.rs` 的 crate 级文档写明「本 crate 负责图引擎 / 不负责持久化」
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

> ✅ **T-002 已落地**（2026-11-07，NEX-38）——
> `crates/nexus-workflow/src/definition.rs`（`FrozenDefinition` 三件套）
> + `crates/nexus-workflow/src/graph.rs`（`CompiledGraph` / `NodeKind` 六种 / `compile`）
> + 三个测试文件（`tests/definition.rs` 10 个 / `tests/graph.rs` 21 个 / `tests/architecture.rs` 3 个）。
>
> ⚠️ **三处偏离任务 AC 的原始措辞**，均已记录决策：
>
> 1. **子图按名注册**（`GraphDefinition::subgraphs`）而非**树内嵌** ——
>    树内嵌的话 JSON 是**树**、**字面上无法包含自己**，
>    「递归检测」就是**永不触发的死代码**。
>    参考架构 §3.5.1 明确子图来自**冻结的** `definition.Subflows`（按名注册的表），
>    参考实现的递归检测用 `c.stack`（参考架构 §3.5.2）——
>    那个 stack 装的就是**正在展开的子图名**。
>    递归判据因此是「**子图名**在展开路径上」。
> 2. **父图后继挂在子图出口**而非容器 ——
>    挂在容器上是**错的**：容器只是子图的**入口**，它完成**不代表子图跑完**。
>    有专门的测试 `parent_successor_is_gated_by_subgraph_exit_not_by_container` 守住。
> 3. **状态大小上限**是**独立判据函数** `check_state_size` 而非编译期检查 ——
>    状态是**运行期**的东西，编译期还没有状态。
>    错误映射（`BudgetExceeded`）与上限值（4 MiB）都有测试。
>
> **变异测试 6 次注入**，全部被精确抓住。其中「节点数上限 128 → 1000」
> **第一次未被抓住** —— 因为断言引用了**同一个常量**（恒真）。
> 上限断言改用**字面量**后抓住。这条教训写进了架构文档 §2.5.5。
>
> ⚠️ **`Cargo.toml` 新增了 `syn`（dev-dependency）** ——
> DoR 原写「不修改」，但**结构层断言**需要 token 级扫描器（`syn`），
> 且它必须**剥离注释**（文档注释里恰恰要出现 `sqlx` 一词）。
> dev-dependency **不影响生产依赖方向**。

### T-003 — 落地节点执行契约与图状态

**节点是唯一的扩展点**：契约（trait）+ 状态容器 + 条件求值 + 模板渲染。

对应 backlog 任务：`backlog/tasks/NEX-39 - feat(workflow) 落地节点执行契约与图状态.md`
依赖：T-002
对应设计条款：架构文档 §2.3.2、§2.3.3；参考架构 §2.5、§3.5.4

- AC: `crates/nexus-workflow/src/node.rs` 定义 `NodeInvoke` trait —— `invoke` 返回 `Result<Option<NodeOutput>, NodeError>`
- AC: `NodeError` 是**三分类** enum：`Retryable` / `Permanent` / `Interrupt`（不是字符串）
- AC: `Ok(None)` 表示「**无产出但正常**」，与 `Err` 区分
- AC: `crates/nexus-workflow/src/state.rs` 定义 `GraphState`（键 → JSON 值）；状态大小 > **4 MiB** → **错误**（不是截断）
- AC: 并发槽用 `Semaphore`（默认 **4**）；节点任务用 `acquire_owned()`
- AC: 条件求值用 `evalexpr`，结果**必须**是布尔；非布尔 → **错误**
- AC: 条件求值**失败** → **错误**，**不是** `false`（否则走错分支）
- AC: 条件里的变量**不存在** → **错误**，不是 `false`
- AC: 模板渲染：整串是 `{{...}}` 且**只出现一次** → **保留原始类型**（数字仍是数字）
- AC: 模板渲染：插值结果**仍含** `{{` → **错误**（未解析的工具参数模板）
- AC: 模板渲染：嵌套 map / slice **递归**渲染
- AC: `crates/nexus-workflow` 的源码**不含** `Box<dyn` 于节点执行热路径 —— `Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**处（结构层断言）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-004 — 落地中断提案与审批指纹

**审批的「哪一次调用、哪些参数」绑定成不可变指纹** —— 参数一变，审批自动失效。

对应 backlog 任务：`backlog/tasks/NEX-40 - feat(workflow) 落地中断提案与审批指纹.md`
依赖：T-002、T-003
对应设计条款：参考架构 §3.5.3、§2.6（审批能力走类型隔离）

- AC: `crates/nexus-workflow/src/interrupt.rs` 定义 `Interrupt` —— 含 `path` / `args_hash` / `prompt` / `resume_key`
- AC: `args_hash = sha256({path, prompt, parameters})` —— 三个输入**全部**参与
- AC: **参数变化 → 指纹不同** —— 同一 `path` / `prompt`、不同 `parameters` → `args_hash` **不同**
- AC: **同一** `path` / `prompt` / `parameters` → `args_hash` **相同**（指纹是确定性的，不是随机的）
- AC: 节点类型 `Human` → 产生中断提案；工具节点且**危险** → 产生中断提案
- AC: 中断提案经 `NodeError::Interrupt` **从节点返回**（不是由引擎旁路生成）
- AC: 恢复时 `decision.args_hash != proposal.args_hash` → **拒绝恢复**（参数已变）
- AC: 审批能力**不走 context / 不走参数传递** —— 用**类型区分**（typestate）：未授权作用域与已授权作用域是**不同类型**，已授权类型的构造函数**模块私有**
- AC: `crates/nexus-workflow` 的源码**不含** `context.` / `ctx.set(` 形态的审批能力传递（结构层断言）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-005 — 落地执行 driver 与版本三元组校验

**driver 是「领取 → 校验 → 编译 → 执行」的骨架**。

对应 backlog 任务：`backlog/tasks/NEX-41 - feat(execution) 落地执行 driver 与版本三元组校验.md`
依赖：T-001
对应设计条款：架构文档 §2.5.4；参考架构 §2.5、§3.4.3

- AC: `crates/nexus-execution/src/driver.rs` 定义 `Driver::execute` —— 领取后**先校验**版本三元组，再编译，再执行
- AC: 版本三元组 = `(definition_hash, `engine_version`, `graph_schema)`；**三者任一**不同 → **拒绝续跑**
- AC: 版本不匹配映射到 `ErrorCode::RunConflict`（30007，终止型）—— **不新增**业务码
- AC: 围栏写与业务写在**同一个事务**里 —— 围栏检查与业务更新之间**不跨**事务边界（参考架构 Phase 9 必抄不变量 #9）
- AC: run 状态机的终态集合**全部**存在：`succeeded` / `failed` / `needs_attention` / `waiting_approval` / `canceled` / `retry_wait`
- AC: `needs_attention` **不合并**进 `failed` —— 两者的**落库状态**不同（行为层断言查落库值，不是查枚举定义）
- AC: `nexus-execution` **不依赖** `nexus-service` / `nexus-api` / `nexus-cli`（结构层断言）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-006 — 落地操作幂等账本与三种 replay_policy

**「同一路径不重复执行」是持久执行的核心不变量**。

对应 backlog 任务：`backlog/tasks/NEX-42 - feat(execution) 落地操作幂等账本与三种 replay_policy.md`
依赖：T-003、T-005
对应设计条款：架构文档 §2.6.3；参考架构 §3.4.1

- AC: `crates/nexus-execution/src/operation.rs` 定义 `ReplayPolicy` enum —— `ReadOnly` / `Idempotent` / `Manual`，**序列化到 DB 用 `&'static str`**（`'readonly'` / `'idempotent'` / `'manual'`）
- AC: `begin_operation` 在**一个事务**里完成：查既有 operation → 判定 → 创建或重置 → 写 `execution_attempts` 一行
- AC: 既有 operation 的 `args_hash` / `kind` / `replay_policy` **任一**不同 → **冲突**（不复用旧结果）
- AC: 既有 operation `Succeeded` → **幂等命中**，返回旧输出（不重新执行）
- AC: 既有 operation `unknown` → **要求人工核对**，**不重放**
- AC: 既有 operation `Running` 且 `generation` 与当前一致 → **冲突**（并发同节点）
- AC: `manual` 策略且上次执行者丢失 → 该 operation 与 attempt 标 `unknown` + 追加 `operation_unknown` 事件
- AC: `attempt` 达上限 → **尝试耗尽**失败；**上限是配置项**，改配置即改上限（**不是**硬编码常数）
- AC: `attempt` 上限可从 `nexus-config` 的配置栈读取，缺失时用默认值
- AC: `complete_operation` 在**同一事务**里更新 operation 与 attempt，并追加事件
- AC: **三种策略各自**有测试 —— 不是只测 `readonly`
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-007 — 落地检查点版本三元组

**检查点是续跑的「断点」** —— 三元组不一致就**拒绝**，不静默重放。

对应 backlog 任务：`backlog/tasks/NEX-43 - feat(execution) 落地检查点版本三元组.md`
依赖：T-001
对应设计条款：参考架构 §3.4.3；架构文档 §2.5.2

- AC: `crates/nexus-execution/src/checkpoint.rs` 定义检查点的读写
- AC: 检查点绑定**三元组** `(definition_hash, `engine_version`, `graph_schema`)` —— 三个都存
- AC: 读：比对**既有 run 的版本** 与 **当前进程常量**；任一不同 → 版本不匹配（`RunConflict`）
- AC: 写：`INSERT ... ON CONFLICT DO UPDATE` **带三元组条件**；影响行数 ≠ 1 → 版本不匹配
- AC: 同一 `run_id` + `checkpoint_id` **覆盖**写（复合主键），不是插入重复行
- AC: 负载大小上限**下沉到数据库**（`octet_length(payload) <= 16777216`）—— 应用层**不重复**这个约束
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-008 — 落地审批挂起与恢复

**P6 完成判据的第三项**：审批恢复有测试。

对应 backlog 任务：`backlog/tasks/NEX-44 - feat(execution) 落地审批挂起与恢复.md`
依赖：T-004、T-005
对应设计条款：参考架构 §2.6、§3.5.3

- AC: `crates/nexus-execution/src/approval.rs` 实现 `suspend` —— 写 `execution_approvals`（`decision = 'pending'`）并把 run 置 `waiting_approval`
- AC: `decide` 校验 `args_hash` 一致且状态仍为 `pending`
- AC: `approved` → run 置 `queued`（**可续跑**）
- AC: `rejected` → run 置 `canceled` 且 `generation + 1`
- AC: **重复的同决策** → **幂等**返回，不报错（不是冲突）
- AC: **冲突的决策**（同 interrupt 不同决策） → 冲突错误
- AC: **过期**的审批（`expires_at <= clock_timestamp()`） → 拒绝写入决策（冲突错误）
- AC: 挂起时发现已有审批的 `args_hash` / `path` 与本次不同 → 版本不匹配（`RunConflict`）
- AC: 恢复时二次校验 `decision.args_hash == proposal.args_hash` —— 不一致**拒绝恢复**
- AC: 审批挂起与恢复**各自**有集成测试（需要真实 PostgreSQL 17）
- AC: 审批恢复的集成测试**自带种子数据**（`users` / `workflows` / `workflow_versions`）—— **不依赖** `workflows` 的写入方（决策 D13）
- AC: P6 **不交付**审批的 HTTP 端点（决策 D17）—— `crates/nexus-execution` 源码**不含** axum / `Router` 引用
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-009 — 落地取消语义与人工核对

**「不可知即人工核对」** —— 取消保留不确定性，核对必须带 evidence。

对应 backlog 任务：`backlog/tasks/NEX-45 - feat(execution) 落地取消语义与人工核对.md`
依赖：T-006
对应设计条款：架构文档 §2.6.3、§2.1；参考架构 §3.4.4、§3.4.5

- AC: `cancel` 在**一个事务**里：扫描 `status = 'running'` 的 operation → 分类更新 → 更新 run
- AC: 在途 `manual` 操作 → 标 `unknown` + 追加 `operation_unknown` 事件（reason = `canceled_in_flight`）
- AC: 在途**其他**策略的操作 → 标 `abandoned`
- AC: run 本身 → `status = canceled`、`generation + 1`、清租约
- AC: **顺序不可调换** —— 先保留不确定性，**再** fence 执行者（测试断言事件写入与 run 更新的**先后**可见）
- AC: `resolve_operation` 的前置条件：run 处于 `needs_attention`、operation 处于 `unknown`、**且** `args_hash` 匹配
- AC: 核对后 operation → `succeeded`，run → `queued` 且 `generation + 1`
- AC: 核对追加 `operation_reconciled` 事件，含 `decided_by` 与 `evidence`
- AC: 核对**绝不**盲自动重放 —— 无 `evidence` 的核对被**拒绝**
- AC: `UnknownOutcome` 的端到端语义有测试 —— 副作用**不重复**执行（架构文档附录 C 的未验证项因此可关闭）
- AC: 取消**不映射**为 `ErrorCode::Internal`（取消不是失败）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-010 — P6 收口：架构文档同步 + 契约升格 + 故障注入

把 P6 落地后的事实**回写**到架构文档，并把不变量契约**升格**为 `verified`。

同时做**门禁自身的防伪验证**（NEX-24 / NEX-25 / NEX-35 的教训）。

对应 backlog 任务：`backlog/tasks/NEX-46 - docs(execution) P6 收口 —— 架构文档同步与契约升格.md`
依赖：T-001、T-002、T-003、T-004、T-005、T-006、T-007、T-008、T-009
对应设计条款：架构文档 §6.1、§6.2、附录 C

- AC: 架构文档 §6.1 的 P6 行标记为**完成**，并附**可核验**的交付摘要（不变量条数、测试数）
- AC: 架构文档 §4.2 差异 1 的「落地记录」补入 T-001 的**实际形态**（迁到哪个 crate、模块名、测试名）
- AC: 架构文档 §4.1 的 crate 图与 §4.2 / §4.3 的**不一致**被**消解**（要么图改，要么条款改 —— 不得留矛盾）
- AC: 架构文档**附录 C** 的 `UnknownOutcome` 分类从「🟡 部分验证」升为「✅ 已验证」，并附可核验的测试名
- AC: `docs/specs/p6/contracts/invariants.json` 的全部不变量 `status` 提升为 `verified`
- AC: `bash scripts/check-specs.sh` 通过（规则 3b：任务全部归档后不得残留 `planned`）
- AC: **故障注入**：对 P6 的关键断言做注入，验证**确实会红** —— 至少覆盖：围栏 `generation` 条件、三元组校验、`manual` 策略的 `unknown` 化、取消的顺序、审批过期
- AC: 故障注入的记录写进任务文件（含「注入什么 / 期望 / 实际」）
- AC: `docs/operations/` 有 P6 的运行说明（需要 PostgreSQL 17 的集成测试怎么跑、缺 `DATABASE_URL` 会 panic）
- AC: `bash scripts/check-docs.sh` 通过；`bash scripts/check-backlog.sh` 通过
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-011 — 修写操作识别的漏判（扫描器健壮性）

**T-001 预审发现的工具缺陷** —— 判定手段本身漏判多种能造成写入的 SQL 形态。

对应 backlog 任务：`backlog/tasks/NEX-52 - fix(core) 修写操作识别的漏判 —— SQL 注释剥离与 schema 限定.md`
依赖：T-001
对应设计条款：架构文档 §4.2 差异 1、§4.3.2 约束 2（**已知识别边界**段落）

- AC: `sql_words()` 剥离 `/* ... */` 与 `-- ...` 两种 SQL 注释 —— 注释里的词**不再**被当成表名
- AC: 表名提取剥离 **schema 限定** —— `public.workflow_runs` 的表名是 `workflow_runs`
- AC: `REPLACE INTO` / `MERGE INTO` / `COPY ... FROM` / `TRUNCATE TABLE` 被识别为写操作
- AC: `INSERT OR REPLACE INTO` / `UPDATE ONLY` 这类**修饰词**形态被识别为写操作
- AC: `crates/nexus-core/tests/token_scan.rs` 的**对抗输入测试**逐条覆盖上述每个形态
- AC: **故障注入**：`INSERT INTO public.workflow_runs ...` 加进 `nexus-repo` 后，
  `nexus_repo_does_not_touch_workflow_runs` 与 `workflow_runs_appears_only_in_execution_owned_crates`
  **两条断言都变红**
- AC: 读操作与**建表 DDL**仍**不**被误判为写；字符串字面量里的内容仍**不**算写
- AC: 架构文档 §4.3.2 约束 2 的「识别边界」说明**更新**为修复后的实际覆盖范围
- AC: `bash scripts/check-naming.sh` / `check-backlog.sh` / `check-specs.sh` / `check-docs.sh` 通过
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
