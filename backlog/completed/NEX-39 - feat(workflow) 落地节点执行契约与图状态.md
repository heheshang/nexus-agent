---
id: NEX-39
title: "feat(workflow): 落地节点执行契约与图状态"
status: Done
assignee: []
labels: [workflow, graph-engine, core]
priority: high
dependencies:
  - NEX-38
references:
  - "docs/specs/p6/spec.md §3.3、§3.4、§3.5、§2.10"
  - "docs/specs/p6/tasks.md T-003"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.5、§3.5.4"
---

## Description

**问题**：节点是工作流的**唯一扩展点**。没有「节点怎么被调用、状态怎么存、条件怎么判、
参数怎么渲染」的契约，driver（NEX-41）就没有可执行的图。

**修法**：落地四件事 —— `NodeInvoke` trait、`GraphState`、条件求值（`evalexpr`）、
模板渲染（`render_args`）。

**为什么错误必须是三分类**（架构文档 §2.6.2：领域错误是 enum，不是字符串）：
`Retryable` / `Permanent` / `Interrupt` 的**处置完全不同** ——
可重试就重试，不可重试就结束，`Interrupt` 就挂起等审批。
把它们合成一个字符串 = 「到底该不该重试」变成运行时猜。

## 范围边界

### 做

- `nexus-workflow/src/node.rs`：`NodeInvoke` / `NodeError` / `NodeOutput`
- `nexus-workflow/src/state.rs`：`GraphState` + 状态大小上限
- `nexus-workflow/src/condition.rs`：`evalexpr` 布尔求值
- `nexus-workflow/src/template.rs`：`render_args`

### 不做

| 项 | 理由 |
|---|---|
| 中断提案（`Interrupt` / `args_hash`） | 属 NEX-40 |
| 具体的工具 / LLM 节点实现 | 属 P8 / P3；P6 只交付**注入的** trait |
| 持久化 / 幂等 | 属 NEX-37 / NEX-42 |
| `Box<dyn>` 的动态分发 | 架构文档 §2.3.2 —— 泛型优先，`dyn` 兜底 |

## Acceptance Criteria

- [x] #1 `crates/nexus-workflow/src/node.rs` 定义 `NodeInvoke` trait —— `invoke` 返回 `Result<Option<NodeOutput>, NodeError>`
- [x] #2 `NodeError` 是**三分类** enum：`Retryable` / `Permanent` / `Interrupt`（不是字符串）
- [x] #3 `Ok(None)` 表示「**无产出但正常**」，与 `Err` 区分
- [x] #4 `crates/nexus-workflow/src/state.rs` 定义 `GraphState`（键 → JSON 值）；状态大小 > **4 MiB** → **错误**（不是截断）
- [x] #5 并发槽用 `Semaphore`（默认 **4**）；节点任务用 `acquire_owned()`
- [x] #6 条件求值用 `evalexpr`，结果**必须**是布尔；非布尔 → **错误**
- [x] #7 条件求值**失败** → **错误**，**不是** `false`（否则走错分支）
- [x] #8 条件里的变量**不存在** → **错误**，不是 `false`
- [x] #9 模板渲染：整串是 `{{...}}` 且**只出现一次** → **保留原始类型**（数字仍是数字）
- [x] #10 模板渲染：插值结果**仍含** `{{` → **错误**（未解析的工具参数模板）
- [x] #11 模板渲染：嵌套 map / slice **递归**渲染
- [x] #12 `crates/nexus-workflow` 的源码**不含** `Box<dyn` 于节点执行热路径 —— `Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**处（结构层断言）
- [x] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### AC → 证据对照

