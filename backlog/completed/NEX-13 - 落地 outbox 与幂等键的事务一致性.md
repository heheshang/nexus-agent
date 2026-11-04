---
id: NEX-13
title: "feat(repo): 落地 outbox 与幂等键的事务一致性"
status: Done
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

- [x] #1 `nexus-repo` 提供 outbox 写入方法：状态更新 + `execution_events` + `execution_outbox` 在**同一事务**
- [x] #2 序号由**数据库**分配（`next_seq = next_seq + 1 ... RETURNING next_seq`），不靠应用层自增
- [x] #3 outbox 领取用 `FOR UPDATE SKIP LOCKED`，批量领取不互相阻塞
- [x] #4 投递成功后 `published_at` 与业务更新在**同一事务**里标记
- [x] #5 「至少一次」的语义写进**公开接口的文档注释** —— 消费方必须按 `(run_id, seq)` 去重
- [x] #6 幂等键：`request_key` ≤ 64 字符（**有校验**，超长报错）
- [x] #7 幂等键唯一约束 `(owner_id, request_key)` 已在 NEX-9 的迁移里落地
- [x] #8 重复提交**返回既有 run**，不报错 —— 有集成测试断言
- [x] #9 序号分配与事件写入的原子性有集成测试断言（事务回滚后 seq **不前进**）
- [x] #10 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #11 CI 六道门禁全绿

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

### 1. 规范化 JSON 只能放在 `nexus-core` —— 依赖方向唯一决定

NEX-12 留下的待办：「定义哈希是**占位实现**，NEX-13 落地幂等键时必须换成真正的规范化哈希」。

实现规范化时先撞上一个**归属问题**：它该放哪个 crate？

- `nexus-repo` —— `nexus-workflow` 需要它，但能力层**不得**依赖数据层（§4.1.2）
- `nexus-workflow` —— `nexus-repo` 需要它，但 `repo → workflow` 会成环
  （`workflow → execution → repo → workflow`）
- **`nexus-core`** —— 依赖图的**根**（§4.4 规则 2），**所有** crate 都能依赖它

所以位置是**被依赖方向唯一决定**的，没有选择余地。已落地为
`crates/nexus-core/src/canonical.rs`，并加了**结构层架构测试**
（`canonicalization_lives_in_nexus_core_only`）把这件事钉死 ——
它同时证明「`nexus-core` **有**实现」与「另两个 crate **没有**自己的实现」。

### 2. 规范化的「最短往返」不是 `f64` 的 Display —— 实测踩坑

§3.5 规则 3 要求「数字用最短往返表示（`1.0` → `1`，`1e2` → `100`）」。

两处实测发现：

- **`serde_json` 认为 `1.0` 与 `1` 是不同的值**（`"1.0" == "1"` 为 `false`）。
  所以规范化必须**自己**做「浮点整数化」，不能只看存的是什么类型。
- **Rust 的 `f64` `Display` 从不使用指数形式** —— `1.7976931348623157e308`
  被写成 309 位十进制数字，那个字符串**解析不回来**
  （`serde_json` 报 `number out of range`）。

  最后用 `serde_json::Number::from_f64` 借用 serde_json 内部的 ryu ——
  不重复造轮子，也不承担「自己写的不对」的风险。

### 3. 定义哈希算法变更 = 破坏性变更（已记录）

`leases.rs` 的 `definition_hash_of` 从 NEX-12 的占位实现
（`DefaultHasher` + `definition.to_string()`，**键序敏感**）换成了
规范化 SHA-256。

**已存在的 run 的 `definition_hash` 是旧算法的值** —— 用新算法重算会得到
不同的哈希 → 续跑时会被判为「定义被改过」。

参考实现用 SHA-256（`internal/service/workflow_snapshot.go:137`），
所以这不是「Rust 版的偏离」，而是「回到参考实现的算法」。
生产库需要**重新生成** `definition_hash` 或清空历史 run。

### 4. `claim` / `claim_batch` 是**全局队列** —— 跨测试二进制的污染

实测踩到的坑（不是理论）：

`claim` 领取队列里**任意**一个可运行的 run —— **没有**「按 id 领取」的入口。
`lease_fencing.rs` 与 `outbox_idempotency.rs` 是**两个独立的测试二进制**，
各自的进程内 `Mutex` **互相看不见**。

结果：`outbox_idempotency.rs` 留下的 `queued` run 会被
`lease_fencing.rs` 的 `claim` 领走 → `assert_eq!(lease.run_id(), run_id)` 失败
→ 整文件 8 个测试变红。**级联失败**。

