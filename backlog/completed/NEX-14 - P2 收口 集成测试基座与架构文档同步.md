---
id: NEX-14
title: "test(repo): P2 收口 —— 集成测试基座与架构文档同步"
status: Done
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

- [x] #1 集成测试基座落地：统一的「有库则跑、无库则跳过」机制，**不**让 CI 因缺库而红
- [x] #2 架构测试新增：`workflow_runs` 的类型只在 `nexus-execution` 出现（§4.2 差异 1）
- [x] #3 架构测试新增：`nexus-repo` 只依赖能力层、不被能力层依赖（§4.1.2）
- [x] #4 集成测试断言：迁移在空库可完整应用，且可重复执行
- [x] #5 集成测试断言：并发领取互不阻塞（3 连接各领不同 run）
- [x] #6 集成测试断言：generation fencing 拒绝僵尸写入
- [x] #7 集成测试断言：幂等提交返回既有 run
- [x] #8 集成测试断言：outbox 与状态同事务（回滚后 seq 不前进）
- [x] #9 集成测试断言：部分索引定义含 `WHERE` 子句
- [x] #10 架构文档 §4.2 对照表中 `nexus-repo` 相关行**已更新**为 P2 落地后的实际状态
- [x] #11 架构文档 §5.3 架构测试表的两项 P2 待办**已标记为完成**
- [x] #12 架构文档 §5.4 不变量表**已补** P2 已验证项
- [x] #13 架构文档 §6.1 的 P2 行**已标记**为完成
- [x] #14 CI 配置已加 PostgreSQL `services:` —— 集成测试在 CI 里**真的跑**
- [x] #15 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #16 CI 全部门禁全绿

## 备注

- **AC #14 是本任务最容易漏的一项** —— 本地有数据库不等于 CI 有。
  没有 `services:`，CI 里的集成测试会**全部跳过**，
  「集成测试绿」就是个假象。改 CI 后要**确认 CI 日志里真的跑了**集成测试
  （不是 0 passed）。

  > **实现时更正**：AC #14 有**两层**，初稿只想到第一层。
  >
  > | 层 | 初稿是否想到 | 实际状态 |
  > |---|---|---|
  > | ① CI 有没有 PG `services:` | ✅ 想到了 | **早已存在**（NEX-11 就有） |
  > | ② CI 的 PG 是不是**空库** | ❌ **没想到** | **是空库** → 集成测试全红 |
  >
  > 第 ② 层才是真缺口。已用 `apply-migrations` 二进制修复，
  > 并加了「解析 `cargo test` 输出」这一步防止套件没被编进来。
  >
  > 详见「实现过程中的发现」第 3、4、5 条。
- CI 的 PG 版本要与本地一致 —— **PostgreSQL 17**（参考实现的迁移在 17 上干净应用）。
- §5.3 的架构测试是**源码扫描**还是**依赖图断言**？
  `scripts/check-deps.sh` 规则 5 已经检查了「能力层不依赖 nexus-repo」——
  AC #3 是在**测试**里再断言一次（Rust 侧也有），两者不冲突：
  shell 脚本给 CI 早期反馈，Rust 测试给 `cargo test` 反馈。

  > **实现时更正**：规则 5 只覆盖了**反向**（能力层不依赖 `nexus-repo`）。
  > AC #3 的**正向**（`nexus-repo` 只依赖能力层）原先**任何脚本都没覆盖** ——
  > `nexus-repo` 依赖 `nexus-service` 能通过所有检查。
  > 已补 Rust 侧的 `nexus_repo_depends_only_on_capability_layer`，
  > 以及 `check-deps.sh` 规则 7（`deps_of` ↔ `Cargo.toml` 一致性核对）。
- 集成测试要用**独立库**（`nexus_test`），不能碰 `nexus_design` ——
  后者有参考实现的数据，测试会污染它。
- 本任务**不改**任何契约（表结构、列名、约束）。
  发现实现与参考实现的 schema 不符时，是**实现**错了 —— 按 §6.3 的纪律回退修正实现。
- `examples/demo-lease-fencing` 保留作为回归基准 —— 与 P1 的结论一致。

## 实现过程中的发现

**1. 任务初稿的前提「P2 落地后 `nexus-repo` 真正实现了 `nexus-rag` 的 trait」不成立。**
`crates/nexus-rag/src/lib.rs` 目前只有 `CRATE_NAME` 常量 —— **没有任何 `pub trait`**，
`nexus-repo` 里也没有对应的 `impl`。RAG 的内容属于 **P7（知识与记忆）**，不在 P2 范围。

所以 AC #10 没有按初稿写成「已实现 trait」，而是把 §4.3 的表格行改为
**⚠️ 部分**并加注记：依赖方向成立（`repo` 依赖 `rag`，架构测试锁住），
trait 的实证待 P7。**按 §6.3 的纪律，这是「实现还没到」而不是「契约错了」——
不改契约，改表述。**

**2. §5.3 声称「`deps_of` ↔ `Cargo.toml` 一致性核对」已落地，但脚本里没有这条规则。**
初稿写于 NEX-11 完成时，那条核对是**手工做的**（一次性比对），
没有变成可重复执行的门禁。文档声称与实现不符本身就是缺陷 ——
已补 `scripts/check-deps.sh` **规则 7**，逐 crate 比对两边的内部依赖集合。

**为什么这条必需**：规则 1/2/4/5 全部基于 `deps_of` 判定。
它若与真实 manifest 漂移，那些规则就变成「在错误的依赖图上做正确的判定」——
看起来全绿，实际什么都没拦住。

已用**注入依赖漂移**（给 `nexus-capacity` 加一条 `nexus-rag` 依赖）验证过规则 7 确实会红。

