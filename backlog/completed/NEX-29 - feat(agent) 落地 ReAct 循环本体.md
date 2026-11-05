---
id: NEX-29
title: "feat(agent): 落地 ReAct 循环本体"
status: Done
assignee: []
labels: [agent, react]
priority: medium
dependencies:
  - NEX-26
  - NEX-27
  - NEX-28
references:
  - "docs/specs/p4/spec.md §3.1、§3.3、§3.4、§3.6"
  - "docs/specs/p4/tasks.md T-004"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.4"
---

## Description

**P4 完成判据的落点** —— 架构文档 §6.1：「**ReAct 循环有测试**」。

前三个任务铺好了地基：trait 边界（T-001）、历史裁剪（T-002）、事件 sink（T-003）。
本任务把它们装成**循环**。

### 循环的管道顺序

```
run(input, tools, cancel)
  → 协议完整性校验（tool_call ↔ tool_result 配对）        ← 先于任何裁剪
  → 上下文裁剪（整轮淘汰）                                  ← 每步都重跑
  → 模型调用（ChatModel::chat）
  → 校验本步输出（tool_call ID / 最终答案）
  → 有工具调用？
      ├─ 否 → 发 done，终态
      └─ 是 → 并发执行工具（Semaphore 限流）
                → 工具结果结构化回灌为 tool 角色消息
                → 发 tool_result 事件
                → 回到循环开头
```

### 三个安全阀

参考架构 Phase 1「必抄的不变量」②：三个安全阀（最大迭代、单步超时、总超时）。

| 阀 | 触发 | 终态 |
|---|---|---|
| 最大迭代 | 迭代次数 > `max_iterations` | `error` / `MaxIteration`（30002） |
| 单步超时 | 单次模型调用超时 | `error` / `ModelTimeout`（50002） |
| 总超时 | 整个运行超时 | `error` / `ModelTimeout`（50002） |

**三个阀的值全部由调用方传入**（决策 D6）—— `nexus-agent` **不内置**默认值。
理由：缺省值会让「没配」与「配成 0」**无法区分**。

### 工具失败**不终止**循环

这是 Agent **自我修正**能力的前提。参考架构 Phase 1「必抄的不变量」①：
「**工具错误结构化回灌**而不是抛异常」。

工具失败被包成观察（`{"error": ..., "message": ...}`），
作为 `tool` 角色消息回灌给模型 —— 模型看到错误可以换个参数重试。

**但**：工具输出是**不可信输入**，回灌时必须是 `tool` 角色，
**不能**是 `assistant` 角色 —— 否则工具输出就变成了**指令**。

### 取消 ≠ 失败

参考架构 §3.2.4：取消的终态是 `done`，**不是** `error`。

**关键顺序**：先固化错误，再 `cancel()` ——
否则供应商失败会被 `cancel` 覆盖成「用户取消」
（参考实现 `eino_runner.go:216-218` 的注释原文）。

## 范围边界

### 做

- ReAct 循环本体
- 三个安全阀
- 工具并发限流
- 工具输出截断
- 终态分类

### 不做

| 项 | 理由 |
|---|---|
| 危险工具的授权边界 | 属 P8；P4 只做「工具错误结构化回灌」的语义 |
| Schema 子集白名单 | 属 P8；P4 只把 `parameters` 原样透传 |
| 多智能体路由 / Host | P4 是**单 Agent** |
| Checkpoint / 中断恢复 | 属 P6 |
| 装配层（谁构造 ReActRunner） | 属 P5/P6 |

## Acceptance Criteria

