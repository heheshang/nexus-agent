# Spec · P6 工作流层

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 权威顺序：架构文档 > 本 Spec > `backlog/tasks/*`（Task 层）。
> 本文件是**契约**，不是进度记录 —— 进度看 `backlog/tasks/` 的任务状态。
> 分层判定记录见 [`../p6-tiering.md`](../p6-tiering.md)。

**特性**：`nexus-workflow`（自研图引擎）+ `nexus-execution`（持久执行与审批恢复）。
**架构文档阶段**：§6.1 的 P6 行。
**完成判据**：**租约、fencing、审批恢复有测试**（拆成可验证子目标，见 §6）。

> **范围的第一处裁剪（先查清「已有什么」）**：
> **租约与 fencing 已由 P2 交付**（`crates/nexus-repo/src/lease.rs` / `leases.rs`，NEX-12），
> 并有 trybuild 负向测试与变异测试**焊死**。
> P6 **不重复造** 租约与 fencing —— P6 要做的是把引擎侧持久化的**归属**落到唯一的 crate（T-001），
> 以及补上 P2 **没有**的部分：幂等账本、检查点、审批挂起与恢复、`needs_attention` 人工核对、取消语义。
> 详见 [`../p6-tiering.md`](../p6-tiering.md) §「判定时的事实核查」。
>
> ⚠️ **引擎侧持久化的归属形态（T-001 / 决策 D12）**：
> 操作者已于 2026-11-06 裁决为**选项 B** —— **不迁移**。
> `nexus-execution` **新增**对 `nexus-repo` 的依赖，引擎侧持久化能力**留在** `nexus-repo`。
> 该决策**修订了架构文档 §4.2 / §4.3 / §4.4 的已定稿条款**，
> 因此 T-001 需要**人工评审**，且 AI 产出的改动需**额外一名评审**。

---

## 1. 范围

### 1.1 在范围内

| 项 | 说明 |
|---|---|
| **冻结定义** | `FrozenDefinition`（`canonical` + `hash` + `engine_version`）三件套（架构文档 §2.5.2） |
| **图编译** | 结构化 JSON 图定义 → `CompiledGraph`；拓扑排序 + 入度计数；**展开上限**（深度 8 / 节点 128 / 状态 4 MiB） |
| **节点执行契约** | `NodeInvoke` trait；`NodeError` 三分类（可重试 / 不可重试 / 中断） |
| **图状态** | `GraphState`（键值映射）；`AllPredecessor` 触发语义；并发槽（`Semaphore`） |
| **条件求值** | `evalexpr` 求值布尔分支条件（语义限定为**布尔判断**） |
| **模板渲染** | `render_args`：`{{...}}` 保留原始类型；未解析的 `{{` → **错误** |
| **中断提案** | `Interrupt`（`path` / `args_hash` / `prompt` / `resume_key`）；`ArgsHash = sha256({path, prompt, parameters})` |
| **执行 driver** | 领取后校验版本三元组 → 编译 → 执行 → 续跑（`resume_with_data`） |
| **操作幂等账本** | `begin_operation` / `complete_operation`；三种 `replay_policy`（`readonly` / `idempotent` / `manual`）；`attempt` 上限 |
| **检查点** | 版本三元组（`definition_hash` / `engine_version` / `graph_schema`）绑定；读写不一致 → 版本不匹配 |
| **审批挂起与恢复** | `suspend`（写 `execution_approvals`）→ 决策（`approved` / `rejected` / 重复决策幂等 / 过期拒绝）→ 续跑 |
| **取消语义** | 取消时把在途 `manual` 操作标 `unknown`（**先保留不确定性，再 fence 执行者**） |
| **人工核对** | `needs_attention` + `resolve_operation`（必须带 evidence，**绝不**盲重放） |
| **`workflow_runs` 独占写入** | `nexus-execution` 是产品侧运行视图的**唯一写入者** —— 判定手段是「**写操作**归属」（架构文档 §4.2 差异 1，2026-11-06 修订） |
| **引擎侧持久化归属落地** | **不迁移**：`nexus-execution` **新增**对 `nexus-repo` 的依赖，引擎侧持久化能力**留在** `nexus-repo`（OQ-1 的选项 B · 决策 D12） |

### 1.2 不在范围内

| 项 | 理由 |
|---|---|
| **DSL 文本前端**（用户手写工作流定义） | OQ-2：DSL **语法**是产品选择，不阻塞「审批恢复有测试」；P6 的图编译输入是**结构化 JSON**。DSL 登记为后续任务 |
| **`workflows` / `workflow_versions` 的写入**（工作流定义的 CRUD） | OQ-5：属工作流**产品侧**，不是执行侧；P6 的集成测试自带种子数据 |
| **审批的 HTTP 端点** | OQ-6：属 `nexus-service`（7 行空壳）；P5 已定「业务编排属 `nexus-service`」。P6 只交付仓储与恢复语义 |
| **`nexus-service` 的接线**（`RunSubmitter` 的实现方） | `nexus-service` 是 7 行空壳；接业务编排是**另一个阶段**的工作量 |
| **MCP / A2A 集成** | 属 P8（`nexus-mcp` / `nexus-a2a`） |
| **共享容量（advisory lock / `FenceRun`）** | 参考架构 §3.6；`nexus-capacity` 是空壳。属 P9 的容量治理（参考架构 §5.4 第 7 项与 P6 **强耦合**，但**不是** P6 的完成判据） |
| **RAG / 记忆 / 知识库节点** | 属 P7；P6 的节点执行用**注入的 trait**，不实现具体知识检索 |
| **前端控制台** | 参考架构 §1.5；属 P9 |
| **run 生命周期的 Prometheus 指标口径** | OQ-7：属 P9 / §5.6 |
| **优雅关停** | 属 `nexus-cli`（进程生命周期） |
| **新增任何业务码** | 架构文档 §6.3 已定稿 42 个码；P6 复用（见 §2.1） |