**3. AC #14 漏掉的真问题：CI 的 PG 服务起的是「空库」。**
初稿只关注「CI 有没有 `services:`」——那部分其实早就有了。
但 `postgres:17` 镜像只创建数据库，**不建任何表**；
而集成测试直接操作 `public` schema 的表。

本地复现：新建空库后跑 `nexus-repo` 集成测试 → **9 个全红**（表不存在）。
也就是说 **CI 的集成测试从来没绿过** —— 不是「跳过」，是「报错」。

修复：新增 `crates/nexus-infra/src/bin/apply-migrations.rs`
（跑嵌入式 `sqlx::migrate!` + 断言 Phase 0 的表齐了），
CI 增加「应用数据库迁移」步骤。

**为什么用 Rust 二进制而不是 CI 里装 `psql` 跑 `.up.sql`**：
迁移器已通过 `sqlx::migrate!` 嵌入二进制，版本记账（`_sqlx_migrations`）
由 sqlx 负责，**幂等**；走 psql 会绕过记账，迁移会被重复应用。

**4. `nexus-repo` 的集成测试基座在 CI 缺库时是「静默跳过」——与 `nexus-infra` 不一致。**
`nexus-infra/tests/migration.rs` 早就有「CI 缺 `DATABASE_URL` 直接 panic」，
`nexus-repo/tests/common/mod.rs` 却只打印 `[SKIP]` 然后 `return`。
被跳过的测试在摘要里显示为 `passed` —— CI 全绿，但一条数据库测试都没跑。

已把 `nexus-repo` 的基座改成与 `nexus-infra` **行为一致**：
CI 环境缺库 → `panic`（红）；本地缺库 → 跳过（绿）。
已用 `CI=1` + 删掉 `DATABASE_URL` 验证过确实会 panic。

**5. 「集成测试绿」有三种假象形态，三条防线都做了。**

| 假象形态 | 防线 |
|---|---|
| 无 `DATABASE_URL` → 全部跳过 → 摘要显示 `passed` | 基座在 CI 直接 panic |
| PG 服务是空库 → 表不存在 → 测试报错 | CI「应用数据库迁移」步骤 |
| 测试套件根本没被编进来 | CI 解析 `cargo test` 输出，断言各套件测试数达阈值 |

第三条的判据用 `running N tests` 那一行的 N ——
它紧跟在 `Running tests/xxx.rs` 后面，取值稳定；
而 `test result: ok. N passed` 那一行与 `Running` 行之间隔着 N 行测试明细，
窗口大小会随测试数量变化而出错（第一版就是这么写的，验证时才发现）。

**6. 允许集断言比负向断言更能防住「换了个地方违反架构」。**
NEX-11 的 `workflow_runs` 断言是**负向**的（`nexus-repo` 不许碰），
只管住了**一个** crate。`nexus-service` / `nexus-api` / `nexus-agent`
引用 `workflow_runs` 它照样绿 —— 而那同样破坏「只有执行层能写」的前提。

新增 `workflow_runs_appears_only_in_execution_owned_crates`：
**全 workspace** 扫描 `src/`，允许集只有 `nexus-execution`（独占方）
与 `nexus-infra`（迁移 DDL —— 建表不是访问）。

**踩到的坑**：第一版扫描了 `tests/` 目录，结果**这条测试把自己判成违规** ——
架构测试的允许集里就写着 `"workflow_runs"`。
约束的对象是**生产代码**，`tests/` 里出现只意味着「有测试在断言它」。
改为只扫 `src/`。

**7. `nexus-repo` 的依赖面断言补上了「正向」那一半。**
原先只有「能力层不依赖 `nexus-repo`」（反向）——
`nexus-repo` 依赖 `nexus-service`（上层）能通过，
而那会形成 `service → repo → service` 的环。**没有任何脚本覆盖这一侧。**

新增 `nexus_repo_depends_only_on_capability_layer`：
`nexus-repo` 的内部依赖全部落在「能力层 + 依赖图的根 + 同层的 `nexus-infra`」里。
`nexus-infra` 必须允许 —— 它是数据访问层，与 `nexus-repo` **平级**，
把它排掉会让断言拒绝一个**正确**的依赖。

**8. macOS 的 bash 3.2 在 `$c：`（全角冒号紧跟变量名）上会报 `unbound variable`。**
写 `check-deps.sh` 规则 7 时踩到的 —— `echo "✗ $c：依赖声明不一致"` 直接让脚本崩。
改成 `${c}：` 即可。CI 用的是 bash 5.x 不会有这个问题，但**本地能跑的脚本才是能用的脚本**。

**9. 集成测试的「空库」有两种含义，不要混。**
`nexus-infra/tests/migration.rs` 的 `fresh_pool` 建**独立 schema**
（`DROP SCHEMA ... CASCADE` + `CREATE SCHEMA`）——
它测的是「迁移能在空 schema 上应用」。

而 `nexus-repo` 的集成测试直接用 `public` schema —— 它们测的是「仓储能正常工作」，
前提是**表已经存在**。

所以 CI 需要的是「迁移应用到 `public`」，
而不是「每个测试建自己的 schema」。用 `apply-migrations` 二进制正好对上。

**10. 本任务的多数 AC 已被 NEX-9/11/12/13 覆盖 —— 收口任务的价值在「发现漏的那部分」。**
逐条核对后，AC #4~#9 的集成测试断言早已存在且通过；
真正的缺口是 **AC #1 / #14 的 CI 假象**、**AC #3 的正向断言缺失**、
以及**AC #10 的前提不成立**。
收口任务不是「重做一遍」，是「核对判据是否真的成立」。