- [x] #1 `crates/nexus-agent/src/react.rs` 实现 ReAct 循环，管道顺序为：配对校验 → 裁剪 → 模型调用 → 输出校验 → 工具执行 → 结果回灌
- [x] #2 循环跑通**至少两轮**：模型要求工具 → 工具执行 → 结果回灌 → 模型给出最终答案
- [x] #3 **三个安全阀**全部生效：最大迭代（`MaxIteration` 30002）、单步超时（`ModelTimeout` 50002）、总超时（`ModelTimeout` 50002）
- [x] #4 工具失败**结构化回灌**为 `tool` 角色消息（形状 `{"error": ..., "message": ...}`），且**不终止**循环 —— 模型仍能给出答案
- [x] #5 工具输出回灌时是 **`tool` 角色**消息，**不是** `assistant` 消息 —— 工具输出是数据，不是指令
- [x] #6 工具并发用 `Semaphore` 限流，同时执行的工具数**不超过**上限
- [x] #7 工具输出**超长被截断**，且截断保持 UTF-8 边界
- [x] #8 取消的终态是 `done`（**不是** `error`）—— 取消 ≠ 失败
- [x] #9 **先固化错误再取消** —— 供应商失败不会被 `cancel` 覆盖成「用户取消」
- [x] #10 迭代上限 / 超时 / 工具并发上限**全部由调用方传入**，`nexus-agent` **不内置**隐式默认值
- [x] #11 裁剪在**每步**都重跑（不是只在第一步）
- [x] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对 —— 
> 本仓库的 `pipeline-cli` 尚未构建（`dist/` 缺失），按 `ai-sdlc-dor-gate.mdc` §5 执行。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「循环在什么输入下产生什么终态」或「消息的角色是什么」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

「限流的默认值是多少」已**显式落定**（决策 D6：调用方传入，不内置默认值，AC #10）。

### #3 引用可解析

- `docs/specs/p4/spec.md §3.1、§3.3、§3.4、§3.6` —— 存在
- `docs/specs/p4/tasks.md T-004` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.4` —— 存在

### #4 范围有界

单 crate 改动（`nexus-agent`）。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-agent/src/react.rs` | **修改** —— 循环本体 |
| `crates/nexus-agent/src/tools.rs` | **修改** —— 工具执行与截断 |
| `crates/nexus-agent/tests/react_loop.rs` | **新增** —— 行为层测试 |

**不新增依赖**（`tokio` / `futures` / `tokio-util` **已**在 `nexus-agent` 的依赖里）。

### #6 完成态可描述

调用方传入模型、工具与三个阀的值，就能跑一次完整的 Agent 对话：
模型要工具就调工具、工具失败就回灌让模型自己纠正、模型给答案就发 `done`。

### #7 无隐形依赖

- NEX-26 / NEX-27 / NEX-28 均已登记在 `backlog/tasks/`（trait 边界、裁剪、事件 sink）
- `tokio` / `futures` / `tokio-util` **已**在 `nexus-agent` 的 `Cargo.toml` 里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，**但需 NEX-26 / NEX-27 / NEX-28 完成后开工**。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：三个安全阀与工具并发上限的值**全部由调用方传入**，`nexus-agent` 不内置默认值。
- **依据**：缺省值会让「没配」与「配成 0」无法区分 —— 与 `nexus-llm::GatewayConfig::from_config`
  的既有纪律一致（0 值必须回退到缺省）。把默认值放在**装配层**而不是**运行时 crate**，
  让「配置在哪」只有一个答案。详见 `docs/specs/p4/spec.md` §6.4。
- **可逆性**：高 —— 加一个 `Default` impl 是纯增量。
- **影响面**：`nexus-agent` 的构造函数签名。

### 类别：decide-and-proceed

- **决策**：工具失败**不终止**循环，错误以 `{"error", "message"}` 形状回灌。
- **依据**：参考架构 Phase 1「必抄的不变量」① —— 工具错误结构化回灌而不是抛异常；
  这是 Agent 自我修正能力的前提。
- **可逆性**：中 —— 改变回灌形状会影响模型的自我修正行为。
- **影响面**：`crates/nexus-agent/src/tools.rs` 与 `react.rs` 的回灌路径。

---

## 实现过程中的发现

### 设计选择 —— 终态事件由**调用方**发

`ReactLoop::run` **只**返回终态分类，**不**发终态事件。

理由：「先固化错误，再取消」的顺序要求终态事件的**发送**发生在
错误固化**之后**，而发送需要调用方掌握取消令牌 —— 如果循环内部自己发，
顺序就拧了。

