---
id: NEX-28
title: "feat(agent): 落地事件 sink 与背压"
status: To Do
assignee: []
labels: [agent, events, backpressure]
priority: medium
dependencies:
  - NEX-26
references:
  - "docs/specs/p4/spec.md §3.5、§3.7、§6.5"
  - "docs/specs/p4/tasks.md T-003"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.2"
---

## Description

**决策 D7 的落地**：P4 定义自己的**异步**事件 sink，不复用 `nexus-core::EventSink`。

### 为什么不复用 `nexus-core::EventSink`

两者形状不同：

| | `nexus-core::EventSink` | P4 需要的 |
|---|---|---|
| 写入方式 | `emit(&mut self)` —— **同步** | **异步**发送 |
| 写者 | **单写者**（`&mut self` 编译期串行化） | **多个并发生产者**（多工具并发、模型调用与事件发送交错） |
| 背压 | 无 | **必须有**（消费者太慢 → 超时 → 主动取消） |
| 序号 | 调用方同步分配 | 需要**单点分配**（与写入原子） |

强行统一会让其中一个变形 —— `nexus-core` 的是 P1 定稿的**单写者**保证，
改成异步多写者会**破坏**架构文档 §2.2.3 的核心不变量。

**这是「并存」不是「替换」。**

### 序号分配：单点分配，不用锁

参考架构 §2.2 给了三个方案，推荐**方案 C**：

```
ingress mpsc（生产者只管 send）→ 唯一的序号分配 task → output mpsc（消费者读的流）
```

这样 `Seq` 分配与写入**天然原子**，且**没有锁**。

**方案 A**（`AtomicU64` + `fetch_add`）**不够** —— 它保证序号唯一，
但**不保证与通道写入的原子性**：事件 A 可能拿到 seq=1 却比 seq=2 更晚到达通道。
如果前端依赖严格有序，方案 A 不够。

### 双通道

架构文档 §3.2 定稿：只有 `delta` / `reasoning` 是**可丢**的，其余全部**必达**。

「可丢」满了返回 `Discarded` **不阻塞**；「必达」满了**挂起等待**。

背压超时 → **主动取消**整个运行（不是无限阻塞）—— 参考架构 §2.2 不变量 ④。

## 范围边界

### 做

- P4 的异步 sink（单点分配序号）
- 双通道（可丢 / 必达）
- 背压与主动取消
- 事件负载复用 `nexus_core::EventPayload`

### 不做

| 项 | 理由 |
|---|---|
| SSE 桥接 / 半帧处理 | 属 P5（`nexus-api`） |
| 新增事件类型 / 事件名 | 12 个事件名**已定稿**（架构文档 §2.2.4.1），P4 **不新增** |
| 修改 `nexus-core::EventSink` | 它是 P1 定稿的单写者保证，P4 **不动**它 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-agent/src/events.rs` 定义 sink，采用**单点分配**序号（ingress 通道 → 唯一分配 task → output 通道）
- [ ] #2 事件序号**严格单调、无重复、无空洞**，且与写入顺序一致
- [ ] #3 序号分配与写入**天然原子** —— 不使用 `Mutex`
- [ ] #4 **双通道**：`delta` / `reasoning` 走**可丢**通道，其余全部走**必达**通道
- [ ] #5 填满可丢通道后，**终态事件仍然送达**
- [ ] #6 背压超时**主动取消**整个运行，而不是无限阻塞
- [ ] #7 事件负载复用 `nexus_core::EventPayload` —— 事件名与负载形状**不新增**（12 个事件名不变）
- [ ] #8 `nexus-agent` **不**复用 `nexus_core::EventSink`（两者形状不同：同步单写者 vs 并发生产者 + 背压）
- [ ] #9 并发工具调用下序号仍严格单调 —— 有测试
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对 —— 
> 本仓库的 `pipeline-cli` 尚未构建（`dist/` 缺失），按 `ai-sdlc-dor-gate.mdc` §5 执行。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

10 条 AC 全部可二元判定 —— 每条都指明「序号是否单调」「通道是否阻塞」或「事件名是否新增」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

「用方案 A（`fetch_add`）还是方案 C（单点分配）」已**显式落定**（AC #1 / #3），
依据是参考架构 §2.2 对方案 A 的明确批评（「不保证与通道写入的原子性」）。

### #3 引用可解析

- `docs/specs/p4/spec.md §3.5、§3.7、§6.5` —— 存在
- `docs/specs/p4/tasks.md T-003` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.2` —— 存在

### #4 范围有界

单 crate 改动（`nexus-agent`）。**不触及** 42 个业务码、12 个事件名（**复用**不新增）、
18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-agent/src/events.rs` | **新增** —— sink 实现 |
| `crates/nexus-agent/tests/events.rs` | **新增** —— 行为层测试 |

**不新增依赖**（`tokio` / `futures` **已**在 `nexus-agent` 的依赖里）。

### #6 完成态可描述

调用方拿到一个 `Stream`，其中事件的 `seq` 严格单调；
可丢通道满时增量被丢弃但终态必达；消费太慢时整个运行被主动取消。

### #7 无隐形依赖

- NEX-26 已登记在 `backlog/tasks/`（本任务复用 `EventPayload` 的类型定义，但那是 `nexus-core` 的）
- `tokio` / `futures` **已**在 `nexus-agent` 的 `Cargo.toml` 里
- `nexus-core` **已**在依赖里

### 结论

**pass** —— 七项全过，**但需 NEX-26 完成后开工**。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：P4 自定义异步 sink，**不复用** `nexus_core::EventSink`。
- **依据**：两者形状不同（同步单写者 vs 并发生产者 + 背压）。
  强行统一会让 `nexus-core` 的单写者保证变形 —— 那是 P1 定稿的契约。
  详见 `docs/specs/p4/spec.md` §6.5。
- **可逆性**：中 —— sink 是内部实现细节，但事件**负载类型**是共享的（`nexus_core::EventPayload`）。
- **影响面**：`crates/nexus-agent/src/events.rs`。

### 类别：decide-and-proceed

- **决策**：序号用**单点分配 task**（方案 C），不用 `AtomicU64` + `fetch_add`（方案 A）。
- **依据**：参考架构 §2.2 —— 方案 A「保证序号唯一但**不保证与通道写入的原子性**」，
  事件 A 可能拿到 seq=1 却比 seq=2 更晚到达。
- **可逆性**：中 —— 换实现会影响「序号与写入顺序一致」这条不变量的测试写法。
- **影响面**：`crates/nexus-agent/src/events.rs`。

---

## 实现过程中的发现

（待补）
