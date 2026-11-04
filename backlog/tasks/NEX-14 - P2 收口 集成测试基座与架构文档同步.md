---
id: NEX-14
title: "test(repo): P2 收口 —— 集成测试基座与架构文档同步"
status: To Do
assignee: []
labels: [repo, testing, docs, p2]
priority: medium
dependencies:
  - NEX-8
  - NEX-9
  - NEX-10
  - NEX-11
  - NEX-12
  - NEX-13
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.3"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.4"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.2"
  - "docs/operations/p2-plan.md"
---

## Description

P2 的收口任务：**「迁移可应用、集成测试绿」**两项判据的最终确认，
外加同步架构文档中因 P2 落地而失效的表述。

### §5.3 的架构测试表 —— P2 相关部分

| 测试 | 检验的设计条款 | P2 状态 |
|---|---|---|
| `workflow_runs` 只在 `nexus-execution` 出现 | §4.2 差异 1 | ⬜ **本任务做** —— P2 后实体存在了 |
| `nexus-repo` 只依赖能力层（单向） | §4.1.2 | ⬜ **本任务做** —— P2 后实体存在了 |

这两项在 P1 期间标着「⬜ P2 才有」—— 现在 P2 落地，正是补上的时候。

### §5.4 的不变量表 —— 新增 P2 已验证项

| 不变量 | 测试方式 | 状态 |
|---|---|---|
| 迁移可在空库完整应用 | 集成测试：空库 → migrate → 断言表存在 | 待补 |
| 迁移可重复执行 | 集成测试：连跑两次不报错 | 待补 |
| 并发领取互不阻塞 | 3 个并发连接各领不同 run | 待补 |
| generation fencing 拒绝僵尸写入 | 旧持有者写入影响 0 行 | 待补 |
| 幂等提交返回既有 run | 重复提交断言同一 `run_id` | 待补 |
| outbox 与状态同事务 | 事务回滚后 `next_seq` 不前进 | 待补 |
| 部分索引只覆盖就绪行 | 断言索引定义含 `WHERE` 子句 | 待补 |

### §4.2 需要同步的表格行

架构文档 §4.2 的对照表有两行与 P2 相关：

```
| `nexus-repo` ↔ `nexus-rag` | 单向 `repo → rag` | ... |
| `nexus-core` 子模块 | ... | ... |
```

P2 落地后 `nexus-repo` 真正实现了 `nexus-rag` 的 trait —— 需要把「已单向」更新为
**有实证**的状态（哪个 trait、哪个实现）。

### §6.1 的 P2 行

标注完成判据已达成。

## Acceptance Criteria

- [ ] #1 集成测试基座落地：统一的「有库则跑、无库则跳过」机制，**不**让 CI 因缺库而红
- [ ] #2 架构测试新增：`workflow_runs` 的类型只在 `nexus-execution` 出现（§4.2 差异 1）
- [ ] #3 架构测试新增：`nexus-repo` 只依赖能力层、不被能力层依赖（§4.1.2）
- [ ] #4 集成测试断言：迁移在空库可完整应用，且可重复执行
- [ ] #5 集成测试断言：并发领取互不阻塞（3 连接各领不同 run）
- [ ] #6 集成测试断言：generation fencing 拒绝僵尸写入
- [ ] #7 集成测试断言：幂等提交返回既有 run
- [ ] #8 集成测试断言：outbox 与状态同事务（回滚后 seq 不前进）
- [ ] #9 集成测试断言：部分索引定义含 `WHERE` 子句
- [ ] #10 架构文档 §4.2 对照表中 `nexus-repo` 相关行**已更新**为 P2 落地后的实际状态
- [ ] #11 架构文档 §5.3 架构测试表的两项 P2 待办**已标记为完成**
- [ ] #12 架构文档 §5.4 不变量表**已补** P2 已验证项
- [ ] #13 架构文档 §6.1 的 P2 行**已标记**为完成
- [ ] #14 CI 配置已加 PostgreSQL `services:` —— 集成测试在 CI 里**真的跑**
- [ ] #15 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #16 CI 全部门禁全绿

## 备注

- **AC #14 是本任务最容易漏的一项** —— 本地有数据库不等于 CI 有。
  没有 `services:`，CI 里的集成测试会**全部跳过**，
  「集成测试绿」就是个假象。改 CI 后要**确认 CI 日志里真的跑了**集成测试
  （不是 0 passed）。
- CI 的 PG 版本要与本地一致 —— **PostgreSQL 17**（参考实现的迁移在 17 上干净应用）。
- §5.3 的架构测试是**源码扫描**还是**依赖图断言**？
  `scripts/check-deps.sh` 规则 5 已经检查了「能力层不依赖 nexus-repo」——
  AC #3 是在**测试**里再断言一次（Rust 侧也有），两者不冲突：
  shell 脚本给 CI 早期反馈，Rust 测试给 `cargo test` 反馈。
- 集成测试要用**独立库**（`nexus_test`），不能碰 `nexus_design` ——
  后者有参考实现的数据，测试会污染它。
- 本任务**不改**任何契约（表结构、列名、约束）。
  发现实现与参考实现的 schema 不符时，是**实现**错了 —— 按 §6.3 的纪律回退修正实现。
- `examples/demo-lease-fencing` 保留作为回归基准 —— 与 P1 的结论一致。

## 实现过程中的发现

<!-- 实现时填写 -->
