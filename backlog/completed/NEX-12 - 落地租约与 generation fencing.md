---
id: NEX-12
title: "feat(repo): 落地租约与 generation fencing"
status: Completed
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

- [x] #1 `nexus-repo` 有 `Fence` 结构体，承载 `id` / `generation` / `lease_owner`
- [x] #2 `Fence` 提供构造 SQL `WHERE` 子句的方法 —— **一处**定义，全局复用
- [x] #3 `RunLease` **持有** `generation`，**不提供**返回裸 generation 的公开方法
- [x] #4 写入方法签名要求 `&RunLease`（或 `&Fence`）—— 调用方无法绕过 fencing
- [x] #5 领取用 `FOR UPDATE SKIP LOCKED`，**3 个并发连接各领到不同的 run**（集成测试）
- [x] #6 fencing 生效：`generation` 不匹配的写入**影响 0 行**（集成测试）
- [x] #7 当前持有者的写入**影响 1 行**（集成测试）
- [x] #8 `RunLease` 的 `RunId` 用 NEX-4 的 `RunId` 类型，不用裸 `i64`
- [x] #9 租约过期判定用 `clock_timestamp()`（不是 `now()`）—— `now()` 在事务内是**固定值**，
      用它判断租约是否过期会得到错误结果
- [x] #10 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #11 CI 六道门禁全绿

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

### 落点

| 文件 | 内容 |
|---|---|
| `crates/nexus-repo/src/lease.rs` | **类型**：`Fence` / `RunLease` / `LeaseOutcome` / `LeaseLost` |
| `crates/nexus-repo/src/leases.rs` | **仓储**：`ExecutionLeaseRepository`（领取 + 围栏写入） |
| `crates/nexus-repo/tests/lease_fencing.rs` | **行为层**：13 个集成测试（连真实 PG） |
| `crates/nexus-repo/tests/compile_fail.rs` | **类型层**：4 个 `trybuild` 负向测试 |
| `crates/nexus-core/tests/architecture.rs` | **结构层**：`nexus-repo` 不得引用 `workflow_runs` |

### 发现 1：`push_bind` 的占位符编号是**自动**的

最初把围栏条件写成**硬编码 `$1` / `$2` / `$3`** 的常量片段，
再用 `push_bind` 绑定三个值 —— 两者**混用**，生成出：

```text
WHERE $1$2$3 AND id = $1 AND generation = $2 AND lease_owner = $3 AND ...
```

**语法错误**（`syntax error at or near "$3"`）。

`push_bind` 的编号是从 `$1` 起算的**自动**编号
（`sqlx-postgres-0.9.0/src/arguments.rs:150` —— `write!(writer, "${}", self.buffer.count)`）。
而围栏写入的语句形态不止一种（`SET status = ?`、`SET next_seq = next_seq + ?`），
参数个数与顺序各不相同 —— **手写绝对编号必然漂移**。

**修正**：模板用 `?` 标记绑定点，由 sqlx 编号。
`Fence::CONDITIONS_TEMPLATE` 是**唯一**的真相，
`push_conditions` 按 `?` 切分它 —— 定义与实现**不可能漂移**。

### 发现 2：「断言 SQL 文本的副本」**测不出**实现是否还对

**变异测试**（把 `generation` 条件从 `push_conditions` 里删掉）→
**12 个测试全部依然通过**。

原因是围栏有**六个**条件：僵尸写入被拒，可能是**任意一个**拦下的。
worker-b 重新领取后 `lease_owner` 也变了、`lease_until` 也刷新了 ——
即使 `generation` 条件**整个消失**，僵尸写入**照样**被 `lease_owner` 拦下。

**修正**（`forge_lease_back_to`）：测试把 `lease_owner` / `lease_until`
**伪造回**旧持有者的样子，只保留 worker-b 的新 `generation`。
这样**唯一**的差异是 `generation` —— 删掉它，测试**必须**变红。

**三种变异均已确认变红**：

| 变异 | 变红的测试 |
|---|---|
| 删掉 `generation` 条件 | 单元 3 个 + 集成 5 个 |
| `clock_timestamp()` → `now()` | 单元 1 个 + 集成 1 个（长事务那个） |
| 删掉 `lease_owner` 条件 | 单元 2 个 + 集成 5 个 |

**教训**：「测试通过」不等于「测试在测你以为的东西」。
断言**真实生成的 SQL**（`Fence::where_sql()`）而不是断言副本 —— 这是关键。

### 发现 3：`now()` 的坑只在**长事务**里显现

`now()` 是**事务开始时间**，在事务内固定；`clock_timestamp()` 是实际当前时间。

短事务里两者几乎一样 —— 所以最初写的「过期租约被拒」测试**抓不住**这个变异
（`now()` 版本的集成测试**全绿**）。

**修正**：新增 `expiry_is_judged_inside_a_long_transaction` ——
开启事务 → `sleep` 到租约真的过期 → 在**同一个事务**里做围栏写入。
用 `now()` 的实现会**放行**（错误），测试变红。

### 发现 4：`claim` 是**全局队列**语义，测试**必须串行**

`claim` 领取队列里**任意**一个可运行 run（`ORDER BY available_at, id LIMIT 1`），
**没有**「按 `request_key` 领取」的入口 —— 那不是生产语义。

于是并行执行的测试**互相抢 run**：
6 个并行测试 → 5 个失败（「应能领到一个 run」拿到 `None`，
或「领到的不是我造的那个」）。

**修正**：领取类测试用进程内互斥锁 `CLAIM_LOCK` **串行化**。
`claim_lock_is_actually_used` 用源码扫描断言锁**确实**被用上
（防止「有人删掉了 `CLAIM_LOCK.lock()`，测试又恰好通过」的退化）。

> 这与 NEX-11 的 `status` 隔离问题是**同一类**：
> **隔离维度必须落在「被操作的那个键」上**。
> 队列操作的键是**全局队列**，那就只能串行。

### 发现 5：`execution_runs` 没有 `updated_at` 列

初版想顺手更新 `updated_state_at`，但迁移 4 里**没有这一列**。
「顺手加一列」是**破坏性变更**（持久化契约）—— 只能去掉。

### 范围说明：`definition_hash` 是**占位实现**

`create_queued_run` 需要填 `definition_hash CHAR(64)`，
但真正的哈希要按 §2.5.2 的**规范化 JSON** 规则计算（键序无关、数组序敏感、数字最短往返）——
那属于 `nexus-workflow` 的职责，不在本任务范围。

本实现用 `DefaultHasher` 算一个**稳定**的 64 位十六进制串，
并**明确标注**为占位。NEX-13 落地幂等键时必须换成真正的规范化哈希。

### 未做的部分（明确划界）

| 项 | 原因 |
|---|---|
| **续租**（lease renewal） | 参考实现有，本任务的 AC 没有。`lease_until` 只在领取时设定。 |
| **`RunLease::execute(...)`** | §2.5.4 提到的方法名。本实现用 `write_status_fenced` / `increment_counter_fenced` —— 命名更具体，且**签名要求 `&mut Transaction`** 以支撑「围栏写与业务写同事务」。 |
| **`Fence` 作为写入参数** | AC #4 说「`&RunLease`（或 `&Fence`）」。选了 `&RunLease` —— 它才是「持有租约」的证据。`Fence` 仍可通过 `RunLease::fence()` 降级得到。 |
| **定义哈希** | 见上 —— 占位实现。 |
