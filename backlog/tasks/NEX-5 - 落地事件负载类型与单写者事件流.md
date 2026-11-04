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

- [x] #1 `nexus-core` 有 `event` 模块，含 §2.2.4.1 表中的 **12 个**事件变体，字段名与表**逐字一致**
- [x] #2 事件负载用 `#[serde(tag = "type", rename_all = "snake_case")]`，序列化后 `type` 与其他字段**同级**（扁平 JSON）
- [x] #3 `agent_event` 的内层字段名是 `kind`（不是第二个 `type`）
- [x] #4 逃生舱 `Extension` 变体的 `#[serde(flatten)]` 标注在**变体内部的 struct 字段**上，不是变体本身
- [x] #5 `EventSink` **不可 `Clone`**（不 derive `Clone`），`emit` 取 `&mut self`
- [x] #6 序号从 1 开始、严格递增、与写入顺序一致
- [x] #7 `ToolStatus` / `FinishReason` / `AgentEventKind` 是 **enum**，不是字符串
- [x] #8 **契约测试**：事件名清单与 §2.2.4.1 表逐条比对（少一个、错一个、顺序不同都失败）
- [x] #9 **契约测试**：12 个变体全部往返序列化（`serialize` → `deserialize` → 再 `serialize` 等价）
- [x] #10 **契约测试**：字段名与 §2.2.4.1 表逐条比对 —— `call_id` 不是 `id`，`elapsed_ms` 不是 `ms`
- [x] #11 **负向测试**（追加）：`EventSink` 不可 `Clone`（E0599）、事件字段名写错（E0559）、写入需独占借用（E0499），`.stderr` 快照入库
- [x] #12 **通道语义**：`EventChannel` 类型化区分可丢/必达，只有 `delta` / `reasoning` 可丢 —— 由契约测试固化
- [x] #13 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #14 CI 六道门禁全绿（含 `scripts/check-naming.sh`）

## 备注

- `Event.run_id` 用 NEX-4 的 `RunId`，不是裸 `String`。
- `Usage.cost` 用 `Option<String>`（十进制字符串）而不是 `f64` —— 浮点在跨系统传递中会丢精度；这是 demo 的选择，架构文档未明确，需要时按契约表确认。
- `EventStream` 的错误类型用 `EventStreamError` 而非 `AppError` —— 后者属于 NEX-6，且 `nexus-core` 不该定义业务码。
- `Meta.session_id` 用 `SessionId`、`Meta.agent_id` 用 `AgentId`、`ToolCall.call_id` 用 `ToolCallId` —— 都走 NEX-4 的类型化 ID，不裸用 `String` / `i64`。

## 实现过程中的发现

**1. `EventStream` 的错误类型不能是 `AppError`。**
架构文档 §2.2.3 写的是 `EventStream = Pin<Box<dyn Stream<Item = Result<Event, AppError>> + Send>>`，
但 `AppError` 是 NEX-6 的产物，且它承载**业务码**（42 个码的枚举）。
事件流本身不产生业务错误 —— 它只是「流出了故障」（如来源通道关闭）。
所以本任务定义 `EventStreamError`（只有 `SourceClosed`），
等 NEX-6 落地后由上层 crate 做 `From<EventStreamError> for AppError` 的转换。

**2. 三个负向用例的错误码与文档预期不同（已按实际快照修正）。**
初稿按直觉写了错误码，实际编译结果：
- 字段名写错 → **E0559**（`variant ... has no field named`），不是 E0560
- 重复可变借用 → **E0499**，不是 E0502
这类偏差正是 `.stderr` 快照存在的意义 —— 快照是**编译器实际行为**的记录，
文档注释必须跟着实际走。

**3. 「独占借用」用例的写法需要真正制造重叠借用。**
最初写两次 `sink.emit(...)` 并列，实际**不报错** —— 两次调用是顺序的，
前一个的返回值不持有借用，NLL 让第二个调用合法。
改成同时持有 `&mut sink` 的两个引用才触发 E0499。
这说明「`&mut self` 保证串行化」这条性质，**不能靠直觉断言**，得让编译器证明。

**4. `Usage.cost` 的类型是个真实的设计空白。**
架构文档 §2.2.4 写 `cost: Option<Decimal>`，但 `Decimal` 来自 `rust_decimal` crate ——
workspace 没有这个依赖，且它对 `nexus-core` 这种纯类型 crate 偏重。
demo 用的是 `Option<String>`（十进制字符串）。
本任务按 demo 走 `Option<String>`，理由是：成本要**累加并对账**，
浮点会丢精度，字符串在 JSON 里也保持原样透传。
但这与架构文档的 `Option<Decimal>` **不完全一致** —— 已记在备注里，
若后续要求严格对齐，需要引入 `rust_decimal` 并同步修改事件契约。

