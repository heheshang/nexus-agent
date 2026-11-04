---
id: NEX-13
title: "feat(repo): 落地 outbox 与幂等键的事务一致性"
status: To Do
assignee: []
labels: [repo, postgres, outbox, p2]
priority: high
dependencies:
  - NEX-11
  - NEX-12
references:
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §4.1.1"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §4.1.2"
  - "docs/operations/p2-plan.md"
---

## Description

§4.1.1 的**四种一致性机制**中的两种 —— 它们都落在数据层：

| 机制 | 语义 | 参考实现位置 |
|---|---|---|
| **outbox**（至少一次） | 事件与状态在**同一事务**写入 `execution_outbox` | `internal/execution/outbox.go:30` |
| **幂等键** | `request_key`（≤64 字符）+ `ON CONFLICT (owner_id, request_key) DO NOTHING`；重复提交返回既有 run | `internal/execution/store.go:49` |

另两种（租约 + fencing、检查点版本绑定）中，租约已在 NEX-12 落地。

### outbox 的关键细节（§4.1.2）

Go 版 `appendEvent`（`internal/execution/store.go:421`）在**同一个事务**里做三件事：

1. `UPDATE workflow_runs SET next_seq=next_seq+1, state_version=state_version+1
    WHERE id=? RETURNING next_seq` —— **序号分配也是原子的**
2. 写 `execution_events`
3. 写 `execution_outbox`

`PublishOutbox`（`outbox.go:30`）用 `FOR UPDATE SKIP LOCKED` 批量领取，
每条单独 2s 超时投递，投递成功后在**同一事务**里标记 `published_at`。

投递语义注释原文（`outbox.go:28-29`）：

> Delivery is at least once: a crash after the peer accepts but before the PG commit
> can redeliver the same (run_id,seq). Consumers must deduplicate by it.

**「至少一次」是语义保证，不是 bug** —— 消费方**必须**按 `(run_id, seq)` 去重。
这条要写进类型/接口的文档注释里，让消费方不可能忽略。

### 幂等键

`request_key` ≤ 64 字符 + `ON CONFLICT (owner_id, request_key) DO NOTHING`。
重复提交**返回既有 run**（而不是报错）—— 这是幂等的定义。

### `seq` 与 `nexus-core` 的事件序号

NEX-5 的 `EventSink` 保证了**进程内**的序号严格单调。
但持久化场景下，序号必须由**数据库**分配（`next_seq = next_seq + 1 RETURNING`）——
因为可能有多个 writer 进程。

这是两个不同层次的不变量，**不要混淆**：
- `EventSink` 的 `seq` = 单次运行内、进程内的流序号
- `execution_events.next_seq` = 持久化的、跨进程的流序号

## Acceptance Criteria

- [ ] #1 `nexus-repo` 提供 outbox 写入方法：状态更新 + `execution_events` + `execution_outbox` 在**同一事务**
- [ ] #2 序号由**数据库**分配（`next_seq = next_seq + 1 ... RETURNING next_seq`），不靠应用层自增
- [ ] #3 outbox 领取用 `FOR UPDATE SKIP LOCKED`，批量领取不互相阻塞
- [ ] #4 投递成功后 `published_at` 与业务更新在**同一事务**里标记
- [ ] #5 「至少一次」的语义写进**公开接口的文档注释** —— 消费方必须按 `(run_id, seq)` 去重
- [ ] #6 幂等键：`request_key` ≤ 64 字符（**有校验**，超长报错）
- [ ] #7 幂等键唯一约束 `(owner_id, request_key)` 已在 NEX-9 的迁移里落地
- [ ] #8 重复提交**返回既有 run**，不报错 —— 有集成测试断言
- [ ] #9 序号分配与事件写入的原子性有集成测试断言（事务回滚后 seq **不前进**）
- [ ] #10 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #11 CI 六道门禁全绿

## 备注

- AC #9 的测试形状：在一个事务里写事件、然后**故意回滚**，再查 `next_seq` ——
  断言它**没有**前进。这验证了「序号分配与写入同事务」。
- AC #6 的长度校验属于**应用层**校验 —— 数据库的 `VARCHAR(64)` 也会拒绝，
  但应用层要给出**清晰的业务错误**（`ErrorCode::InvalidParam`），而不是把 PG 的错误往上冒。
- `run_id` 用 NEX-4 的 `RunId` 类型。`owner_id` —— 查一下参考实现用的是
  `user_id` 还是 `owner_id`；按 §4.1.2 的原文是 `owner_id`。
- `execution_events` / `execution_outbox` 是 `nexus-execution` 的表 ——
  **但**它们是**执行状态**的一部分。本任务在 `nexus-repo` 里实现它们的读写，
  这不违反 §4.2 差异 1（那条约束针对的是 `workflow_runs`）。
  如果实现中发现边界模糊，在任务里记录并**停下**，不要自行决定。
- 投递（把 outbox 发给消费方）是 P6 的事 —— 本任务只负责**写**与**领取**，
  不负责实际投递。投递的「2 秒超时」属于消费侧策略。

## 实现过程中的发现

<!-- 实现时填写 -->