---

## 2. 不可让步的红线

### 2.1 契约不可改

`nexus-core` 的 **42 个业务码、12 个事件名、18 个 ID 类型已定稿**（架构文档 §6.3）。

P6 **复用**下列码值，**不得新增**：

| 码值 | 枚举 | HTTP | 分类 | P6 用途 |
|---|---|---|---|---|
| `10001` | `InvalidParam` | 400 | Terminal | 图定义 / 参数非法 |
| `30004` | `RunNotFound` | 404 | Terminal | run 不存在 |
| `30005` | `WorkflowDsl` | 400 | Terminal | 图定义**解析**失败 |
| `30006` | `BudgetExceeded` | 500 | Terminal | 展开上限超限（深度 / 节点数 / 状态大小） |
| `30007` | `RunConflict` | 409 | Terminal | 版本不匹配 / 审批指纹不匹配 / 租约丢失（OQ-4） |
| `50001` | `ModelUnavailable` | 502 | Retryable | 节点内的 LLM 不可用 |
| `60006` | `StorageUnavailable` | 503 | Retryable | 数据库不可用 |
| `70001` | `ToolNotFound` | 404 | Terminal | 节点引用的工具不存在 |
| `70002` | `ToolFailed` | 500 | **UnknownOutcome** | 工具失败 —— **结果未知**，不得自动重试 |
| `90001` | `Internal` | 500 | Terminal | 执行层内部状态不一致 |
| `90003` | `ServiceUnavailable` | 503 | Retryable | 执行层不可用 |

> ⚠️ **P6 不得新增任何 `ErrorCode` 变体** —— 由
> `scripts/check-specs.sh` 规则 4/5 机器校验。
>
> ⚠️ **`ToolFailed` 是 `UnknownOutcome`** —— 它**不能**映射为可重试。
> 「工具可能已产生副作用（支付、写库）」，自动重试会重复执行。
> 这条与 P3 / P4 的白名单纪律**同一条**。

### 2.2 围栏写入**必须**带租约

围栏条件**只有一处定义**（架构文档 §2.5.4 落地记录）：

```sql
id = ? AND generation = ? AND lease_owner = ?
  AND status = 'running'
  AND lease_until > clock_timestamp()
  AND deadline_at > clock_timestamp()
```

**六个条件缺一不可**。`generation` 之外的四道是**第二道防线**。

**为什么这条是红线**：围栏是「僵尸 worker 不能覆盖新 worker 的写入」的**唯一**保证。
少一个条件 —— 尤其少 `generation` —— 整个持久执行模型**静默失效**。

**怎么被守住**：

1. **类型级** —— 写入方法要求 `&RunLease`，调用方**写不出**「忘记带 generation」的 UPDATE
   （NEX-12 已用 trybuild 固化：`crates/nexus-repo/tests/compile_fail/fenced_write_requires_a_lease.rs`）。
2. **变异测试** —— 删掉 `generation` 条件必须**变红**（NEX-12 已验证三种变异）。
3. **P6 承接** —— T-001 **不迁移**这些模块（决策 D12 选项 B），
   它们**留在** `nexus-repo`，围栏语义与变异测试**必须仍然成立**
   （T-001 的 AC 显式要求「围栏的变异测试在新的依赖形态下仍然变红」）。

> **换依赖 ≠ 换归属** —— 在选项 B 下，围栏实现**不动**，
> 但 `nexus-execution` 开始**依赖** `nexus-repo`。
> 这引入了一个新的风险：**执行层能绕过围栏直接调数据层**。
> T-001 的 AC 因此要求「`nexus-execution` 的**围栏写**必须**经由** `nexus-repo` 的
> 带租约 API，不允许在执行层自己拼 SQL」。
>
> 如果变异测试在改完之后**不再变红**，说明断言被改弱了。那是回归，不是重构。

### 2.3 `clock_timestamp()` 而不是 `now()`

`now()` 是**事务开始时间**，在事务内固定。长事务里用它判租约过期会**误判为未过期**。

领取与围栏判定**全部**用 `clock_timestamp()`（NEX-12 已落地）。

### 2.4 取消**不是**失败

- 取消的终态是 `canceled`，**不是** `failed`（架构文档 §2.1）。
- 取消**不映射**为 `Internal`。
- 取消**必须**先保留不确定性（把在途 `manual` 操作标 `unknown`），**再** fence 执行者
  （参考架构 §3.4.4）。