调用方的顺序：
1. `run()` → 拿到 `Outcome`；
2. 按 `Outcome` 发终态事件（`done` / `error`）；
3. **然后**取消上游工作。

### 发现 1 —— `Outcome` 不能存 `AppError`

`AppError` **不实现** `Clone` / `Eq` —— 而终态需要能被**比较**（测试要断言）。

改成存**码** + **消息**：

```rust
Outcome::Failed { code: ErrorCode, message: String, iterations: usize }
```

### 发现 2 —— 工具并发限流的测试要**共享计数器**

第一版的工具各自持有计数器，但工具是 `Arc<dyn InvokableTool>` ——
**拿不到具体类型**，读不到峰值。

改成所有工具共享一个 `Arc<AtomicUsize>` 峰值计数器。

另外，第一版的测试**跑了两遍**循环（第一遍用 `dyn` 读不到峰值，白跑）。
删掉第一遍。

### 发现 3 —— 预算必须**精确**算，不能硬编码

第一版硬编码 `cfg.context_budget = 200` —— 结果**全部**红。

根因：预算的口径是 `body + framing`，而 `framing` 依赖**消息条数**与
**工具定义数**。硬编码的魔数对不上真实内容。

而且 —— 第一轮就超预算时，模型**根本没被调用**（在构造请求阶段就失败了），
所以「模型被调用几次」这个断言本身就是错的。

修法：
1. 把 `estimate_body_bytes` / `framing_bytes` **提升为 `pub`** ——
   调用方需要用**同一个口径**算预算；
2. 测试用 `estimate_total_budget(&expected_history, &tool_defs)` 算出精确预算。

> **顺带暴露了一个真实的设计缺口**：
> `estimate_body_bytes` 与 `framing_bytes` 之前是**私有**的 ——
> 外部根本无法知道「一次请求占多少预算」。
> 这不是测试的问题，是**API 的问题**。已修。

### 发现 4 —— 「裁剪每步都跑」的证明方式是**间接**的

第一版断言「模型被调用两次」—— 但实际是**一次**：第二步在**构造请求**时
就超预算了，压根没走到模型调用。

这其实**更**是证据：如果裁剪只在第一步跑，第二步会用**未裁剪**的历史
去调模型 —— 就会被调用两次。

所以正确的断言是「模型只被调用一次」+「报 `BudgetExceeded`」。

### 发现 5 —— 「裁剪确实淘汰了东西」需要**单独**一个测试

「裁剪每步都跑」和「裁剪确实淘汰」是两件事。

而且要测淘汰，第一轮必须**大到**超预算 —— 两轮都很短的话，
step 1 的总量反而比 step 2 的保留集还小，那就不触发淘汰。

另外，保留集的**最坏情况**要**精确**重建（assistant 消息**带工具调用**、
tool 结果是**真实输出**）—— 否则第二步还是会超预算。

### 发现 6 —— 模块文档里的 ASCII 图会被当成 **doctest**

`//! ```` 里的 `→` `←` `├─` 被 rustdoc 当成 Rust 代码编译 → 编译错误。

改成 ` ```text ` —— 明确标记为**纯文本**。

### 门禁自身的防伪验证

六条关键断言都用**注入故障**验证过会红：

| 注入 | 结果 |
|---|---|
| 工具失败**直接终止**循环 | `a_tool_failure_is_fed_back_as_a_structured_observation` FAILED ✓ |
| 工具结果用 **`assistant` 角色**回灌 | 10 个测试同时红 ✓ |
| 迭代上限**不生效** | `the_iteration_limit_terminates_the_loop` FAILED ✓ |
| 取消被**当成失败** | `cancellation_is_not_a_failure` FAILED ✓ |
| 工具并发限流**失效** | `concurrent_tool_execution_respects_the_limit` FAILED ✓ |
| **跳过**协议校验与裁剪 | 3 个测试同时红 ✓ |

### 验证结果

- `cargo test -p nexus-agent`：65 个测试（26 单元 + 5 结构层 + 14 事件 + 20 ReAct）全绿
- `cargo test --workspace --locked`：448 通过 / 0 失败（原 428）
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