| AC | 证据 |
|---|---|
| #1 | `tests/node.rs::a_node_can_be_invoked_and_returns_its_output`、`::the_node_context_exposes_the_path_and_the_state` |
| #2 | `tests/node.rs::the_node_error_has_three_distinct_classes`、`::a_node_can_return_each_of_the_three_error_classes` |
| #3 | `tests/node.rs::no_output_is_distinct_from_an_error`、`::an_error_is_not_reported_as_no_output` |
| #4 | `tests/node.rs::a_state_write_beyond_the_limit_is_rejected_and_reverted`、`::an_oversized_state_is_not_truncated`、`::an_oversized_merge_is_rejected_as_a_whole`、`::the_state_size_is_measured_on_the_serialized_form` |
| #5 | `tests/node.rs::the_concurrency_gate_defaults_to_four_slots`、`::the_concurrency_gate_limits_parallel_nodes`、`::a_cloned_gate_shares_the_same_slots`、`::a_gate_with_zero_slots_is_rejected` |
| #6 | `tests/node.rs::a_non_boolean_condition_result_is_an_error` |
| #7 | `tests/node.rs::a_failed_condition_evaluation_is_an_error` |
| #8 | `tests/node.rs::a_missing_condition_variable_is_an_error` |
| #9 | `tests/node.rs::a_whole_string_template_keeps_the_original_type`、`::a_mixed_template_is_interpolated_as_text` |
| #10 | `tests/node.rs::an_unresolved_template_is_rejected` |
| #11 | `tests/node.rs::a_nested_template_is_rendered_recursively` |
| #12 | `tests/architecture.rs::the_workflow_crate_does_not_use_box_dyn_on_the_node_hot_path`、`::the_dyn_trait_object_only_appears_at_the_graph_loading_site`、`::the_graph_loading_site_uses_arc_dyn_node_invoke` |
| #13 | 门禁命令退出码 —— 见「门禁执行记录」 |

### 门禁执行记录

| 门禁 | 结果 |
|---|---|
| `cargo fmt --all --check` | ✅ 通过 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ✅ 零告警 |
| `cargo test --workspace --locked` | ⚠️ **除 `nexus-core` / `nexus-repo` 的 trybuild 快照缺失外全绿** —— 见下方「已知失败」 |
| `bash scripts/check-lock.sh` | ✅ 通过 |
| `bash scripts/check-deps.sh` | ✅ 通过 |
| `bash scripts/check-naming.sh` | ✅ 通过 |
| `bash scripts/check-backlog.sh` | ✅ 通过 |
| `bash scripts/check-specs.sh` | ✅ 通过 |
| `bash scripts/check-rules.sh` | ✅ 通过 |
| `bash scripts/check-docs.sh` | ✅ 通过 |

### 已知失败（**预存在**，与本任务无关）

`nexus-core` 与 `nexus-repo` 的 `trybuild` 负向测试**缺 `.stderr` 快照** ——
干净检出下会红（trybuild 会现场生成快照并让测试失败）。

- **已验证是预存在**：把本任务的**全部**改动 `git stash` 后重跑，失败**完全相同**。
- **归属**：NEX-53（`fix/trybuild-snapshots-not-tracked`，提交 `a82fad4`）——
  那条分支已把 9 个 `.stderr` 快照入库，**未合入本分支**。
- **本任务不动它**：那是**另一条并行分支**的工作，且与 `nexus-workflow` **无交集**。
  合并顺序由操作者决定。

### 三层验证的落点

| 层 | 落点 |
|---|---|
| **类型层** | `NodeError` 是 enum（三分类穷尽 `match`）；`NodeInvoke` 用 `#[async_trait]` 保证 `dyn` 兼容 |
| **结构层** | `tests/architecture.rs` 的 6 条断言（依赖方向 / 无 `sqlx` / 职责边界文档 / 无 `Box<dyn>` / `Arc<dyn>` 只在装载处 / 装载处**确实**用 `Arc<dyn NodeInvoke>`） |
| **行为层** | `tests/node.rs` 的 26 条测试（契约 / 三分类 / 状态上限 / 并发闸门 / 条件求值 / 模板渲染） |

### 变异测试（断言**真的**有效）