### 2.5 「结果未知」**不能**当可重试处理

`UnknownOutcome`（架构文档 §2.6.3）在 P6 有**两个**落点：

| 落点 | 行为 |
|---|---|
| `manual` 策略的操作失败 | 标 `unknown` → **不自动重试** → run 转 `needs_attention` |
| 取消时在途的 `manual` 操作 | 标 `unknown` → 追加 `operation_unknown` 事件 |

### 2.6 `needs_attention` **不能**合并进 `failed`

参考架构 Phase 9 的已知坑 #1 明写：「**绝不能**把 `needs_attention` 合并进 `failed`」。

理由：`failed` 是「**确定**失败」；`needs_attention` 是「**结果未知**，等人工核对」。
合并 = 运维看不出哪些 run 需要人管。

### 2.7 依赖方向

**P6 只新增一条依赖边：`nexus-execution → nexus-repo`**（决策 D12 · 选项 B）。

| crate | 依赖 |
|---|---|
| `nexus-workflow` | `nexus-core` / `nexus-config` / `nexus-model` / `nexus-llm` / `nexus-tool` / `nexus-rag` / `nexus-observability`（**既有，不变**） |
| `nexus-execution` | `nexus-core` / `nexus-config` / `nexus-model` / `nexus-llm` / `nexus-tool` / `nexus-workflow` / `nexus-capacity` / `nexus-infra` / `nexus-observability` / `nexus-security`（**既有**）+ **`nexus-repo`（T-001 新增）** |

由 `scripts/check-deps.sh` 规则 7（`deps_of` ↔ `Cargo.toml` 一致性）强制。

**硬约束**（架构文档 §4.3.2，2026-11-06 修订）：

1. `nexus-execution` **不得**依赖 `nexus-service` / `nexus-api` / `nexus-cli`；
2. `workflow_runs` / `workflow_step_runs` 的**写操作**只允许发生在 `nexus-execution` ——
   `nexus-repo` **不得**包含这两张表的**写操作**；
3. `nexus-model` **不得**出现任何数据库 schema 痕迹；
4. `nexus-repo` **不得**依赖 `nexus-execution`（反向成环 —— §4.4 规则 4）。

> ⚠️ **约束 2 的判定手段变了（这是选项 B 的关键后果）**：
> 原先的表述是「`nexus-repo` **不得引用** `workflow_runs` 的**类型**」。
> 依赖方向改为 `execution → repo` 后，类型对**所有依赖 `nexus-repo` 的 crate** 可见，
> 「`nexus-repo` 不持有该类型」**推不出**「别的 crate 写不了」。
>
> 判定手段从「**类型**归属」改为「**写操作**归属」：
> 全 workspace 扫描 `workflow_runs` / `workflow_step_runs` 的
> `INSERT` / `UPDATE` / `DELETE`，写操作**只允许**出现在 `nexus-execution`；
> `nexus-infra` 允许的是**建表 DDL**（迁移），不是访问。
>
> 强度**不降** —— 前者可被「把类型下沉到 `nexus-repo`」绕过，
> 后者管住**任何 crate 的任何写操作**。
>
> 该修订触及**已定稿条款**（架构文档 §6.3 第 5 项「crate 边界与 trait 归属」）——
> T-001 需要**人工评审**，且 AI 产出的改动需**额外一名评审**。

> ⚠️ **执行层与数据层的单向性**：`nexus-execution → nexus-repo` 允许，
> `nexus-repo → nexus-execution` **禁止**。反向会成环，Cargo 编译期就会拒绝。

### 2.8 内部原因不外泄

`AppError.cause` **绝不**进入对外响应或事件负载（架构文档 §2.6.5）。
**类型级**保证：`AppError` **不实现 `Serialize`**；只有 `UserFacing` 可序列化。

P6 写进 `execution_events` 的事件负载**不含** cause。

### 2.9 不引入 `unsafe`

workspace 已 `forbid`。

### 2.10 不引入 `Box<dyn>` 于热路径

节点执行是**逐节点**的热点路径。用**泛型 + 静态分发**。
`Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**这种「异构集合」的场景。

---

## 3. 核心契约

### 3.1 冻结定义（三件套）

```
定义 JSON ──规范化──> canonical bytes ──SHA-256──> hash
        └──> engine_version（解释这份定义的引擎版本）
