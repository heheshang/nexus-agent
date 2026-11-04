---
id: NEX-4
title: "feat(core): 落地类型化 ID 与取消语义类型状态"
status: To Do
assignee: []
labels: [core, typestate, p1]
priority: high
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.1"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.8"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.8.1"
  - "examples/demo-cancel-typestate/"
---

## Description

架构文档 §6.1 的 **P1 核心类型**第一部分：把 §2.8.1 的 18 种 ID 类型与 §2.1 的取消语义类型状态搬进 `nexus-core`。

`nexus-core` 当前是 19 行占位（只有一个 `CRATE_NAME` 常量）。它是**依赖图的根**（§4.4 规则 2：零内部依赖），所以它放的是**所有 crate 都要用的纯类型**。

### §2.8.1 ID 类型（定稿，18 种）

| 类型 | 底层 | 类型 | 底层 |
|---|---|---|---|
| `UserId` | `i64` | `KbId` | `i64` |
| `SessionId` | `Uuid` | `DocumentId` | `i64` |
| `AgentId` | `i64` | `DatasetId` | `i64` |
| `MessageId` | `i64` | `CaseId` | `i64` |
| `RunId` | `i64` | `VectorId` | `String` |
| `WorkflowId` | `i64` | `TraceId` | `String` |
| `ToolId` | `i64` | `InterruptId` | `String` |
| `ToolCallId` | `String` | `ModelConfigId` | `i64` |
| `ProviderId` | `i64` | `EmbeddingConfigId` | `i64` |

**序列化约定（重要，附录 C.1 陷阱）**：ID 序列化为**裸值**（`SessionId` → UUID 字符串），**不是** `{"value": ...}`。搞错会导致数据库写入失败。

### §2.1 取消语义（定稿）

- `RequestScope` —— 可取消，持有父的子 token
- `DetachedScope` —— **不可取消**，持有独立的 `CancellationToken::new()`，**不持有父 token 引用**，也不对外暴露
- `DetachedScope` 上**不存在** `cancel()` / `token()` / `subscribe()`
- **规则 A**：`DetachedScope` 只能通过 `detach()` 构造，且 `detach()` 只能从 `RequestScope` 调用
- **规则 B**：`DetachedScope::persist` 是唯一能落库的方法，`RequestScope` 上**没有**落库方法

§2.1.6 提到 `RunScope` —— 但架构文档未给它定稿形态（未说明与 `RequestScope` 的关系）。**本任务只实现 `RequestScope` 与 `DetachedScope`**，`RunScope` 留到有定稿时再做（避免在实现里「顺手决定」）。

### 参考实现

`examples/demo-cancel-typestate/` —— 但**不能直接搬**：demo 用 `Arc<dyn Fn()>` 回调模拟取消，而架构文档 §2.1.5 定稿的是 `tokio_util::sync::CancellationToken`（`new()` vs `child_token()` 方向相反，已实测确认）。

## Acceptance Criteria

- [ ] #1 `nexus-core` 有 `id` 模块，含 §2.8.1 表中的 **18 个** ID 类型，每个是**独立类型**（不是 `type UserId = i64` 的别名 —— 那样传错类型不会报错，违背 §2.8 的设计意图）
- [ ] #2 每个 ID 类型实现 `Serialize`，输出**裸值**：`SessionId` → UUID 字符串，`UserId` → 数字，`ToolCallId` → 字符串
- [ ] #3 `nexus-core` 有 `cancel` 模块，含 `RequestScope` 与 `DetachedScope`
- [ ] #4 `DetachedScope` 上**不存在** `cancel()` / `token()` / `subscribe()` 任何取消入口（`CancellationToken` 字段私有，不 `pub`）
- [ ] #5 `DetachedScope` 只能通过 `RequestScope::detach()` 构造 —— 无 `new()`、无 `Default`、字段全部私有
- [ ] #6 `RequestScope` 上**没有**落库方法；`DetachedScope::persist` 是唯一落库出口
- [ ] #7 `DetachedScope` 内部用 `CancellationToken::new()`（独立），**不是** `child_token()` —— 父取消后它不受影响
- [ ] #8 `nexus-core` 的 `Cargo.toml` 增加 `tokio-util`（含 `rt` feature）与 `tokio`（含 `sync` feature），版本走 `workspace.dependencies`
- [ ] #9 **负向测试**：`tests/compile_fail/` 下有两个用例，分别断言「`DetachedScope` 上调 `cancel()` 编译失败」与「`DetachedScope` 不能当 `RequestScope` 用编译失败」，`.stderr` 快照入库
- [ ] #10 **行为测试**：父 scope 取消后，`RequestScope` 的子 scope 确实被取消；`detach()` 出来的 `DetachedScope` 不受父取消影响
- [ ] #11 `nexus-core` 仍然**零内部依赖**（`crates/nexus-core/tests/architecture.rs` 的既有断言继续通过）
- [ ] #12 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #13 CI 六道门禁全绿（含 `scripts/check-naming.sh`）

## 备注

- ID 类型数量与底层类型必须由**契约测试**固化（对照 §2.8.1 表逐条断言），不能只靠「写的时候看着对」。
- `nexus-core` 加 `tokio-util` 依赖后，需确认 `scripts/check-deps.sh` 的「零内部依赖」规则仍然成立 —— `tokio-util` 是外部 crate，不算内部依赖。
