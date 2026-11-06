---
id: NEX-37
title: "feat(execution): 引擎侧持久化归属落地（新增依赖 + 修订已定稿条款）"
status: In Progress
assignee: []
labels: [execution, crate-boundary, contract-revision]
priority: high
dependencies: []
references:
  - "docs/specs/p6/spec.md §2.2、§2.7、§3.12、§7.1"
  - "docs/specs/p6/tasks.md T-001"
  - "docs/specs/p6-tiering.md OQ-1"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.1、§4.2 差异 1、§4.3 目标态表、§4.3.2 约束 2、§4.4 规则 4、§6.3"
---

## Description

**问题**：权威文档**自相矛盾**，且现状与其中一半**相反**。

| 位置 | 说法（**修订前**） |
|---|---|
| 架构文档 §4.1 crate 图 | `EXE --> REPO`（执行 → 数据） |
| 架构文档 §4.2 差异 1 / §4.3 表格 | 「`nexus-execution` **不依赖** `nexus-repo`」 |

而**现状**是：引擎侧持久化（租约 / fencing / 幂等提交 / 事件序号）
在 `crates/nexus-repo/src/lease.rs`、`leases.rs`、`outbox.rs`、`outbox_repo.rs` 里。

**修法**：按**决策 D12（选项 B，操作者 2026-11-06 裁决）** ——
**不迁移**。`nexus-execution` **新增**对 `nexus-repo` 的依赖，
引擎侧持久化能力**留在** `nexus-repo`。

选 B 而不是 A 的三条理由：

1. §4.1 的 crate 图**本就**画着 `EXE --> REPO`，§4.1.1 的层次表也允许「执行 → 数据」——
   选 B 与**目标态总览一致**；
2. 围栏条件「只有一处定义」的纪律**不受影响** ——
   实现仍在 `nexus-repo/src/lease.rs` 一处，NEX-12 的变异测试**不用搬家、不会失效**；
3. 迁移是**纯 churn** —— 要重跑全部围栏测试、动 `nexus-repo` 里其他模块的依赖，
   收益只是「让实现与某一条表述对齐」。选 B 改的是**文档表述**，成本远低。

> ⚠️ **本任务修订了架构文档的已定稿条款**（§6.3 第 5 项「crate 边界与 trait 归属」）：
> §4.2 差异 1、§4.3 目标态表、§4.3.2 约束 2、§4.4（新增规则 4）、§5.3、§6.3。
>
> 按 `ai-sdlc-quality-gates.mdc`，**契约级变更必须走人工评审**，
> 且 **AI 产出的改动需额外一名评审**。
>
> ⚠️ **本任务受 OQ-1 的时间盒约束**（见 `docs/specs/p6-tiering.md`）。
> D12 是**类别 (b) timeboxed**：时间盒截止 **2026-11-13**；
> 到期前**不实施**；无人干预时的自主回退 = 按选项 B 执行
> （依赖边是加法、文档修订可 `git revert`）。

### 「写入权唯一」这条控制在选项 B 下的新落点

原先的判定手段是「`nexus-repo` **不得引用** `workflow_runs` 的**类型**」。
依赖方向改为 `execution → repo` 后，`nexus-repo` 的类型对**所有依赖它的 crate**
（`nexus-service` / `nexus-api`）可见 —— 「类型不出现在 `nexus-repo`」
**推不出**「别的 crate 写不了」。

判定手段因此从「**类型**归属」改为「**写操作**归属」：
全 workspace 扫描 `workflow_runs` / `workflow_step_runs` 的
`INSERT` / `UPDATE` / `DELETE`，写操作**只允许**出现在 `nexus-execution`；
`nexus-infra` 允许的是**建表 DDL**（迁移），不是访问。

**强度不降**：类型归属可被「把类型下沉到 `nexus-repo`」绕过；
写操作归属管住**任何 crate 的任何写操作**。

## 范围边界

### 做