```

| 字段 | 回答的问题 | 不匹配时 |
|---|---|---|
| `canonical` | 「这次跑的到底是什么」 | 快照本身就是答案 |
| `hash` | 「当前定义的哈希与运行记录里的一致吗」 | 检测到外部修改 → **拒绝续跑** |
| `engine_version` | 「用哪个版本的引擎解释这份定义」 | 版本不兼容 → 需要迁移路径 |

**规范化规则**（架构文档 §2.5.3，**已定稿**）：

1. 对象键按**字节序**排序；
2. 数字规范化（`1.0` 与 `1` 哈希相同）；
3. 字符串按 UTF-8 原始字节，**不做**转义差异归一；
4. 不包含任何非语义字段（时间戳、生成器版本）。

**实现落点**：`nexus_core::canonical::canonical_hash_hex`（**已存在**，P3 落地）。
`nexus-workflow` 与 `nexus-execution` **共用**它 —— 放在 `nexus-core` 正是因为
这两个 crate 互相不能依赖（否则成环）。

### 3.2 图编译

```
结构化 JSON 图定义 ──compile──> CompiledGraph
```

| 要素 | 语义 |
|---|---|
| 节点种类 | `Lambda` / `Branch` / `SubGraph` / `Parallel` / `Human` / `Tool` |
| 触发语义 | `AllPredecessor` —— **所有**前驱完成才触发 |
| 编译产物 | `nodes`（节点表）+ `roots`（入口节点）+ `name` |
| 子图递归检测 | 编译时用 `HashSet<NodeId>` 检测**递归**（自引用）→ **错误**，不是无限展开 |

**展开上限**（参考架构 §3.5.2，**已定稿**，全部照抄）：

| 上限 | 值 |
|---|---|
| 嵌套深度 | **8** |
| 展开节点总数 | **128** |
| 并发槽 | **4** |
| 状态大小 | **4 MiB** |
| DSL 文本 | **1 MiB**（P6 不做 DSL 文本；该上限在 OQ-2 的后续任务里生效） |

**超限** → `ErrorCode::BudgetExceeded`（30006）。**不是** panic。

### 3.3 节点执行契约

```
NodeInvoke::invoke(ctx, input) -> Result<Option<NodeOutput>, NodeError>
```

| 返回 | 语义 |
|---|---|
| `Ok(Some(out))` | 本节点正常产出 |
| `Ok(None)` | **无产出但正常** |
| `Err(NodeError::Retryable(..))` | 明确失败，可重试 |
| `Err(NodeError::Permanent(..))` | 明确失败，**不可**重试 |
| `Err(NodeError::Interrupt(interrupt))` | 需要**挂起**等待审批 |

**`NodeError` 是三分类**，不是字符串（架构文档 §2.6.2：领域错误是 enum）。

### 3.4 图状态与并发

| 项 | 语义 |
|---|---|
| 状态容器 | `GraphState`（键 → JSON 值） |
| 状态大小检查 | 序列化后长度 > 4 MiB → **错误**（不是截断） |
| 并发槽 | `Semaphore`（默认 4）—— 节点任务 `acquire_owned()` |
| 完成顺序 | 完成一个节点 → 递减后继入度 → 归零则 `spawn` |

**图状态用 `serde_json::Map`** —— 与 Go 版的 `map[string]any` 同构。

### 3.5 条件求值与模板渲染

**条件求值**：`evalexpr` 求值**布尔**分支条件，变量从图状态取值。

| 规则 | 理由 |
|---|---|
| 结果**必须**是布尔 | 条件语义就是「yes / no 哪个目标」；非布尔是**定义错误** |
| 求值**失败** → **错误** | 不是 `false` —— 静默当 `false` 会走错分支 |
| 变量**不存在** → **错误** | 同上 |

**模板渲染**（参考架构 §3.5.4）：

| 规则 | 语义 |
|---|---|
| 整串是 `{{...}}` 且**只出现一次** | **保留原始类型**（数字仍是数字） |
| 否则 | 文本插值 |
| 插值结果**仍含** `{{` | **错误**（未解析的工具参数模板） |
| 嵌套 map / slice | **递归**渲染 |

### 3.6 中断提案与审批指纹

```
Interrupt { path, args_hash, prompt, resume_key }
```

| 规则 | 语义 |
|---|---|
| `args_hash = sha256({path, prompt, parameters})` | **参数变化 → 审批自动失效** |
| 恢复时 `decision.args_hash != proposal.args_hash` | **拒绝恢复**（参数已变） |
| 审批能力**不能**走 context | Rust 无 context values → **必须显式传递**（参考架构 §2.6） |

### 3.7 执行 driver

```
领取（租约 + generation+1）
  → 校验版本三元组（definition_hash / engine_version / graph_schema）
  → 编译图
  → 执行（逐节点：begin_operation → 执行 → complete_operation）
  → 遇 Interrupt → suspend(approvals)
  → 终态：Succeeded / Failed / NeedsAttention / Canceled
```

**围栏写与业务写同事务**（参考架构 Phase 9 必抄不变量 #9）——
围栏检查与业务更新之间不能隔事务边界。

### 3.8 操作幂等账本

```
begin_operation(path, kind, args_hash, policy)
  → (operation, cached)
```

| 既有 operation 的状态 | 行为 |
|---|---|
| `args_hash` / `kind` / `replay_policy` 任一不同 | **冲突** —— 不能复用旧结果 |
| `Succeeded` | **幂等命中** —— 直接返回旧输出 |
| `unknown` | **要求人工核对** —— 不重放 |
| `Running` 且 `generation` 与当前一致 | **冲突** —— 并发同节点 |
| `manual` 策略且上次执行者丢失 | 标 `unknown` + 追加 `operation_unknown` 事件 |
| `attempt` 已达上限 | **尝试耗尽** —— 失败 |

**三种 `replay_policy`**（OQ-3，**三种全做**）：

| 策略 | 语义 | 失败后行为 |
|---|---|---|
| `readonly` | 无副作用 | 可自动重试 |
| `idempotent` | 幂等副作用 | 可重试 |
| `manual` | 副作用未知 | 标 `unknown`，**不自动重试**，转 `needs_attention` |

**`attempt` 上限是配置项**，不是硬编码常数（参考架构 §3.4.6 明写：
「Rust 版应做成配置项」）。

### 3.9 检查点版本三元组

| 规则 | 语义 |
|---|---|
| 三元组 = `(definition_hash, engine_version, graph_schema)` | 续跑时任一不同 → **拒绝** |
| 读 | 比对**既有 run 的版本** 与 **当前进程常量**；任一不同 → 版本不匹配 |
| 写 | `ON CONFLICT DO UPDATE` 带三元组条件；影响行数 ≠ 1 → 版本不匹配 |

**为什么要写两次版本**：既有 run 的版本（防用新引擎读旧检查点）+
当前进程的版本（防用旧引擎写新检查点）。

### 3.10 审批挂起与恢复

```
suspend(approvals) → status = waiting_approval
decide(interrupt_id, args_hash, decision, comment)
  → approved → status = queued（可续跑）
  → rejected → status = canceled, generation + 1
```

| 规则 | 语义 |
|---|---|
| **重复决策**（同决策） | **幂等**返回，不报错 |
| **冲突决策**（不同决策） | 冲突错误 |
| **过期** | `expires_at > clock_timestamp()` 才允许写入决策；否则冲突 |
| **参数变化** | 挂起时发现已有审批的 `args_hash` / `path` 与本次不同 → 版本不匹配 |
| **恢复二次校验** | 恢复时比对 `decision.args_hash == proposal.args_hash` |

### 3.11 取消与人工核对

**取消**（参考架构 §3.4.4）：

| 顺序 | 动作 |
|---|---|
| 1 | 扫描 `status = 'running'` 的 operation |
| 2 | `replay_policy = 'manual'` → 标 `unknown` + 追加 `operation_unknown` 事件（reason = `canceled_in_flight`） |
| 3 | 其他 → 标 `abandoned` |
| 4 | run 本身 → `status = canceled`、`generation + 1`、清租约 |

> **顺序不可调换** —— 先保留不确定性，**再** fence 执行者。
> 调换 = 取消后无法查清「到底发生了什么」。

**人工核对**（`resolve_operation`）：

| 规则 | 语义 |
|---|---|
| 前置条件 | run 处于 `needs_attention`、operation 处于 `unknown`、**且** `args_hash` 匹配 |
| 动作 | operation → `succeeded`；run → `queued` 且 `generation + 1`；追加 `operation_reconciled` 事件（含 `decided_by` 与 `evidence`） |
| **绝不** | 盲自动重放 —— 核对必须带 evidence |

### 3.12 `workflow_runs` 的独占写入

`nexus-execution` 是 `workflow_runs` / `workflow_step_runs` 的**唯一写入者**
（架构文档 §4.2 差异 1，2026-11-06 修订）。

| 规则 | 理由 |
|---|---|
| **写操作**只允许出现在 `nexus-execution` | 判定手段（结构层扫描 `INSERT` / `UPDATE` / `DELETE`）—— 与 `nexus-repo` 是否持有该类型**无关** |
| `nexus-infra` 只允许**建表 DDL**（迁移） | 建表不是访问 |
| 状态变更时**同步**写产品视图 | 产品侧关心「结果是什么」，引擎侧关心「谁在跑、写到哪一步」 |
| **两张表不是同一份数据的两份拷贝** | `execution_runs` 是**权威**，PG 是唯一事实来源 |

> **为什么判定手段从「类型」改成「写操作」**：
> 选项 B 下 `nexus-execution` 依赖 `nexus-repo`，
> `nexus-repo` 持有 `workflow_runs` 的行类型**不再**破坏「写入权唯一」——
> 真正的风险是**别的 crate 写这张表**，而那与类型归属无关。
>
> 强度**不降**：类型归属可被「把类型下沉到 `nexus-repo`」绕过；
> 写操作归属管住**任何 crate 的任何写操作**。

---

## 4. 安全契约

### 4.1 凭据不外泄

P6 **不接触**任何凭据（节点的工具/LLM 调用由 `nexus-tool` / `nexus-llm` 处理）。

### 4.2 内部原因不外泄

已在 §2.8 与 §3.11 声明。**类型级**保证。

### 4.3 事件负载不含 cause

写进 `execution_events` 的 `data` **只含**事件负载，**不含** cause。

### 4.4 审批能力走**类型隔离**，不走参数传递

参考架构 §2.6 的取舍说明：

> Go 版靠注释说明「审批能力不来自普通 Agent 输入」，靠自觉；
> Rust 版可以让越权路径**编译不过**。

P6 采用**类型区分**（typestate）：未授权作用域与已授权作用域是**不同的类型**，
已授权类型的构造函数**模块私有**。

---

## 5. 验证方式

```bash
# 门禁
bash scripts/check-lock.sh      # 依赖锁定纪律
bash scripts/check-deps.sh      # crate 依赖方向铁律
bash scripts/check-naming.sh    # 标识符不得含中文
bash scripts/check-backlog.sh   # backlog 组织纪律
bash scripts/check-specs.sh     # Spec 层组织纪律
bash scripts/check-rules.sh     # 规则文件一致性
bash scripts/check-docs.sh      # 文档层一致性
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是**独立 workspace**，验证命令为 `cd examples && cargo test --workspace`。

**需要真实 PostgreSQL 17 的集成测试**（租约 / 围栏 / 幂等 / 审批）：
沿用 P2 的基座 —— 缺 `DATABASE_URL` 时 **panic**（不是静默跳过）。

---

## 6. 完成判据与「防伪」要求

架构文档 §6.1：**「租约、fencing、审批恢复有测试」**

拆成**十一个**可验证子目标：

1. **围栏的变异测试在新的依赖形态下仍然变红** —— 删掉 `generation` 条件必须失败（§2.2）。
2. **版本三元组校验有测试** —— 三元组任一不同 → 拒绝续跑（§3.9）。
3. **图编译的展开上限有测试** —— 深度 / 节点数 / 状态大小三个上限**各自**被测。
4. **条件求值有测试** —— 非布尔结果 / 求值失败 / 变量不存在**各自**被测。
5. **模板渲染保留原始类型有测试** —— `{{...}}` 整串 → 数字仍是数字。
6. **中断指纹有测试** —— 参数变化 → 指纹不同 → 审批失效。
7. **操作幂等账本有测试** —— 三种 `replay_policy` **各自**被测，含 `manual` 的 `unknown` 化。
8. **`attempt` 上限是配置项有测试** —— 改配置即改上限。
9. **审批挂起与恢复有测试** —— 挂起 → 批准 → 续跑；拒绝 → 取消；重复决策幂等；过期拒绝。
10. **取消保留不确定性有测试** —— 取消后在途 `manual` 操作是 `unknown`（**不是** `abandoned`）。
11. **人工核对带 evidence 有测试** —— `operation_reconciled` 事件含 `decided_by` 与 `evidence`。

### 6.1 「有测试」的**防伪**要求

沿用 NEX-14 / NEX-22 / NEX-24 / NEX-25 / NEX-35 的教训：

| 假象形态 | 防线 |
| --- | --- |
| 只测了「租约能领取」 | 必须测**僵尸写入被拒**（影响 0 行） |
| 围栏测试只断言「报错了」 | 必须断言**结果分类**（`Applied` vs `Fenced`） |
| 断言引用了被测代码的**常量** | 必须断言**字面量**（NEX-25 的教训 —— 同义反复） |
| 只测了「正常路径」 | 必须测「**取消**」「**冲突**」「**过期**」「**版本不匹配**」 |
| 只测了一种 `replay_policy` | 必须测**三种各自**的行为 |
| 改动依赖方向时把变异测试改弱 | 改完后**必须仍能变红**（§2.2） |
| 改了依赖方向后把「写入权唯一」的断言留在原处 | 判定手段必须**同步**改为「**写操作**归属」（§2.7 / §3.12） |
| 断言「`needs_attention` 不等于 `failed`」但只查了枚举值 | 必须查**落库后的状态** |
| 测试「存在」但没跑 | `scripts/check-specs.sh` 规则 3 机器校验 |
| 门禁/断言**没验证过会红** | **必须做故障注入**（NEX-24 的教训） |

---

## 7. 已定稿的技术决策

| 编号 | 决策 | 选择 | 依据 | 类别 |
|---|---|---|---|---|
| D12 | 引擎侧持久化的 crate 归属 | **选项 B**：`nexus-execution` **新增**依赖 `nexus-repo`；引擎侧持久化能力**留在** `nexus-repo`（**不迁移**） | 见 §7.1 | **(b) timeboxed** |
| D13 | `workflow_runs` 的外键依赖 | P6 **不交付** `workflows` 的写入；集成测试自带种子数据 | 见 §7.2 | (a) decide-and-proceed |
| D14 | `replay_policy` 的范围 | **三种全做** | 见 §7.3 | (a) decide-and-proceed |
| D15 | 图定义的输入形态 | **结构化 JSON**，不做 DSL 文本 | 见 §7.4 | (a) decide-and-proceed |
| D16 | 版本不匹配 / 指纹不匹配的码 | 统一 `RunConflict`（30007） | 见 §7.5 | (a) decide-and-proceed |
| D17 | 审批的 HTTP 端点 | **不在 P6** —— 属 `nexus-service` | 见 §7.6 | (a) decide-and-proceed |
| D18 | run 指标口径 | **不在 P6** —— 属 P9 / §5.6 | 见 §7.7 | (a) decide-and-proceed |

### 7.1 D12 —— 引擎侧持久化**留在 `nexus-repo`**，`nexus-execution` 依赖之（选项 B）

**决策**（操作者 2026-11-06 裁决）：**不迁移**。
引擎侧持久化（`lease` / `leases` / `outbox_repo` + 引擎侧行类型）
**留在** `crates/nexus-repo/`；`nexus-execution` **新增**对 `nexus-repo` 的依赖，
复用数据层已有的持久化能力。

**类别判定**：**(b) timeboxed**。

- **为什么不是 (a) decide-and-proceed**：本次改动**修订了架构文档 §6.3 第 5 项
  「crate 边界与 trait 归属」的已定稿条款**（§4.2 差异 1、§4.3 目标态表、
  §4.3.2 约束 2、§4.4 规则 4），**触及 crate 依赖方向**——
  按 `ai-sdlc-decision-authority.mdc` §2.1，「影响面大 / 触及契约」**至少**是 (b)。
- **为什么不是 (c) operator-only**：不涉及法律、许可、金钱、凭据，
  也不需要以操作者身份执行的动作。**不需要**再次升级。
- **时间盒与自主回退**：见下方。

**依据**：

1. **§4.1 的 crate 图本就画着 `EXE --> REPO`**，且 §4.1.1 的层次表
   「执行 → 数据 ✅」允许这条方向 —— 选 B 与**目标态总览一致**。
2. **围栏条件「只有一处定义」的纪律不受影响** ——
   实现仍在 `crates/nexus-repo/src/lease.rs` 一处，
   NEX-12 的变异测试**不用搬家、不会失效**。
3. **迁移是纯 churn**：迁移要重跑全部围栏测试，
   且要动 `nexus-repo` 里其他模块的依赖 —— 收益只是「让实现与某一条表述对齐」。
4. 选 B 要改的是**文档表述**，不是**代码位置** ——
   改文档的成本远低于迁移一个已被变异测试焊死的模块。

**反方论点与回应**：

> 「类型下沉到 `nexus-repo` 之后，别的 crate（`nexus-service` / `nexus-api`）就也能拿到
> `workflow_runs` 的类型了 —— 『写入权唯一』这个保证不就没了吗？」

**回应**：这个担心**成立**，所以判定手段必须**改**：

- 原先的判定手段是「`nexus-repo` **不得引用** `workflow_runs` 的**类型**」。
  依赖方向改为 `execution → repo` 后，这个手段**不再等价**于「写入权唯一」。
- 新手段是「`workflow_runs` / `workflow_step_runs` 的**写操作**
  （`INSERT` / `UPDATE` / `DELETE`）**只允许**出现在 `nexus-execution`」。
  它**与类型归属无关**，管住**任何 crate 的任何写操作**。
- **强度不降**：类型归属可被「把类型下沉到 `nexus-repo`」绕过；
  写操作归属管不住「别人写」才被绕过。

**时间盒与自主回退**（类别 (b) 的强制内容）：

| 项 | 内容 |
|---|---|
| **时间盒截止** | **2026-11-13** |
| **到期前的行为** | **不实施** T-001；其余任务（T-002 ~ T-010）照常推进 |
| **无人干预时的自主回退** | 按**选项 B** 执行 T-001 —— 依赖边是**加法**（`nexus-execution` 多一条 `nexus-repo`），文档修订是**可回滚**的（`git revert` 即可），且与 §4.1 的目标态图一致 |
| **已裁决** | 操作者已于 **2026-11-06** 裁决为**选项 B** —— 本项**不再是待决项** |

**可逆性**：中 —— 依赖边与文档修订都可 `git revert`；
但**长期**看这是 crate 边界的既定事实。
**影响面**：`crates/nexus-execution/Cargo.toml`、
`scripts/check-deps.sh` 的 `deps_of`、
`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`
（§4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 / §4.4 规则 4 / §5.3 / §6.3）。

> ⚠️ **本项修订了已定稿条款** —— T-001 需要**人工评审**，
> 且 AI 产出的改动需**额外一名评审**（`ai-sdlc-quality-gates.mdc`）。

### 7.2 D13 —— `workflows` 的写入不归 P6

**决策**：P6 **不交付** `workflows` / `workflow_versions` 的写入；
`workflow_runs` 的写入路径要求调用方传入**已存在的** `workflow_id`。

**依据**：

1. 工作流定义的 CRUD 属**工作流产品侧**，不是执行侧；
2. 参考架构 §2.5 的 Go 版流程本来就是「提交时带 `workflow_id`」；
3. 参考架构 §5.2 Phase 0 的坑明写「demo 自己 INSERT（注意外键链 `runs → workflows → users`）」。

**可逆性**：高 —— 后续阶段补上写入方即可。
**影响面**：`crates/nexus-execution/*` 的 `workflow_runs` 写入路径。

### 7.3 D14 —— 三种 `replay_policy` 全做

**决策**：`readonly` / `idempotent` / `manual` **三种全做**。

**依据**：架构文档附录 C 的未验证项里，`UnknownOutcome` 分类被标注为
「🟡 **部分验证**（P3：网关层断言超时不被自动重试；但『端到端不重复副作用』
**需要 P6 的持久执行来验证**）」。

「不重复副作用」的**唯一**实现路径就是 `manual` 策略 ——
标 `unknown`、**不自动重试**、转 `needs_attention` 等人工核对。
只做 `readonly` 的话，附录 C 这一项**无法在 P6 关闭**。

**反方论点与回应**：

> 「参考架构 §5.2 的 Phase 9 行明写『**可以简化**：可以先只做 `readonly` 一种』。
> 少做一种 = 少一套分支、少一套测试、少一个 `attempt` 上限的语义。」

**回应**：这条论点成立 —— **工作量**确实少。但**完成判据**不成立：

1. 阶段表的完成判据是「租约、fencing、**审批恢复**有测试」，
   而**审批恢复**与 `needs_attention` 人工核对**共用** `manual` 策略的 `unknown` 化语义；
2. 架构文档附录 C 明确把「端到端不重复副作用」的验证责任**指派给 P6**；
3. 少做一种的代价是**一个阶段的判据无法关闭** —— 那比多写一套分支贵。

**可逆性**：高 —— 补一种策略是增量。
**影响面**：`crates/nexus-execution/*` 的操作账本。

### 7.4 D15 —— 图定义的输入是结构化 JSON

**决策**：图编译的输入是**结构化 JSON**（节点 / 边 / 分支条件 / 子图引用），
**不含** DSL 文本解析器。条件表达式用 `evalexpr`，语义限定为**布尔判断**。

**依据**：

1. DSL **文本语法**是**产品选择**（用户写什么），不是执行语义 ——
   它不阻塞「审批恢复有测试」；
2. 参考架构 §5.3 局限 #5：参考项目**同时存在两套工作流引擎**，Rust 版**只做一套**（编译式）。
   少一个前端不增加「两套路径」的维护负担；
3. `crates/nexus-workflow/Cargo.toml` 已声明 `evalexpr` —— 脚手架本身预告了这个落点。

**可逆性**：高 —— DSL 前端是**增量**（解析器 + 一层编译），不改动已定稿的图语义。
**影响面**：`crates/nexus-workflow/src/graph.rs`。

### 7.5 D16 —— 版本不匹配统一映射 `RunConflict`

**决策**：`definition_hash` / `engine_version` / `graph_schema` 不一致，
以及审批 `args_hash` 不一致，**统一**映射到 `RunConflict`（30007，HTTP 409，终止型）。

**依据**：

1. §6.3 是**前后端契约**，新增码 = 破坏性变更 —— 不属于 P6 的裁量范围；
2. 这几类情形的共同点是「**当前操作与已固化的记录冲突**」—— `RunConflict` 语义吻合；
3. `RunConflict` 已是**终止型**（`nexus-repo` 的 `LeaseLost` 已用它）—— 重试无意义，符合语义。

**可逆性**：中 —— 改码是**破坏性变更**（需前后端同步）。
**影响面**：`crates/nexus-execution/*` 的错误映射。

### 7.6 D17 —— 审批的 HTTP 端点不在 P6

**决策**：P6 只交付审批的**仓储语义与恢复语义**；**不交付** HTTP 端点、不接线 `nexus-service`。

**依据**：参考实现的审批端点属 service 层；`nexus-service` 是 7 行空壳；
P5 的决策 D9 已定「业务编排属 `nexus-service`」。

**可逆性**：高 —— 端点是**注册**到既有路由骨架的增量。
**影响面**：无（不交付即无影响）。

### 7.7 D18 —— run 指标口径不在 P6

**决策**：P6 **不交付** run 生命周期的 Prometheus 指标口径。

**依据**：架构文档 §5.6 的指标基线属 P9；P6 的可观测性由**事件流**（`execution_events`）承载。

**可逆性**：高 —— 指标是增量。
**影响面**：无（不交付即无影响）。

### 7.8 后续任务（带触发条件）

| # | 后续工作 | 触发条件 |
|---|---|---|
| F4 | DSL 文本前端（用户手写工作流定义） | 当产品侧需要用户手写定义时（OQ-2） |
| F5 | 审批的 HTTP 端点 + `nexus-service` 接线 | 当 `nexus-service` 落地业务编排时（OQ-6） |
| F6 | `workflows` / `workflow_versions` 的写入方 | 当工作流定义 CRUD 立项时（OQ-5） |
| F7 | 共享容量（advisory lock / `FenceRun`） | 当容量治理立项时（参考架构 §3.6） |
| F8 | run 生命周期的指标口径 | 当 P9 启动时（OQ-7） |

---

## 8. 验证方式补充：三层验证的落点

| 层 | P6 的落点 |
|---|---|
| **类型层**（`trybuild` 负向测试） | 围栏写**必须**带 `&RunLease`（NEX-12 已有，留在 `nexus-repo` 后仍有效）；`RunLease` 不可直接构造；已授权作用域的构造**模块私有** |
| **结构层**（架构测试） | `nexus-execution` **依赖** `nexus-repo` 且**不得**依赖 `nexus-service` / `nexus-api` / `nexus-cli`；`nexus-repo` **不得**依赖 `nexus-execution`；`workflow_runs` / `workflow_step_runs` 的**写操作**只允许出现在 `nexus-execution`；`nexus-model` **无** schema 痕迹；`nexus-workflow` / `nexus-execution` **不含** `Box<dyn>` 于节点执行热路径 |
| **行为层**（集成测试） | §6 的十一个子目标 |