**关键认识**：锁与删除**解决的是两个不同的问题**，缺一不可：

| 问题 | 手段 | 为什么锁**不够** |
|---|---|---|
| 两个测试**同时**领取 | **跨进程**的 advisory lock | — |
| 一个测试领走**另一个**的 run | **删除**自己造的数据 | 锁只保证「不同时领」，**不**保证「领到的是自己的」 |

所以用了**三**条对策：

| 对策 | 作用 |
|---|---|
| `common::QueueGuard` —— **PG advisory lock**（`pg_try_advisory_lock`） | **跨进程**串行化全局队列操作 |
| `create_leased_run` —— **直接插入**已持租约的 run，不碰队列 | 造数据不再参与队列竞争 |
| `TestRuns` / `TRACKED` —— 登记创建的 run，测试末尾**删除** | 不留 `queued` 行在全局队列里 |
| `cleanup_by_prefix` —— 按**请求键前缀**在测试**开始时**也清一次 | 测试 panic 时末尾的 cleanup 不执行，遗留会污染**下一次**运行 |

前缀里含测试名 → 并行执行时一个测试的清理**不会**删另一个测试的数据。

#### 为什么锁必须是**跨进程**的

`cargo test` 会**并行**运行多个**测试二进制** ——
`lease_fencing.rs` 与 `outbox_idempotency.rs` 是两个**独立进程**。

进程内的 `tokio::Mutex` 对**另一个进程**完全无效。实测后果：
`lease_fencing.rs` 的 13 个测试里多个变红（被另一个进程领走了自己的 run）。

跨进程只能用 **PostgreSQL 的 advisory lock** —— 由**数据库**仲裁。

用 `pg_try_advisory_lock`（**非阻塞**）+ **超时跳过**，而不是 `pg_advisory_lock`（阻塞）：
拿不到锁就**跳过**这个测试，而不是挂死整个测试运行。

`QueueGuard` 用**独立连接**（不是从池里借的）——
advisory lock 绑定在**会话**上，从池里借的连接归还后会话还在，锁不会释放。
独立连接在 `Drop` 时**关闭** → PG 自动释放会话级锁（也覆盖 panic 的 unwind 路径）。

已加**结构层断言** `the_queue_lock_is_cross_process_not_process_local` ——
它扫描源码断言「进程内的 `Mutex` 不得出现」，防止有人把它改回去。

### 5. `request_key` 的 `VARCHAR(64)` 是**真**约束 —— 测试也撞上了

最初给测试数据的请求键加了长前缀（`outbox_<测试名>_`），直接撞上
`value too long for type character varying(64)`。

这正好**验证了 AC #6 的约束是真的** —— 应用层 [`RequestKey::new`] 校验
与数据库约束**一致**。测试改用短前缀（`ob0_`…`ob8_`）。

### 6. `FOR UPDATE SKIP LOCKED` 的测试断言要按「**本测试的消息**」数

`claim_batch` 会领走**任意**一批待投递消息，包括**别的**测试的。
所以断言不能是「恰好领到 N 条」，而是「**本测试的消息都在里面**」。

### 7. 变异测试 —— 验证测试真的在测东西

沿用 NEX-12 的做法（变异测试的教训：「测试通过」不等于「测试在测你以为的东西」）：

| 变异 | 被哪个测试抓住 |
|---|---|
| 去掉围栏条件（僵尸写入能通过） | `stale_lease_cannot_allocate_a_sequence` |
| 序号分配挪到**独立事务**（回滚不回退） | `rolled_back_transaction_does_not_advance_seq` |
| outbox 表改名成非引擎侧 | `outbox_tables_are_engine_side_only`（架构测试） |

### 8. 边界澄清：`execution_outbox` 的 `run_id` / `seq` 可空

迁移 5 里 `execution_outbox.run_id` 与 `seq` **都可空**（不像 `execution_events`
的复合主键那样必填）。

所以 [`OutboxMessage::dedup_tuple`] 在两者任一为空时返回 `None`。
消费方遇到 `None` **不能跳过**（那会丢消息）—— 应退回用 `dedup_key` 或 `id` 去重。
这条写进了公开接口的文档注释（AC #5）。

### 9. 未做的事（明确划界）

- **不实现投递** —— 那是 P6 的事。本任务只负责**写**与**领取**。
- **不改 `EventSink`** —— NEX-5 的进程内序号与本任务的跨进程序号是**两个层次**的
  不变量，不要混淆。
- **不改 `workflow_runs`** —— 那由 `nexus-execution` 独占（§4.2 差异 1）。

