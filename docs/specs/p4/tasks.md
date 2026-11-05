# P4 Agent 运行时 — Tasks

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 契约见 `spec.md`；机器可读的不变量见 `contracts/`。
>
> 布局遵循 spec-kit `v0.8-headings`（每个任务一个 `### T-NNN — <title>` 标题）。
> `taskId` 形如 `T-NNN`，与 `backlog/tasks/` 下的 `NEX-nn` 一一对应。

## Tasks

### T-001 — 落地 trait 边界与协议完整性校验

**P4 的地基**：所有后续任务都建立在这套 trait 与校验上。

`nexus-agent` 定义**自己的** `ChatModel` 与 `InvokableTool` trait（决策 D4），
签名复用 `nexus-llm` 的数据类型；`Gateway` 经 trait 注入，**不引用**具体类型。

同时落地**协议完整性校验**（`tool_call ↔ tool_result` 配对）——
它**先于**任何裁剪，是 §3.2 的第 1 步。

对应 backlog 任务：`backlog/tasks/NEX-26 - feat(agent) 落地 trait 边界与协议完整性校验.md`
依赖：无
对应设计条款：架构文档 §2.3.2、§2.3.3、§4.1.2；参考架构 §2.2、§3.2.2

- AC: `crates/nexus-agent/src/react.rs` 定义 `ChatModel` trait，方法签名用 `nexus_llm::types::{ChatRequest, ChatResponse, StreamEvent}`
- AC: `nexus-agent` **不引用** `nexus_llm::Gateway` 具体类型 —— `Gateway` 经 trait 注入
- AC: `ChatModel` trait 用**泛型 + 静态分发**，`nexus-agent` 源码里**不含** `Box<dyn ChatModel>`
- AC: `crates/nexus-agent/src/tools.rs` 定义 `InvokableTool` trait，且**是 `dyn` 兼容**的（工具注册表是异构集合）
- AC: `crates/nexus-agent/src/protocol.rs` 实现配对校验：孤立 tool_result、缺失 tool_result、重复 tool_call ID 三种情况均被拒
- AC: 配对校验失败返回 `ErrorCode::InvalidParam`（10001）
- AC: 配对校验的失败**发生在裁剪之前** —— 配对断裂时报的是配对错误，不是预算错误
- AC: `crates/nexus-agent/tests/architecture.rs` 断言 `nexus-agent` 不依赖 `nexus-repo` / `nexus-service` / `nexus-api`
- AC: `crates/nexus-agent/Cargo.toml` **不新增**任何内部 crate 依赖
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-002 — 落地上下文整轮淘汰

**决策 D5 的落地**：超预算时按**整轮**删除历史，不生成摘要。

对应 backlog 任务：`backlog/tasks/NEX-27 - feat(agent) 落地上下文整轮淘汰.md`
依赖：T-001
对应设计条款：架构文档 §3.1.1；参考架构 §2.2 不变量 ①③、§3.2.1、§3.2.2

- AC: `crates/nexus-agent/src/compaction.rs` 实现 `compact_input_history` —— **纯函数**，不修改入参
- AC: 保留**所有** system 消息
- AC: 保留**首个 user 之前的前缀**
- AC: 保留**最新一轮 user** 轮
- AC: 历史 user 轮按**整轮**删除 —— 该轮的 assistant 消息与**所有**并行 tool 结果**一起**被删，不留孤立 tool_result
- AC: **不生成摘要** —— 保留的消息是原样的，未新增任何摘要消息
- AC: 裁剪后**复核**协议完整性 —— 裁剪结果仍然配对完整
- AC: 仍超预算时返回 `ErrorCode::BudgetExceeded`（30006）
- AC: 边界值双向测试：恰好在预算内通过、超出即拒绝
- AC: 工具定义计入预算
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-003 — 落地事件 sink 与背压

**决策 D7 的落地**：P4 自定义异步 sink，不复用 `nexus-core::EventSink`。

对应 backlog 任务：`backlog/tasks/NEX-28 - feat(agent) 落地事件 sink 与背压.md`
依赖：T-001
对应设计条款：架构文档 §2.2.3、§3.2；参考架构 §2.2 方案 C、§3.2.3