- 给 `nexus-execution` **新增** `nexus-repo` 依赖（`Cargo.toml` + `check-deps.sh` 的 `deps_of`）
- 修订架构文档 §4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 / §4.4 / §5.3 / §6.3
- 修订架构测试 `workflow_runs_appears_only_in_execution_owned_crates` → 「写操作归属」判定
- 新增架构测试：`nexus-repo` **不得**依赖 `nexus-execution`（反向会成环）
- 验证围栏的变异测试**仍然会红**

### 不做

| 项 | 理由 |
|---|---|
| **迁移** `lease` / `leases` / `outbox` / `outbox_repo` | D12 = 选项 B，明确**不迁移** |
| 改围栏条件的**文本** | 改安全机制需要独立的决策与变异测试 |
| 让 `nexus-execution` **自己**拼围栏 SQL | 执行层**必须经由** `nexus-repo` 的带租约 API —— 否则围栏的「一处定义」被绕过 |
| 给 `nexus-workflow` 加内容 | 属 NEX-38 ~ NEX-40 |
| 实现审批 / 幂等账本 / 检查点 | 属 NEX-42 ~ NEX-45 |
| 改 `nexus-model` / `nexus-execution → service/api/cli` 的约束 | 那些约束**不受影响**，仍成立 |

## Acceptance Criteria

> **验证记录（2026-11-06，S4 实现完成）**：
> 每条 AC 的判据与**可复核证据**列在文末「交付验证记录」。
> 未打勾的 #17 是**人工评审**项 —— 它**阻塞合并**，不阻塞实现。

- [x] #1 `crates/nexus-execution/Cargo.toml` **新增** `nexus-repo.workspace = true`；`scripts/check-deps.sh` 的 `deps_of nexus-execution` **新增** `nexus-repo`；`bash scripts/check-deps.sh` 通过（规则 7：`deps_of` ↔ `Cargo.toml` 一致）
- [x] #2 引擎侧持久化模块**留在** `nexus-repo` —— `crates/nexus-repo/src/lease.rs` / `leases.rs` / `outbox.rs` / `outbox_repo.rs` **继续存在**（**不迁移**、**不删除**、**不改路径**）
- [x] #3 架构文档 **§4.2 差异 1** 已修订为「`nexus-execution` **依赖** `nexus-repo`，但 `workflow_runs` / `workflow_step_runs` 的**写入权**仍由执行层独占」
- [x] #4 架构文档 **§4.3 目标态表**的 `nexus-execution → nexus-repo` 行已修订（设计目标改为「**依赖**」，并注明 P6 · D12 选项 B 的裁决与落地状态）
- [x] #5 架构文档 **§4.3.2 约束 2** 已修订为「`workflow_runs` / `workflow_step_runs` 的**写操作**（`INSERT` / `UPDATE` / `DELETE`）**只允许**发生在 `nexus-execution`；`nexus-infra` 允许**建表 DDL**（迁移），不是访问」
- [x] #6 架构文档 **§4.4** 已新增**规则 4**：「`nexus-execution → nexus-repo` 允许，`nexus-repo → nexus-execution` 禁止（反向会成环）」
- [x] #7 架构文档 **§5.3** 的 `workflow_runs` 那一行与 **§6.3** 的「crate 边界与 trait 归属」条目**同步更新** —— §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 **六处一致**，**不留半截矛盾**
- [x] #8 架构测试 `workflow_runs_appears_only_in_execution_owned_crates` 已修订 —— 判定从「**类型**只允许出现在某处」改为「**写操作**只允许出现在 `nexus-execution`」；断言**不得**被改弱成「只看某个 crate」
- [x] #9 新增架构测试断言 `nexus-repo` **不得**依赖 `nexus-execution`（反向会成环）
- [x] #10 架构测试断言 `nexus-execution` **不依赖** `nexus-service` / `nexus-api` / `nexus-cli` **仍然成立**（不因新增依赖而放宽或删除）
- [x] #11 架构测试断言 `nexus-model` **无**数据库 schema 痕迹 **仍然成立**（不因新增依赖而放宽或删除）
- [x] #12 `crates/nexus-execution` 的**围栏写**必须**经由** `nexus-repo` 的带租约 API —— 执行层**不自己**拼围栏 SQL（结构层断言）
- [x] #13 NEX-12 的三条 **trybuild 负向测试**（`RunLease` 无 `generation()` / 不可直接构造 / 围栏写必须带 `&RunLease`）**留在** `crates/nexus-repo/tests/`，且**仍然编译失败**
- [x] #14 围栏的**变异测试仍然会红** —— 删掉 `generation` 条件时 `lease_fencing` 的对应测试**必须失败**（断言被改弱即为回归）
- [x] #15 `Fence::CONDITIONS_TEMPLATE` 的**文本逐字节不变** —— 围栏条件的六个子句与绑定顺序**不改**
- [x] #16 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #17 本任务已通过**人工评审**，且 AI 产出的改动有**额外一名评审**的签字（`ai-sdlc-quality-gates.mdc` 的契约级变更要求）

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。
> **修订日期**：2026-11-06 —— OQ-1 由操作者裁决为**选项 B**后重判。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