AGENTS.md §6：「看起来显然」的不变量要用**变异测试**验证断言真的有效。

在**独立 `git worktree`**（`/tmp/nex39-mutation`）里注入 **15 处**变异，
每次只改一处，期望结果必须是**红**。**全部 15 处都变红** ✅

| # | 变异 | 变红的测试 |
|---|---|---|
| 1 | 条件求值失败 → `Ok(false)` | `a_failed_condition_evaluation_is_an_error` |
| 2 | 变量缺失 → 非布尔错误（不再是 `UnknownVariable`） | `a_missing_condition_variable_is_an_error` |
| 3 | 非布尔结果 → `Ok(true)` | `a_non_boolean_condition_result_is_an_error` |
| 4 | 状态超限**不回滚** | `a_state_write_beyond_the_limit_is_rejected_and_reverted` |
| 5 | 状态超限**静默放行** | 同上 |
| 6 | 合并**部分应用** | `an_oversized_merge_is_rejected_as_a_whole` |
| 7 | 整串占位符**丢原始类型** | `a_whole_string_template_keeps_the_original_type` |
| 8 | **不检查**残留 `{{` | `an_unresolved_template_is_rejected` |
| 9 | 嵌套**不递归**渲染 | `a_nested_template_is_rendered_recursively` |
| 10 | 默认槽数 4 → 16 | `the_concurrency_gate_defaults_to_four_slots` |
| 11 | 闸门**不扣减**槽位 | `the_concurrency_gate_limits_parallel_nodes`、`a_cloned_gate_shares_the_same_slots` |
| 12 | 中断**不再是**独立分类 | `the_node_error_has_three_distinct_classes` |
| 13 | 热路径引入 `Box<dyn NodeInvoke>` | `the_workflow_crate_does_not_use_box_dyn_on_the_node_hot_path` |
| 14 | `Arc<dyn NodeInvoke>` 出现在 `state.rs` | `the_dyn_trait_object_only_appears_at_the_graph_loading_site` |
| 15 | 装载处**不再**用 `NodeInvoke` | `the_graph_loading_site_uses_arc_dyn_node_invoke` |


## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

13 条 AC 全部可二元判定 —— 每条都指明「类型存在」「返回值 / 错误可观测」
「源码不含某标识符（结构层断言）」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.3、§3.4、§3.5、§2.10` —— 存在
- `docs/specs/p6/tasks.md T-003` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.5、§3.5.4` —— 存在
- `evalexpr` —— 已在 `crates/nexus-workflow/Cargo.toml` 声明

### #4 范围有界