- AC: `crates/nexus-agent/src/events.rs` 定义 sink，采用**单点分配**序号（ingress 通道 → 唯一分配 task → output 通道）
- AC: 事件序号**严格单调、无重复、无空洞**，且与写入顺序一致
- AC: 序号分配与写入**天然原子** —— 不使用 `Mutex`
- AC: **双通道**：`delta` / `reasoning` 走**可丢**通道，其余全部走**必达**通道
- AC: 填满可丢通道后，**终态事件仍然送达**
- AC: 背压超时**主动取消**整个运行，而不是无限阻塞
- AC: 事件负载复用 `nexus_core::EventPayload` —— 事件名与负载形状**不新增**（12 个事件名不变）
- AC: `nexus-agent` **不**复用 `nexus_core::EventSink`（两者形状不同：同步单写者 vs 并发生产者 + 背压）
- AC: 并发工具调用下序号仍严格单调 —— 有测试
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-004 — 落地 ReAct 循环本体

**P4 完成判据的落点**：ReAct 循环有测试。

对应 backlog 任务：`backlog/tasks/NEX-29 - feat(agent) 落地 ReAct 循环本体.md`
依赖：T-001、T-002、T-003
对应设计条款：架构文档 §3.4；参考架构 §2.2、§3.2.4

- AC: `crates/nexus-agent/src/react.rs` 实现 ReAct 循环，管道顺序为：配对校验 → 裁剪 → 模型调用 → 输出校验 → 工具执行 → 结果回灌
- AC: 循环跑通**至少两轮**：模型要求工具 → 工具执行 → 结果回灌 → 模型给出最终答案
- AC: **三个安全阀**全部生效：最大迭代（`MaxIteration` 30002）、单步超时（`ModelTimeout` 50002）、总超时（`ModelTimeout` 50002）
- AC: 工具失败**结构化回灌**为 `tool` 角色消息（形状 `{"error": ..., "message": ...}`），且**不终止**循环 —— 模型仍能给出答案
- AC: 工具输出回灌时是 **`tool` 角色**消息，**不是** `assistant` 消息 —— 工具输出是数据，不是指令
- AC: 工具并发用 `Semaphore` 限流，同时执行的工具数**不超过**上限
- AC: 工具输出**超长被截断**，且截断保持 UTF-8 边界
- AC: 取消的终态是 `done`（**不是** `error`）—— 取消 ≠ 失败
- AC: **先固化错误再取消** —— 供应商失败不会被 `cancel` 覆盖成「用户取消」
- AC: 迭代上限 / 超时 / 工具并发上限**全部由调用方传入**，`nexus-agent` **不内置**隐式默认值
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-005 — P4 收口：架构文档同步与契约升格

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

对应 backlog 任务：`backlog/tasks/NEX-30 - P4-closeout-docs-and-contract-promotion.md`
依赖：T-001、T-002、T-003、T-004
对应设计条款：架构文档 §5.4、§6.1；参考架构 §2.2

- AC: 架构文档 §5.4 不变量表已补 P4 已验证项（逐条对应 `contracts/invariants.json` 的 25 条）
- AC: 架构文档 §6.1 的 P4 行已标记为完成，并写明完成判据的达成方式
- AC: `docs/specs/p4/contracts/invariants.json` 中所有 `planned` 条目**已升格**为 `verified`
- AC: **每个 `planned` 条目引用的测试都真的存在** —— `bash scripts/check-specs.sh` 规则 3b 通过
- AC: 架构文档 §2.6.4.1 的业务码表未被改动（P4 不新增码值），有 diff 证据
- AC: 架构文档 §2.2.4.1 的事件清单未被改动（P4 不新增事件名），有 diff 证据
- AC: `nexus-agent` 的 crate 级文档写明「负责什么 / 不负责什么」「trait 由本 crate 定义」「不新增依赖方向」
- AC: 决策 D4~D7 的**反方论点与回应**已写入 Spec §6，且后续任务 F1 / F2 的**触发条件**明确
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- AC: CI 全部门禁全绿