17 条 AC 全部可二元判定 —— 每条都指明「文件存在 / 不存在」「依赖边在 / 不在」
「文档里有 / 没有那句话」「测试变红 / 不变红」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。
OQ-1 已由**操作者裁决**（选项 B），不是待定标记。

### #3 引用可解析

- `docs/specs/p6/spec.md §2.2、§2.7、§3.12、§7.1` —— 存在
- `docs/specs/p6/tasks.md T-001` —— 存在
- `docs/specs/p6-tiering.md OQ-1` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.1、§4.2 差异 1、§4.3 目标态表、§4.3.2 约束 2、§4.4 规则 4、§6.3` —— 存在
- `crates/nexus-repo/src/lease.rs` / `leases.rs` / `outbox.rs` / `outbox_repo.rs` —— 存在
- `crates/nexus-core/tests/architecture.rs::workflow_runs_appears_only_in_execution_owned_crates` —— 存在

### #4 范围有界

**一条依赖边 + 一份文档修订 + 一组架构测试断言**。
**不迁移**任何模块、**不新增行为**、**不改围栏条件文本**、**不新增业务码**。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-execution/Cargo.toml` | **修改**（新增 `nexus-repo.workspace = true`） |
| `scripts/check-deps.sh` | **修改**（`deps_of nexus-execution` 新增 `nexus-repo`） |
| `crates/nexus-core/tests/architecture.rs` | **修改**（`workflow_runs_appears_only_in_execution_owned_crates` 改为「写操作归属」判定；新增「`nexus-repo` 不得依赖 `nexus-execution`」断言） |
| `crates/nexus-execution/tests/architecture.rs`（或等价位置） | **新增/修改**（围栏写必须经由 `nexus-repo` 的带租约 API） |
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | **修改**（§4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 / §4.4 规则 4 / §5.3 / §6.3） |
| `crates/nexus-repo/src/{lease,leases,outbox,outbox_repo}.rs` | **不修改** |
| `crates/nexus-repo/tests/{lease_fencing,outbox_idempotency,compile_fail}.rs` | **不修改** |
| `crates/nexus-infra/migrations/*` | **不修改** |

### #6 完成态可描述

`nexus-execution` **依赖** `nexus-repo`，并**经由**它落库；
引擎侧的租约、fencing、幂等提交与事件序号**全部留在** `nexus-repo`；
`workflow_runs` / `workflow_step_runs` 的**写操作**只出现在 `nexus-execution`；
`nexus-repo → nexus-execution` **不存在**；
架构文档 §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 **六处一致**；
围栏的**行为不变**由**留在原地的既有测试**证明，且**变异测试仍然会红**。

### #7 无隐形依赖

