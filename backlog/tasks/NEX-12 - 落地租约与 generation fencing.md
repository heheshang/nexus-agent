---
id: NEX-12
title: "feat(repo): 落地租约与 generation fencing"
status: To Do
assignee: []
labels: [repo, postgres, fencing, p2]
priority: high
dependencies:
  - NEX-11
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.4"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.4.2"
  - "examples/demo-lease-fencing/src/main.rs"
  - "docs/operations/p2-plan.md"
---

## Description

**执行模型的核心不变量**：租约 + fencing。

参考实现已**在真实 PostgreSQL 17 上验证过**（架构文档 §2.5.4 的验证记录），
`examples/demo-lease-fencing` 是可运行的回归基准。本任务把它搬进 `nexus-repo`。

### 机制

**领取**（`FOR UPDATE SKIP LOCKED`）：
```sql
UPDATE workflow_runs SET lease_owner=$w, generation = generation + 1,
                         lease_until = now() + 30s
WHERE ... FOR UPDATE SKIP LOCKED
```
多个 worker 并发领取时**互不阻塞**，各自拿到**不同**的 run。

**fencing**（`WHERE generation = $my_gen`）：
```sql
WHERE id = $run AND generation = $my_gen AND lease_owner = $owner
  AND status='running'
  AND lease_until > clock_timestamp() AND deadline_at > clock_timestamp()
```
worker 崩溃后被别人重新领取 → `generation` 递增 → 旧 worker 的迟到写**影响 0 行**。

### 关键设计：`generation` 封装在租约对象里

§2.5.4 明确：**`generation` 不应该由调用方手动传** —— 应该由**租约对象**持有。

```rust
/// 持有租约的运行句柄。generation 封装在里面，
/// 调用方无法写出「忘记带 generation」的 UPDATE。
pub struct RunLease {
    run_id: RunId,
    generation: i64,
    owner: String,
}
```

`RunLease` 提供 `lease.execute(...)` 这样的方法，内部自动带上 generation。
调用方**拿不到裸的 generation 数字**。

这与 P1 的「`EventSink` 不可 Clone，序号与写入顺序一致」是同一种思路 ——
**把不变量塞进类型，而不是靠约定**。

### fencing 条件封装成结构体

§3.4.2：`sqlx` 没有 GORM 的链式 `Where` 复用机制。
**建议**把 fencing 条件封装成一个结构体：

```rust
#[derive(Clone, Debug)]
pub struct Fence {
    pub id: i64,
    pub generation: i64,
    pub lease_owner: String,
}
```

## Acceptance Criteria

- [ ] #1 `nexus-repo` 有 `Fence` 结构体，承载 `id` / `generation` / `lease_owner`
- [ ] #2 `Fence` 提供构造 SQL `WHERE` 子句的方法 —— **一处**定义，全局复用
- [ ] #3 `RunLease` **持有** `generation`，**不提供**返回裸 generation 的公开方法
- [ ] #4 写入方法签名要求 `&RunLease`（或 `&Fence`）—— 调用方无法绕过 fencing
- [ ] #5 领取用 `FOR UPDATE SKIP LOCKED`，**3 个并发连接各领到不同的 run**（集成测试）
- [ ] #6 fencing 生效：`generation` 不匹配的写入**影响 0 行**（集成测试）
- [ ] #7 当前持有者的写入**影响 1 行**（集成测试）
- [ ] #8 `RunLease` 的 `RunId` 用 NEX-4 的 `RunId` 类型，不用裸 `i64`
- [ ] #9 租约过期判定用 `clock_timestamp()`（不是 `now()`）—— `now()` 在事务内是**固定值**，
      用它判断租约是否过期会得到错误结果
- [ ] #10 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #11 CI 六道门禁全绿

## 备注

- **AC #9 是个容易踩的坑** —— `now()` 返回**事务开始时间**，
  `clock_timestamp()` 返回**实际当前时间**。在长事务里用 `now()` 判断租约过期会误判。
  参考实现的 fencing 条件里用的就是 `clock_timestamp()`。
- `examples/demo-lease-fencing` 是**回归基准**，保留不删。
  本任务落地后，demo 与正式实现是**两份独立的验证** —— 与 P1 的结论一致。
- 租约 TTL 默认 30 秒（参考实现的值）—— 但**不要硬编码**，
  它应是配置项（`nexus-config` 已有 `WORKER__LEASE_TTL_SECS`）。
- `nexus-repo` **不得**引用 `workflow_runs` 的类型（§4.2 差异 1）——
  那张表由 `nexus-execution` 独占。
  **本任务用的是 `execution_runs`**（NEX-9 建的独立表）。
  如果发现实现里出现了 `workflow_runs` 的行类型，说明偏离了架构决策。
- 「围栏写与业务写同事务」是必须抄的不变量 ——
  围栏检查与业务更新必须在**同一个事务**里，否则中间有窗口。

## 实现过程中的发现

<!-- 实现时填写 -->
