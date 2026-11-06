---
id: NEX-39
title: "feat(workflow): 落地节点执行契约与图状态"
status: To Do
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

- [ ] #1 `crates/nexus-workflow/src/node.rs` 定义 `NodeInvoke` trait —— `invoke` 返回 `Result<Option<NodeOutput>, NodeError>`
- [ ] #2 `NodeError` 是**三分类** enum：`Retryable` / `Permanent` / `Interrupt`（不是字符串）
- [ ] #3 `Ok(None)` 表示「**无产出但正常**」，与 `Err` 区分
- [ ] #4 `crates/nexus-workflow/src/state.rs` 定义 `GraphState`（键 → JSON 值）；状态大小 > **4 MiB** → **错误**（不是截断）
- [ ] #5 并发槽用 `Semaphore`（默认 **4**）；节点任务用 `acquire_owned()`
- [ ] #6 条件求值用 `evalexpr`，结果**必须**是布尔；非布尔 → **错误**
- [ ] #7 条件求值**失败** → **错误**，**不是** `false`（否则走错分支）
- [ ] #8 条件里的变量**不存在** → **错误**，不是 `false`
- [ ] #9 模板渲染：整串是 `{{...}}` 且**只出现一次** → **保留原始类型**（数字仍是数字）
- [ ] #10 模板渲染：插值结果**仍含** `{{` → **错误**（未解析的工具参数模板）
- [ ] #11 模板渲染：嵌套 map / slice **递归**渲染
- [ ] #12 `crates/nexus-workflow` 的源码**不含** `Box<dyn` 于节点执行热路径 —— `Arc<dyn NodeInvoke>` 只允许出现在**图定义装载**处（结构层断言）
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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
| `crates/nexus-workflow/Cargo.toml` | **不修改** |

### #6 完成态可描述

调用方可以实现一个 `NodeInvoke` 交给图引擎；
条件分支按布尔结果走对分支，条件表达式写错时**报错**而不是走错分支；
工具参数模板里的 `{{...}}` 能拿到原始类型的值。

### #7 无隐形依赖

- 前置：NEX-38（已登记于 `backlog/tasks/NEX-38 - ....md`）
- `evalexpr` / `serde_json` / `tokio`（`Semaphore` 需 `tokio::sync`）——
  `tokio` **未**在 `crates/nexus-workflow/Cargo.toml` 里声明，
  **必须**在该任务内补上（`tokio.workspace = true`）并同步 `check-deps.sh` 规则 7 —— **这是已知的、已指名的动作**
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
