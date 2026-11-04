---
id: NEX-9
title: "feat(infra): 落地迁移基线 —— 核心表 schema"
status: To Do
assignee: []
labels: [infra, postgres, migrations, p2]
priority: high
dependencies:
  - NEX-8
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.9"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.9"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.9.4"
  - "docs/operations/p2-plan.md"
---

## Description

P2 判据之一：**「迁移可应用」**。

参考实现有 **13 组迁移（001–013，含 down）**，建 **42 张表 + 1 个视图**（本机 `nexus-test-pg` 实测：
`BASE TABLE 42` + `VIEW 1` = 43）。

本任务把参考实现的 schema **搬进 `nexus-infra` 的 `migrations/` 目录**，
用 `sqlx::migrate!` 嵌入二进制。

### 架构文档规定的两处刻意偏离

**偏离 1：执行表分开建。**
参考实现迁移 004 给 `workflow_runs` **加 11 列**（历史兼容）。
Rust 版**新建 `execution_runs`**（引擎侧），`workflow_runs` 只留产品侧字段。
理由（§3.9.4）：避免历史数据迁移的复杂度。

**偏离 2：按 Phase 渐进建表。**
参考实现一次建完 42 表。Rust 版**只建 Phase 0 核心表**（§3.9.4 点名）：

```
users / agents / tools / sessions / messages / workflow_runs
+ execution_*（execution_runs / execution_checkpoints / execution_operations
              / execution_attempts / execution_approvals / execution_events
              / execution_outbox）
```

其余表（`documents` / `memories` / `a2a_*` / `mcp_servers` / `eval_*` / `usage_logs` 等）
留给后续 Phase —— **避免后期反复改表**。

### 三条必须下沉到数据库的约束

§3.9.1 的关键索引模式：

1. **部分索引** —— 只索引「就绪」的行：
   ```sql
   CREATE INDEX idx_execution_ready ON workflow_runs(available_at, id)
     WHERE definition IS NOT NULL AND status IN ('queued','running','retry_wait');
   ```
2. **幂等键唯一** —— `ON CONFLICT (owner_id, request_key) DO NOTHING`
3. **CHECK 约束** —— 约束下沉到数据库（如 `tools.replay_policy`）

### 迁移工具的选择

**`sqlx::migrate!`**（§3.9.3 建议）—— 迁移文件打包进二进制，部署时不需额外拷目录。

**down 脚本：sqlx 0.9 原生支持** —— 这与参考架构文档 §3.9.3 的说法**不同**，
是本次实施时**实测源码**确认的：

`sqlx-core-0.9.0/src/migrate/source.rs` 按文件名解析迁移类型：

```
<VERSION>_<DESCRIPTION>.sql        → Simple
<VERSION>_<DESCRIPTION>.up.sql    → ReversibleUp
<VERSION>_<DESCRIPTION>.down.sql  → ReversibleDown
```

且 `Migrator::undo(migrator, target)` 存在（`migrator.rs:314`）——
即**回滚是 API 能力，不只是 CLI 层的**。

参考架构文档 §3.9.3 写的是「`sqlx::migrate!` 只支持 up 迁移（`sqlx migrate revert`
是 CLI 级别的）」—— 那描述的是**更早的 sqlx 版本**。0.9 已支持可逆迁移。

**结论**：参考实现的 `.up.sql` / `.down.sql` 命名**可以原样沿用**，
不需要另建目录、不需要改命名约定。这比「down 脚本作为独立文件保留」更好 ——
因为 up/down 配对在**同一版本号**下，回滚时 `Migrator::undo` 能直接找到配对。

## Acceptance Criteria

- [ ] #1 `nexus-infra` 有 `migrations/` 目录，用 `sqlx::migrate!` 嵌入
- [ ] #2 迁移覆盖 **Phase 0 核心表**（§3.9.4 点名的 7 类 + `execution_*` 7 张）
- [ ] #3 `execution_runs` 是**独立新建**的表 —— 不是给 `workflow_runs` 加列
- [ ] #4 部分索引（只索引就绪行）已落地 —— 有测试断言索引存在且带 `WHERE` 子句
- [ ] #5 幂等键唯一约束已落地 —— `(owner_id, request_key)` 唯一
- [ ] #6 `replay_policy` 的 CHECK 约束已落地（`readonly` / `idempotent` / `manual`）
- [ ] #7 迁移在**空库**上可完整应用（连跑两次不报错 —— 幂等）
- [ ] #8 迁移文件名用 sqlx 0.9 的**可逆格式** `<版本号>_<描述>.up.sql` / `.down.sql`
      —— 参考实现的命名可原样沿用；版本号前缀是**整数**（`1` / `2`，非 `001`）
- [ ] #9 迁移文件的命名与内容有**参考实现对应关系**的记录（哪个文件对应参考实现的哪一组迁移）
- [ ] #10 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #11 CI 六道门禁全绿

## 备注

- **迁移文件是持久化契约** —— 改列名/类型/约束都是破坏性变更。
- 本机已有可用的数据库：`nexus-test-pg`（PostgreSQL 17，参考实现的 43 个对象已应用）。
  测试应连**独立的测试库**（如 `nexus_test`），不要污染 `nexus_design`。
- 测试库需要**每次重建** —— 因为测试会插入数据。用独立 schema 或独立库名 + 容器重建。
- 参考实现的迁移文件**不在本仓库** —— 需要从参考实现仓库取，或按 §3.9.1 的模式重写。
  无论哪种，都必须在任务里记录**对应关系**（AC #9），
  否则后续无法回答「这个约束是从哪抄来的、为什么抄」。
- **迁移文件名的三个硬约束**（实测 `sqlx-core-0.9.0/src/migrate/source.rs`）：
  1. 版本号前缀必须是**整数** —— `parts[0].parse::<i64>()`，
     `001` 能解析成 `1`，但 `1` 与 `001` 会**撞版本号**，所以只用一种写法。
  2. 必须有**下划线**分隔 —— `splitn(2, '_')`，没有 `_` 的文件被**静默忽略**。
  3. 必须是 `.sql` 结尾 —— 否则同样被静默忽略。
  > **静默忽略**是个陷阱：文件名写错不会报错，只是那个迁移**根本没跑**。
  > 必须有测试断言「预期的表都存在」—— 那才能发现迁移被跳过了。

## 实现过程中的发现

<!-- 实现时填写 -->