**单 crate**（`nexus-workflow`）的**四个**新模块 + 测试。
纯内存计算，**无**数据库、**不新增**依赖方向、**不新增**业务码。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-workflow/src/node.rs` | **新增** |
| `crates/nexus-workflow/src/state.rs` | **新增** |
| `crates/nexus-workflow/src/condition.rs` | **新增** |
| `crates/nexus-workflow/src/template.rs` | **新增** |
| `crates/nexus-workflow/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-workflow/tests/node.rs` | **新增** |
| `crates/nexus-workflow/tests/architecture.rs` | **新增**（结构层断言） |
| `crates/nexus-workflow/Cargo.toml` | **修改**（加 `tokio.workspace = true` —— DoR #7 已指名） |
| `Cargo.lock` | **修改**（`tokio` 进入 `nexus-workflow` 的依赖列表） |

> ⚠️ DoR 的「受影响面」表原本写 `Cargo.toml` **不修改**，
> 但同一份 DoR 的 #7 门禁**指名**了「必须补上 `tokio.workspace = true`」。
> 两处**自相矛盾** —— 以更**具体**的 #7 为准（它指明了动作与理由），
> 本次实现按 #7 执行。

### #6 完成态可描述

调用方可以实现一个 `NodeInvoke` 交给图引擎；
条件分支按布尔结果走对分支，条件表达式写错时**报错**而不是走错分支；
工具参数模板里的 `{{...}}` 能拿到原始类型的值。

### #7 无隐形依赖

- 前置：NEX-38（已登记于 `backlog/tasks/NEX-38 - ....md`）
- `evalexpr` / `serde_json` / `tokio`（`Semaphore` 需 `tokio::sync`）——
  `tokio` **未**在 `crates/nexus-workflow/Cargo.toml` 里声明，
  **必须**在该任务内补上（`tokio.workspace = true`）并同步 `check-deps.sh` 规则 7 —— **这是已知的、已指名的动作**
  - **落地说明**：`tokio` 是**第三方**依赖，而 `check-deps.sh` 规则 7 只比对
    **内部** `nexus-*` 依赖（`awk` 的判据是 `$1 ~ /^nexus-[a-z0-9-]+[.]workspace$/`）——
    所以 `check-deps.sh` **无需修改**，规则 7 保持绿。
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：条件求值**失败**或结果**非布尔**时**报错**，**不**当 `false`。
- **依据**：条件分支的语义是「yes / no 哪个目标」。把「表达式写错」静默成 `false`
  = 图**走错分支**却**不报错** —— 那是最难排查的一类故障。
  参考架构 §3.5.1 只说条件求值用 `expr-lang/expr`，
  但「失败怎么办」没有定稿 —— 本 Spec 取**错误**这一侧，并显式登记为决策。
- **可逆性**：中 —— 语义变更要改错误类型与测试。
- **影响面**：`crates/nexus-workflow/src/condition.rs`。

### 类别：decide-and-proceed

- **决策**：`NodeError` 是**三分类** enum，不是字符串。
- **依据**：架构文档 §2.6.2「领域错误是 enum，不是字符串」。
  三类的**处置完全不同**（重试 / 结束 / 挂起），合并成一个字符串 = 处置变成运行时猜。
- **可逆性**：中 —— enum 变体是公开 API。
- **影响面**：`crates/nexus-workflow/src/node.rs`。

### 类别：decide-and-proceed

- **决策**：节点执行热路径**不含** `Box<dyn>`。
- **依据**：架构文档 §2.3.2「泛型优先，`dyn` 兜底」+ §2.3.3 的 `dyn` 保留场景。
  节点是**逐节点**的热路径；`Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**
  （异构集合）这一处 —— 结构层断言把这条钉死。
- **可逆性**：高 —— 断言可调整，但需要独立的决策记录。
- **影响面**：`crates/nexus-workflow/src/node.rs`、`tests/architecture.rs`。

### 类别：decide-and-proceed

- **决策**：AC #12 的结构层断言取**比原文更严**的形式 ——
  **整个 crate 的 `src/` 里不出现任何 `Box<dyn>`**，而不是只禁「热路径」。
- **依据**：AC 原文说的是「于节点执行热路径」。
  但「热路径」在源码里**没有**可判定的标记 ——
  「这个 `Box<dyn>` 在辅助函数里，不在节点主循环里」是**无法机械判定**的争论，
  而无法机械判定的规则**长期失效**。
  全 crate 禁止 = 热路径必然不含，**更严**的形式满足**更松**的要求。
  且 `Box<dyn>` 在本 crate 里**没有任何正当用途** ——
  异构集合用 `Arc<dyn>`，热路径用泛型。
- **可逆性**：中 —— 若将来确有必要用 `Box<dyn>`，需新的决策记录。
- **影响面**：`crates/nexus-workflow/tests/architecture.rs`。

### 类别：decide-and-proceed

- **决策**：状态大小超限时**回滚**（写操作前的状态逐字节恢复），**不**截断。
- **依据**：AC #4 只说「错误，不是截断」。
  但「报错」与「**报错且状态不变**」是**两件事**：
  若超限的键**留在**状态里，节点读到的就是**半应用**的状态 ——
  与「截断」一样是**静默的错误状态**，只是形态不同。
  「要么整次写入生效，要么完全没发生」是唯一可推理的语义。
  参考架构 §3.5.2 照抄 Go 版 `graph.go:367-369`（超限即报错），
  但 Go 版**没有**「状态已被部分修改」的问题（它先算再写）——
  Rust 版是「先写再量」，所以必须**显式回滚**。