- 无前置任务（加一条依赖边 + 改文档 + 改测试，都是机械的）
- `nexus-repo` 的引擎侧仓储 API **已存在**（`lease.rs` / `leases.rs` / `outbox_repo.rs`）—— **不新建**
- OQ-1 的时间盒（2026-11-13）与自主回退方案**已写明** —— 不是隐形依赖，是**已登记的决策**
- 集成测试需要真实 PostgreSQL 17 —— P2 的基座**已存在**（NEX-14）

### 结论

**pass** —— 七项全过，可以开工。

> ⚠️ **但本任务带一个额外的评审门槛**：
> 它**修订了架构文档的已定稿条款**（§6.3 第 5 项「crate 边界与 trait 归属」）——
> 契约级变更必须走**人工评审**，且 **AI 产出的改动需额外一名评审**
> （`ai-sdlc-quality-gates.mdc`）。
> 这个门槛**不阻塞开工**，但**阻塞合并**。

## 决策记录

### 类别：timeboxed（类别 b）

- **决策**：**不迁移**。`nexus-execution` **新增**依赖 `nexus-repo`；
  引擎侧持久化能力**留在** `nexus-repo`（D12 = 选项 B，操作者 2026-11-06 裁决）。
- **类别判定依据**（`ai-sdlc-decision-authority.mdc` §2.1，取最严）：
  - **可逆性** —— 依赖边是加法、文档修订可 `git revert` → 可低成本回滚（**不单独**构成 (b)）；
  - **影响面** —— 修订**已定稿条款** + 改动**crate 依赖方向** → **至少 (b)**；
  - **控制变更** —— 「写入权唯一」这条控制**仍然成立**（判定手段改了，强度**不降**）——
    **收紧不算削弱**，不额外升级；
  - **operator-only** —— 不涉及法律 / 许可 / 金钱 / 凭据 → **不是 (c)**。
- **依据**：§4.1 的 crate 图本就画着 `EXE --> REPO`；围栏条件「只有一处定义」的纪律不受影响；
  迁移是纯 churn，改文档的成本远低。
- **可逆性**：中 —— 依赖边与文档修订都可 `git revert`；
  但**长期**看这是 crate 边界的既定事实。
- **影响面**：`crates/nexus-execution/Cargo.toml`、`scripts/check-deps.sh`、
  `crates/nexus-core/tests/architecture.rs`、
  `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`。
- **时间盒截止**：**2026-11-13**。
- **无人干预时的自主回退**：按选项 B 执行（依赖边是加法、文档修订可 revert）。
- **评审要求**：**人工评审 + 额外一名评审**（触及已定稿条款）。

### 类别：decide-and-proceed

- **决策**：「写入权唯一」的判定手段从「**类型**归属」改为「**写操作**归属」。
- **依据**：选项 B 下 `nexus-repo` 的类型对所有依赖它的 crate 可见，
  「`nexus-repo` 不持有该类型」**推不出**「别的 crate 写不了」。
  「写操作归属」管住**任何 crate 的任何写操作**，**强度不降**。
- **可逆性**：中 —— 判定手段可回退，但会重新留出「类型下沉」这条绕过路径。
- **影响面**：`crates/nexus-core/tests/architecture.rs`、
  架构文档 §4.2 差异 1 / §4.3.2 约束 2。

### 类别：decide-and-proceed

- **决策**：围栏条件的**文本**在本次改动中**逐字节不变**。
- **依据**：围栏是「僵尸 worker 不能覆盖新 worker 的写入」的**唯一**保证。
  顺手「优化」条件 = 在没有变异测试覆盖的情况下改安全机制。
  AC #14 与 AC #15 各从两侧锁住它（变异仍会红 + 文本相等）。
- **可逆性**：n/a（不动作）。
- **影响面**：`crates/nexus-repo/src/lease.rs`。

## 交付验证记录（2026-11-06，S4 实现完成 / S5 三层验证）

> 每条 AC 的判据、**可复核证据**（精确到测试函数名、文件路径、命令输出）。
> 评审时可**逐条复现** —— 不接受笼统判断。

