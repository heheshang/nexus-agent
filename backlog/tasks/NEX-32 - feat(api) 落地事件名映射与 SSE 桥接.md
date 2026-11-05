---
id: NEX-32
title: "feat(api): 落地事件名映射与 SSE 桥接"
status: To Do
assignee: []
labels: [api, sse, p5]
priority: high
dependencies:
  - NEX-31
references:
  - "docs/specs/p5/spec.md §3.2、§3.3"
  - "docs/specs/p5/tasks.md T-002"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1"
---

## Description

**决策 D8 的落地**：手写 `Body::from_stream`，**不用** axum 的 `Sse`。

12 个事件名**原样**映射；事件流 → 帧序列 → 响应体。

### 为什么不用 axum 的 `Sse`

参考架构 §1.5 **已定稿**：

> axum 的 `Sse` 会用 `Event::default().data()` 自动转义，与 Go 版手写
> `event: / data:` 的字节输出不完全一致。**建议 Rust 版手写 `Body::from_stream`**。

前端 `lib/sse.ts` 是**按字节解析**的 —— 格式差异会表现为
「前端收不到事件」这类**极难排查**的问题。

### 事件名是前后端契约

`nexus_core::EventPayload` 的 12 个变体 → SSE `event:` 名：

`meta` / `delta` / `reasoning` / `plan` / `tool_call` / `tool_result` /
`agent_event` / `refs` / `usage` / `approval` / `done` / `error`

**不得**加前缀、不得改大小写、不得用 Rust 变体名（`ToolCall` ≠ `tool_call`）。

改一个名字，前端那个分支就**静默失效**了。

## 范围边界

### 做

- `crates/nexus-api/src/bridge.rs`：`event_name(&EventPayload) -> &'static str` + 桥接
- `crates/nexus-api/tests/sse_bridge.rs`：行为层测试

### 不做

| 项 | 理由 |
|---|---|
| 鉴权中间件 | 属 T-004 |
| 心跳的**定时器** | 心跳频率是**路由**的职责；桥接只做「事件 → 帧」 |
| 落库 / 事件泵 | 属 `nexus-service`（决策 D9） |

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/src/bridge.rs` 实现 `event_name(&EventPayload) -> &'static str`，**全部 12 个**事件名正确
- [ ] #2 事件名测试**覆盖全部 12 个** —— 不是抽查
- [ ] #3 `crates/nexus-api/src/bridge.rs` 实现事件流 → `Body::from_stream` 的桥接
- [ ] #4 桥接**手写**字节输出 —— 源码里**不含** `axum::response::sse`
- [ ] #5 事件流结束 → 流结束，**不补**额外的帧
- [ ] #6 事件流出错 → 发 `error` 帧，含**错误码**，**不含**内部原因（`cause`）
- [ ] #7 **客户端断开** → `CancellationToken` 触发
- [ ] #8 桥接的**每帧输出**都能被 T-001 的解码器**完整解回**
- [ ] #9 `crates/nexus-api/tests/architecture.rs` 断言源码里**不含** `axum::response::sse`
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

10 条 AC 全部可二元判定 —— 每条都指明「函数存在 / 行为可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.2、§3.3` —— 存在
- `docs/specs/p5/tasks.md T-002` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1` —— 存在
- `crates/nexus-agent/src/events.rs`（`EventStream` 类型）—— 存在

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增**一个**模块 + 一个测试文件。

**不触及**：42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/bridge.rs` | **新增** |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/tests/sse_bridge.rs` | **新增** |
| `crates/nexus-api/tests/architecture.rs` | **修改/新增** —— 结构层断言 |
| `crates/nexus-api/Cargo.toml` | **不修改** —— `tokio` / `axum` / `nexus-agent` / `nexus-core` 已在依赖里 |

### #6 完成态可描述

`nexus-api` 能把一条 `EventStream` 转成一个**真实可跑**的 SSE 响应体，
字节格式与参考实现一致，且客户端断开能触发取消。

### #7 无隐形依赖

- 前置任务 NEX-31 已登记在 `backlog/tasks/`
- `nexus-agent` 已在 `nexus-api` 的依赖里（`check-deps.sh` 允许）
- `nexus-core` 已在依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-31 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：桥接的输入类型是 `nexus_agent::events::EventStream`。
- **依据**：P4 已交付的 sink 产出的就是这条流；复用它**不重复造轮子**，
  且保证「序号严格单调」这条不变量**贯穿**到 SSE 帧。
- **可逆性**：中 —— 若 P4 的 sink 形态变化，桥接要跟着改。
- **影响面**：`crates/nexus-api/src/bridge.rs`。