- **可逆性**：低 —— 语义已写进测试，改动要同时改实现与断言。
- **影响面**：`crates/nexus-workflow/src/state.rs`、`crates/nexus-workflow/tests/node.rs`。

### 类别：decide-and-proceed

- **决策**：条件求值里**对象值**（JSON Object）**不支持**，但**不**让整个求值失败 ——
  装上下文时跳过不支持的值，只有表达式**真的引用**了它才报
  `ConditionError::UnsupportedValue`。
- **依据**：`evalexpr` 没有「字典」类型 —— 只有标量与元组。
  把 JSON 对象硬塞进 `Tuple` 会让 `a.b` 与 `a[0]` **语义混淆**。
  但「状态里有对象」≠「表达式引用了它」——
  前者不该让**整个**状态不可用于条件求值。
- **可逆性**：中 —— 映射表是公开行为。
- **影响面**：`crates/nexus-workflow/src/condition.rs`。

### 类别：decide-and-proceed

- **决策**：`NodeError::Interrupt` 的载荷在 NEX-39 只放**最小骨架**
  （`path` / `prompt`），完整的 `Interrupt`（含 `args_hash` / `resume_key`）由 NEX-40 落地。
- **依据**：AC #2 要求 `NodeError` 是三分类 enum ——
  而 enum 变体的载荷类型**必须在**本任务里就有名字，否则变体没法定义。
  但 `args_hash` 的语义（「批的是哪一次调用、哪些参数」）是 **NEX-40** 的核心 ——
  在本任务里凭空造一个 `args_hash` 只会造出一个**没有指纹语义**的空壳，
  然后 NEX-40 再改一遍。
  骨架里已写明「NEX-40 会替换为完整 `Interrupt`」。
- **可逆性**：高 —— NEX-40 落地时本来就是**要**改这个载荷。
- **影响面**：`crates/nexus-workflow/src/node.rs`。

## 五步法复核

**Five-step review (requirements first).**
1. **Requirement questioned**: 「节点执行契约」的需求来源是 Spec §3.3 / §3.4 / §3.5 与
   参考架构 §2.5 / §3.5.4。质疑点：**「契约」是否需要 `NodeRegistry`？**
   —— AC 只要求 `NodeInvoke` trait + 三分类错误。
2. **Deleted**: 删掉了「`NodeInput` / `NodeOutput` 用裸 `serde_json::Value`」的写法 ——
   裸 `Value` 让「键 → 值」的结构**隐含在调用方**，而 AC #3 要求
   `Ok(None)` 与「空产出」**语义可区分**；用带类型的键值表才表达得了。
   同时删掉了「`NodeContext` 可变」——节点**不直接改**状态，
   产出由驱动层合并（并发因此**无锁**）。
3. **Simplified or optimised**: `NodeOutput` / `NodeInput` 统一为「键 → JSON 值」，
   与 `GraphState` **同构** —— 「取上游产出」不需要额外转换层；
   状态容器改用 `serde_json::Map`（本 workspace 开了 `preserve_order`）——
   序列化结果**稳定**，对算哈希与比对快照有用。
4. **Accelerated**: 契约一次性覆盖**四个**模块（node / state / condition / template），
   而不是分四个任务 —— 四者**互相调用**（条件与模板都要读状态），
   拆开会让中间态**编译不过**。
5. **Automated**: AC #12 的「热路径不含 `Box<dyn>`」被改成**全 crate 禁止**，
   并**自动化**为 AST 级断言（`syn` 解析 trait 对象，注释**自动**不算）；
   另加了 15 处**变异测试**验证断言**真的**有效。