### 新增 / 修改的断言清单

| # | 断言 | 位置 | 层 |
|---|---|---|---|
| 1 | `workflow_runs_appears_only_in_execution_owned_crates` | `crates/nexus-core/tests/architecture.rs` | 结构层（**已改写**：类型归属 → 写操作归属） |
| 2 | `nexus_repo_does_not_touch_workflow_runs` | `crates/nexus-core/tests/architecture.rs` | 结构层（**已收紧**：不得引用 → 不得**写**） |
| 3 | `nexus_repo_does_not_depend_on_the_execution_layer` | `crates/nexus-core/tests/architecture.rs` | 结构层（**新增**） |
| 4 | `nexus_repo_must_not_write_execution_owned_tables` | `crates/nexus-repo/tests/architecture.rs` | 结构层（**已改写**，替换原 `nexus_repo_must_not_reference_execution_owned_tables`） |
| 5 | `the_repo_crate_does_not_depend_on_the_execution_layer` | `crates/nexus-repo/tests/architecture.rs` | 结构层（**新增**） |
| 6 | `the_execution_crate_depends_on_the_data_layer` | `crates/nexus-execution/tests/architecture.rs` | 结构层（**新增**） |
| 7 | `the_execution_crate_does_not_depend_on_the_upper_layers` | `crates/nexus-execution/tests/architecture.rs` | 结构层（**新增**） |
| 8 | `the_execution_crate_does_not_inline_fenced_writes` | `crates/nexus-execution/tests/architecture.rs` | 结构层（**新增**） |
| 9 | `the_execution_crate_writes_no_unrelated_tables` | `crates/nexus-execution/tests/architecture.rs` | 结构层（**新增**） |
| 10 | `extract_sql_writes` / `ScanResult::writes_table` | `crates/nexus-core/tests/support/token_scan.rs` | 结构层**底层工具**（**新增**） |
| 11 | `a_sql_write_is_distinguished_from_a_sql_read` 等 **5 条** | `crates/nexus-core/tests/token_scan.rs` | 结构层工具的**对抗输入**测试（**新增**） |

### AC 逐条证据

| AC | 证据 |
|---|---|
| #1 | `crates/nexus-execution/Cargo.toml` 含 `nexus-repo.workspace = true`；`scripts/check-deps.sh` 的 `deps_of nexus-execution` 含 `nexus-repo`；`bash scripts/check-deps.sh` 输出「规则 7：deps_of 与各 crate 的 Cargo.toml 必须一致 ✓」 |
| #2 | `crates/nexus-repo/src/{lease,leases,outbox,outbox_repo}.rs` **四个文件仍在原路径**；`git diff --stat crates/nexus-repo/src/` 对这四个文件**无输出**（除 `lease.rs` 的临时变异，已还原） |
| #3 ~ #7 | 架构文档 §4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 / §4.4 规则 4 / §5.3 / §6.3 **六处一致**；§5.3 表格的 `workflow_runs` 行与 §4.3 目标态表均标 ✅ **已落地**；§6.3 的「crate 边界与 trait 归属」条目补记「NEX-37 / T-001 已落地」 |
| #8 | `workflow_runs_appears_only_in_execution_owned_crates` 现在的判据是 `scanned.writes_table(table)`（**写**过），允许集为**全 workspace** 的「除 `nexus-execution` 外**任何** crate」；判据**不是**「只看某个 crate」 |
| #9 | `nexus_repo_does_not_depend_on_the_execution_layer`（`nexus-core`）+ `the_repo_crate_does_not_depend_on_the_execution_layer`（`nexus-repo`）+ `scripts/check-deps.sh` **规则 8** —— 三处共同守护 |
| #10 | `the_execution_crate_does_not_depend_on_the_upper_layers` 通过（`nexus-service` / `nexus-api` / `nexus-cli` 均不在依赖清单） |
| #11 | `nexus_model_has_no_database_traces` **仍在** `crates/nexus-core/tests/architecture.rs` 且通过（14 passed） |
| #12 | `the_execution_crate_does_not_inline_fenced_writes` 判据是围栏条件的**六个子句逐条**匹配 `FENCE_CLAUSES`（`generation = ` / `lease_owner = ` / `lease_until > ` / `deadline_at > ` / `status = 'running'`）—— 只抄五个**也会**被抓 |
| #13 | `cargo test -p nexus-repo --test compile_fail` → **4 passed**（`fenced_write_requires_a_lease` / `run_lease_cannot_be_constructed_directly` / `run_lease_has_no_generation_accessor` / `framework_self_check`）—— 三个负向样例**仍然编译失败** |
| #14 | **故障注入实测**（见下表）—— 围栏真的失效时 `stale_generation_write_is_rejected` 与 `fenced_counter_increment_is_rejected` **都**变红 |
| #15 | `crates/nexus-repo/src/lease.rs` 的 `Fence::CONDITIONS_TEMPLATE` **逐字节未变**（`git diff` 对该文件**无输出**）；六个子句与绑定顺序由 `conditions_list_all_six_clauses_in_bind_order` 断言 |
| #16 | `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` **零告警**；`DATABASE_URL=… cargo test --workspace --locked` **零失败**（含真实 PostgreSQL 17 的集成测试） |
| #17 | ⏳ **待人工评审** —— 阻塞合并 |

