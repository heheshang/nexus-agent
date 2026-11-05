---
id: NEX-26
title: "feat(agent): 落地 trait 边界与协议完整性校验"
status: To Do
assignee: []
labels: [agent, react, contract]
priority: medium
dependencies: []
references:
  - "docs/specs/p4/spec.md §3.1、§6.1"
  - "docs/specs/p4/tasks.md T-001"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.3.2"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.2"
---

## Description

**P4 的地基** —— 所有后续任务都建立在这套 trait 与校验上。

`nexus-agent` 目前是 **7 行空壳**（只有 `CRATE_NAME`）。P4 要落地 ReAct 循环，
而 ReAct 循环的第一件事是「跟模型说话」和「跟工具说话」—— 那需要**两个 trait 边界**。

### 为什么 trait 由 `nexus-agent` 定义（决策 D4）

架构文档 §4.1.2 确立的 **consumer-defined trait** 手法：
「trait 由消费者定义（consumer-defined trait）」，
`rag::ChunkStore` 由 `repo` 实现，能力层**不反向依赖**数据层。

P4 的 `ChatModel` 是**消费者定义的** —— ReAct 循环是消费者，网关是实现方。

**但签名复用 `nexus-llm` 的数据类型**（决策 D4）。这是刻意的：
参考架构 §2.2 的禁令对象是**具体 `Gateway` 实现类型**
（「**不要**让 `nexus-agent::host` 依赖 `nexus-llm` 的具体 `Gateway` 类型」），
**不是**协议数据类型 —— 复用数据类型**不违反**该禁令。

自建一套 `Message` / `ToolDef` 会导致两套语义漂移 —— 那才是真正的浪费。

### 为什么 `InvokableTool` 必须 `dyn` 兼容

架构文档 §2.3.3 明确保留的 `dyn` 场景是**异构集合**：
工具注册表里混着内置工具、MCP 工具、用户自定义工具 —— 它们是**不同类型**，
必须放进同一个集合。所以 `InvokableTool` 需要 `#[async_trait]`。

而 `ChatModel` **不需要** —— §2.3.2 说「绝大多数能力接口用泛型」，
参考架构 §2.2 的性能提示也是「内部用泛型 + AFIT 获得静态分发」。

**这个区别是本任务最容易做错的地方** —— 两个 trait 的 `dyn` 策略**相反**。

### 为什么协议完整性校验**先于**裁剪

参考架构 §2.2 不变量 ③：
「`inputBudgetUsage` 在**丢弃任何历史之前**校验 tool_call ↔ tool_result 的配对完整性；
配对断裂直接报错而不是静默裁剪」。

理由是**协议完整性先于裁剪** —— 如果先裁剪，配对断裂会被裁剪**掩盖**，
模型收到一个静默损坏的协议，而错误信息指向「预算」而不是「协议非法」。
调试成本天差地别。

## 范围边界

### 做

- `nexus-agent` 的 `ChatModel` / `InvokableTool` trait 定义
- 协议完整性校验（配对 / 重复 ID / 缺失）
- `crates/nexus-agent/tests/architecture.rs` 的结构层断言

### 不做

| 项 | 理由 |
|---|---|
| ReAct 循环本体 | 属 T-004 |
| 上下文裁剪 | 属 T-002 |
| 事件 sink | 属 T-003 |
| `Gateway` → `ChatModel` 的**适配实现** | 属装配层（P5/P6）；P4 只定义 trait |

## Acceptance Criteria

- [ ] #1 `crates/nexus-agent/src/react.rs` 定义 `ChatModel` trait，方法签名用 `nexus_llm::types::{ChatRequest, ChatResponse, StreamEvent}`
- [ ] #2 `nexus-agent` **不引用** `nexus_llm::Gateway` 具体类型 —— `Gateway` 经 trait 注入
- [ ] #3 `ChatModel` trait 用**泛型 + 静态分发**，`nexus-agent` 源码里**不含** `Box<dyn ChatModel>`
- [ ] #4 `crates/nexus-agent/src/tools.rs` 定义 `InvokableTool` trait，且**是 `dyn` 兼容**的（工具注册表是异构集合）
- [ ] #5 `crates/nexus-agent/src/protocol.rs` 实现配对校验：孤立 tool_result、缺失 tool_result、重复 tool_call ID 三种情况均被拒
- [ ] #6 配对校验失败返回 `ErrorCode::InvalidParam`（10001）
- [ ] #7 配对校验的失败**发生在裁剪之前** —— 配对断裂时报的是配对错误，不是预算错误
- [ ] #8 `crates/nexus-agent/tests/architecture.rs` 断言 `nexus-agent` 不依赖 `nexus-repo` / `nexus-service` / `nexus-api`
- [ ] #9 `crates/nexus-agent/Cargo.toml` **不新增**任何内部 crate 依赖
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

10 条 AC 全部可二元判定 —— 每条都指明「某个文件里有什么」或「某个命令的退出码」。

### #2 无未决标记

无 `TBD` / `TODO` / `???` / 待定项。

「`ChatModel` 要不要 `dyn`」已**显式落定**（AC #3 用泛型，AC #4 的 `InvokableTool` 用 `dyn`），
依据是架构文档 §2.3.2 / §2.3.3 与参考架构 §2.2 —— **不是**未决问题。

### #3 引用可解析

- `docs/specs/p4/spec.md` —— 存在
- `docs/specs/p4/tasks.md` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.3.2` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.2` —— 存在

### #4 范围有界

单 crate 改动（`nexus-agent`）。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-agent/src/react.rs` | **新增** —— trait 定义 |
| `crates/nexus-agent/src/tools.rs` | **新增** —— trait 定义 |
| `crates/nexus-agent/src/protocol.rs` | **新增** —— 配对校验 |
| `crates/nexus-agent/tests/architecture.rs` | **新增** —— 结构层断言 |
| `crates/nexus-agent/Cargo.toml` | **可能**新增 `async-trait`（已在 workspace 依赖里） |

### #6 完成态可描述

调用方可以定义一个自己的 `ChatModel` 实现并注入 ReAct 循环；
非法工具调用协议会在**裁剪之前**被拒并返回 `InvalidParam`。

### #7 无隐形依赖

- `nexus-llm` / `nexus-core` **已**在 `nexus-agent` 的 `Cargo.toml` 里
- `async-trait` **已**在 workspace 根 manifest
- 无上游任务依赖（本任务是 P4 的第一块）

### 结论

**pass** —— 七项全过，可进入实现（S4）。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：`ChatModel` 用泛型静态分发，`InvokableTool` 用 `dyn`。
- **依据**：架构文档 §2.3.2（泛型优先）/ §2.3.3（异构集合保留 `dyn`）；
  参考架构 §2.2 的性能提示。两个 trait 的策略不同是因为**用途**不同。
- **可逆性**：高 —— 泛型 ↔ `dyn` 是签名级改动，调用方跟着改即可。
- **影响面**：`nexus-agent` 的两个 trait 定义。

### 类别：decide-and-proceed

- **决策**：协议完整性校验**先于**裁剪。
- **依据**：参考架构 §2.2 不变量 ③ 与 §3.2.2 的注释原文
  （「Validate the entire protocol before any history can be discarded」）。
  先裁剪会让配对断裂被掩盖，错误信息指向错误的原因。
- **可逆性**：高 —— 顺序调整是局部改动。
- **影响面**：`crates/nexus-agent/src/protocol.rs` 与 `compaction.rs` 的调用顺序。

---

## 实现过程中的发现

（待补）
