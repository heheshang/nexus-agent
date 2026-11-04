---
id: NEX-5
title: "feat(core): 落地事件负载类型与单写者事件流"
status: To Do
assignee: []
labels: [core, events, p1]
priority: high
dependencies:
  - NEX-4
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1"
  - "examples/demo-event-payload/"
---

## Description

架构文档 §6.1 的 **P1 核心类型**第二部分：把 §2.2.4.1 的 12 个事件类型与 §2.2.3 的「单写者 + 显式序号」搬进 `nexus-core`。

事件名是 SSE `event:` 字段的值，**前端按它分支** —— 少一个或改一个就是破坏性变更。

### §2.2.4.1 事件类型清单（定稿，12 个）

| 事件名 | 变体 | 字段 | 通道 |
|---|---|---|---|
| `meta` | `Meta` | `session_id`, `agent_id` | 必达 |
| `delta` | `Delta` | `content` | 可丢 |
| `reasoning` | `Reasoning` | `content` | 可丢 |
| `plan` | `Plan` | `steps`, `current` | 必达 |
| `tool_call` | `ToolCall` | `call_id`, `name`, `arguments` | 必达 |
| `tool_result` | `ToolResult` | `call_id`, `result`, `elapsed_ms`, `status` | 必达 |
| `agent_event` | `AgentEvent` | `kind`, `expert`, `detail` | 必达 |
| `refs` | `Refs` | `items` | 必达 |
| `usage` | `Usage` | `prompt_tokens`, `completion_tokens`, `cost`, `model` | 必达 |
| `approval` | `Approval` | `approval_id`, `kind`, `payload` | 必达 |
| `done` | `Done` | `finish_reason` | 必达 |
| `error` | `Error` | `code`, `message` | 必达 |

**两处刻意偏离参考实现的字段名**（协议破坏性变更，定稿时已明确）：
- `id` → `call_id`（`id` 在事件里语义过泛）
- `ms` → `elapsed_ms`（单位后缀显式化）

**`agent_event` 的内层 `kind`**（`delegate` / `progress` / `merge`）—— 参考实现用**第二个** `type` 字段，本设计改名 `kind` 避免混淆。

### §2.2.3 单写者保证

- `EventSink` **不可 `Clone`** —— 物理上杜绝多写者
- `emit(&mut self, ...)` —— 独占借用，编译期串行化
- 序号分配与写入在同一临界区 —— 顺序天然一致，**严格单调、无重复、无空洞**
- 消费侧 `EventStream = Pin<Box<dyn Stream<Item = Result<Event, AppError>> + Send>>`

### 两个附录 C.1 陷阱

1. `#[serde(flatten)]` **不能**标注在 enum 的**变体**上（内部 tag 与 enum 级 flatten 不兼容）—— 只能用于变体**内部的 struct 字段**
2. `enum` 变体带数据时 `rename_all` 的形状是 `Failed { code }` → `{"failed": {"code": …}}` 而非 `{"code": …}` —— 契约测试固化实际形状

### 依赖

需要 NEX-4 的 `RunId`（`Event.run_id` 字段的类型）。

## Acceptance Criteria

- [ ] #1 `nexus-core` 有 `event` 模块，含 §2.2.4.1 表中的 **12 个**事件变体，字段名与表**逐字一致**
- [ ] #2 事件负载用 `#[serde(tag = "type", rename_all = "snake_case")]`，序列化后 `type` 与其他字段**同级**（扁平 JSON）
- [ ] #3 `agent_event` 的内层字段名是 `kind`（不是第二个 `type`）
- [ ] #4 逃生舱 `Extension` 变体的 `#[serde(flatten)]` 标注在**变体内部的 struct 字段**上，不是变体本身
- [ ] #5 `EventSink` **不可 `Clone`**（不 derive `Clone`），`emit` 取 `&mut self`
- [ ] #6 序号从 1 开始、严格递增、与写入顺序一致
- [ ] #7 `ToolStatus` / `FinishReason` / `AgentEventKind` 是 **enum**，不是字符串
- [ ] #8 **契约测试**：事件名清单与 §2.2.4.1 表逐条比对（少一个、错一个、顺序不同都失败）
- [ ] #9 **契约测试**：12 个变体全部往返序列化（`serialize` → `deserialize` → 再 `serialize` 等价）
- [ ] #10 **契约测试**：字段名与 §2.2.4.1 表逐条比对 —— `call_id` 不是 `id`，`elapsed_ms` 不是 `ms`
- [ ] #11 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #12 CI 六道门禁全绿（含 `scripts/check-naming.sh`）

## 备注

- `Event.run_id` 用 NEX-4 的 `RunId`，不是裸 `String`。
- `Usage.cost` 用 `Option<String>`（十进制字符串）而不是 `f64` —— 浮点在跨系统传递中会丢精度；这是 demo 的选择，架构文档未明确，需要时按契约表确认。