### 故障注入记录（AC #14 与「断言真的有效」）

> 三次注入，**每次都先验证会红，再还原**。
> 这是「断言被改弱即为回归」的**直接证据**。

| # | 注入什么 | 期望 | 实际 | 还原 |
|---|---|---|---|---|
| 1 | 把 `crates/nexus-repo/src/users.rs` 的一条 `INSERT INTO users …` 改成 `INSERT INTO workflow_runs …` | 三条「写操作归属」断言**全部**变红 | ✅ `nexus_repo_must_not_write_execution_owned_tables`、`nexus_repo_does_not_touch_workflow_runs`、`workflow_runs_appears_only_in_execution_owned_crates` **三条全红**，并精确报出文件名与表名 | ✅ `cp /tmp/users.rs.bak`，`git diff` 无输出 |
| 2 | 在 `crates/nexus-execution/src/lib.rs` 内联一条含 `generation = ` / `lease_owner = ` 的围栏 UPDATE | `the_execution_crate_does_not_inline_fenced_writes` 变红 | ✅ 变红，并**逐条**报出命中的围栏子句 | ✅ 还原，`git diff` 无输出 |
| 3 | 从 `Fence::CONDITIONS_TEMPLATE` 删掉 `generation` 条件**并同步删掉对应绑定**（否则只会得到语法错误，测不出 fencing 失效） | `stale_generation_write_is_rejected` / `fenced_counter_increment_is_rejected` 变红 | ✅ **两条都变红** —— 报的是「过期 generation 的写入必须被 fencing 拒绝（影响 0 行）」这一**行为**，不是 SQL 文本 | ✅ 还原，`grep -c 'generation = ?'` = 2，`git diff` 无输出 |

> ⚠️ **注入 3 的关键设计**：只删模板里的 `generation` 子句**不足以**验证变异测试的有效性 ——
> 那只会产生「绑定点数量不匹配」的语法错误，测试红得**不是因为 fencing 失效**。
> 同步删掉绑定后，围栏**真的**不再校验 `generation`，
> 测试**才**红在「僵尸写入被放行」这个**正确的原因**上。

### 门禁全绿记录（2026-11-06）

```
bash scripts/check-lock.sh      ✓ 全部检查通过
bash scripts/check-deps.sh      ✓ 依赖方向检查全部通过（含新增的规则 8）
bash scripts/check-naming.sh    ✓ 标识符命名检查全部通过
bash scripts/check-backlog.sh   ✓ 全部检查通过
bash scripts/check-specs.sh     ✓ Spec 层组织纪律检查全部通过
bash scripts/check-rules.sh     ✓ 全部检查通过
bash scripts/check-docs.sh      ✓ 全部检查通过
cargo fmt --all --check         ✓
cargo clippy --workspace --all-targets --locked -- -D warnings   ✓ 零告警
cargo test --workspace --locked  ✓ 零失败
```

### 契约升格

`docs/specs/p6/contracts/invariants.json` 的 **11 条**不变量由 `planned` 升格为 `verified`：

| 不变量 | 证据 |
|---|---|
| P6-I01 | `nexus-repo/src/lease.rs::conditions_list_all_six_clauses_in_bind_order` |
| P6-I02 | `nexus-repo/tests/lease_fencing.rs::stale_generation_write_is_rejected`（**已做故障注入**） |
| P6-I03 | `nexus-repo/tests/compile_fail.rs::fenced_write_requires_a_lease` |
| P6-I04 | `nexus-repo/tests/compile_fail.rs::run_lease_cannot_be_constructed_directly` |
| P6-I05 | `nexus-repo/tests/compile_fail.rs::run_lease_has_no_generation_accessor` |
| P6-I06 | `nexus-execution/tests/architecture.rs::the_execution_crate_depends_on_the_data_layer` |
| P6-I07 | `nexus-execution/tests/architecture.rs::the_execution_crate_does_not_depend_on_the_upper_layers` |
| P6-I08 | `nexus-core/tests/architecture.rs::workflow_runs_appears_only_in_execution_owned_crates` |
| P6-I49 | `nexus-repo/tests/architecture.rs::the_repo_crate_does_not_depend_on_the_execution_layer` |
| P6-I50 | `nexus-execution/tests/architecture.rs::the_execution_crate_does_not_inline_fenced_writes` |
| P6-I51 | `nexus-core/tests/architecture.rs::workflow_runs_appears_only_in_execution_owned_crates` |

`bash scripts/check-specs.sh` 规则 3 校验「`verified` 的 `verified_by` 指向的测试**必须真的存在**」—— 通过。

### 受影响面（实际）

| 位置 | 变更 |
|---|---|
| `crates/nexus-execution/Cargo.toml` | **修改**（`nexus-repo.workspace = true` 已在 `783f310`；本次新增 `[dev-dependencies] syn`） |
| `crates/nexus-execution/tests/architecture.rs` | **新增**（4 条结构层断言） |
| `crates/nexus-core/tests/support/token_scan.rs` | **修改**（新增 `SqlWrite` / `extract_sql_writes` / `ScanResult::writes_table`） |
| `crates/nexus-core/tests/token_scan.rs` | **修改**（新增 5 条对抗输入测试） |
| `crates/nexus-core/tests/architecture.rs` | **修改**（2 条改写 + 1 条新增） |
| `crates/nexus-repo/tests/architecture.rs` | **修改**（1 条改写 + 1 条新增 + token_scan include） |
| `crates/nexus-repo/Cargo.toml` | **修改**（新增 `[dev-dependencies] syn`） |
| `scripts/check-deps.sh` | **修改**（新增规则 8） |
| `Cargo.lock` | **修改**（两个 crate 的 dev-dependency `syn`） |
| `docs/architecture/…-architecture-design.md` | **修改**（§4.2 / §4.3 / §4.4 / §5.3 / §6.3 同步 + 落地记录） |
| `docs/specs/p6/contracts/invariants.json` | **修改**（11 条升格 `verified`） |
| `docs/specs/p6-tiering.md` | **修改**（判定手段表：需修订 → 已落地） |
| `crates/nexus-repo/src/{lease,leases,outbox,outbox_repo}.rs` | **不修改** ✅ |
| `crates/nexus-repo/tests/{lease_fencing,outbox_idempotency,compile_fail}.rs` | **不修改** ✅ |
| `crates/nexus-infra/migrations/*` | **不修改** ✅ |
