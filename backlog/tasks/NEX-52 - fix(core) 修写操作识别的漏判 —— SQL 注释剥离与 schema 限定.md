---
id: NEX-52
title: "fix(core): 修写操作识别的漏判 —— SQL 注释剥离、schema 限定、REPLACE/MERGE/COPY/TRUNCATE"
status: To Do
assignee: []
labels: [core, token-scan, architecture-test, contract-robustness]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.2 差异 1、§4.3.2 约束 2"
  - "backlog/completed/NEX-37 - feat(execution) 引擎侧持久化归属落地 —— 新增依赖与已定稿条款修订.md"
  - "crates/nexus-core/tests/support/token_scan.rs"
  - "crates/nexus-core/tests/token_scan.rs"
  - "crates/nexus-core/tests/architecture.rs"
---

## Description

**问题**：NEX-37 把「写入权唯一」的判定手段从「**类型**归属」改为「**写操作**归属」，
新增的底层工具 `extract_sql_writes` **漏判多种能造成写入的 SQL 形态** ——
于是「写操作只允许出现在 `nexus-execution`」这条契约**可以被绕过**。

**根因**：`crates/nexus-core/tests/support/token_scan.rs` 的 `sql_words()`
只处理 `'...'`（字符串字面量）、`"..."`（引号标识符）与标识符字符，
**不识别 SQL 注释**（`/* ... */` 与 `-- ...`）。
于是注释里的词会被当成**表名**：

| SQL | 当前提取结果 | 判定 |
|---|---|---|
| `INSERT INTO public.workflow_runs (id) VALUES ($1)` | `table: "public"` | ❌ 漏 |
| `INSERT /* c */ INTO workflow_runs (id) VALUES ($1)` | `table: "c"` | ❌ 漏 |
| `INSERT -- c\n INTO workflow_runs (id) VALUES ($1)` | `table: "c"` | ❌ 漏 |
| `UPDATE ONLY workflow_runs SET x=1` | `table: "ONLY"` | ❌ 漏 |
| `INSERT OR REPLACE INTO workflow_runs (id) VALUES ($1)` | `table: "OR"` | ❌ 漏 |
| `UPDATE public.workflow_runs SET x=1` | `table: "public"` | ❌ 漏 |
| `DELETE FROM public.workflow_runs WHERE id=$1` | `table: "public"` | ❌ 漏 |
| `REPLACE INTO workflow_runs (id) VALUES ($1)` | 无 | ❌ 漏 |
| `MERGE INTO workflow_runs t USING s ... THEN UPDATE SET` | `table: "SET"` | ❌ 漏 |
| `COPY workflow_runs (id) FROM STDIN` | 无 | ❌ 漏 |
| `TRUNCATE TABLE workflow_runs` | 无 | ❌ 漏 |

**实测绕过**（在独立 `git worktree` 注入，主仓库未改动）：

```
INSERT INTO public.workflow_runs (id) VALUES ($1)   => ★绕过★（两条断言全绿）
INSERT /* 幂等 */ INTO workflow_runs (id) VALUES ($1) => 抓到
INSERT INTO workflow_runs (id) VALUES ($1) ON CONFLICT (id) DO NOTHING => 抓到
```

把 `pub const BYPASS_PROBE: &str = "INSERT INTO public.workflow_runs (id) VALUES ($1)";`
加进 `crates/nexus-repo/src/users.rs` 后，
`nexus_repo_does_not_touch_workflow_runs` 与
`workflow_runs_appears_only_in_execution_owned_crates` **两条断言全绿** ——
违规写入成功逃逸。

**为什么这条必须修**：

NEX-37 的提交信息与架构文档 §4.3.2 约束 2 声称
「写操作**只允许**出现在 `nexus-execution`」「管住**任何 crate 的任何写操作**」。
当前实现**管不住**上表这 11 种形态 —— 那是**过度声明**。

> ⚠️ **当前代码没有实际违规**：NEX-37 预审已逐条核对 `nexus-repo` 对
> `workflow_runs` / `workflow_step_runs` 的**全部**引用 ——
> `lease.rs` / `leases.rs` / `outbox_repo.rs` / `rows.rs` / `lib.rs` 里的 5 处提及
> **全部在文档注释里**（`//!` / `///`），不是 SQL。
> 所以 NEX-37 的绿灯是**真绿**，不是假绿。
>
> 但**工具的健壮性**必须补上 —— 否则下一次有人写
> `INSERT INTO public.workflow_runs` 时，门禁会**静默放行**。

## 范围边界

### 做

- `sql_words()` 增加 **SQL 注释剥离**：`/* ... */`（可嵌套与否按 PostgreSQL 实际语义处理）
  与 `-- ...`（到行尾）
- 表名提取增加 **schema 限定**处理：`public.workflow_runs` → 表名 `workflow_runs`
  （`schema.table` 与 `"schema"."table"` 两种形态都要处理）
- 识别 **`REPLACE INTO`** / **`MERGE INTO`** / **`COPY ... FROM`** / **`TRUNCATE`**
  这四种**能造成写入**的形态
- 识别 `INSERT OR REPLACE INTO` / `UPDATE ONLY` 这类**修饰词**形态
- `crates/nexus-core/tests/token_scan.rs` **补齐对抗输入测试** ——
  现有 `sql_write_survives_formatting_variations` 覆盖了大小写 / 多空格 / 换行 / 引号，
  **没覆盖**注释与 schema 限定
- `crates/nexus-core/tests/architecture.rs` 的失败消息补上**已知边界**说明
- 架构文档 §4.3.2 约束 2 补「**识别覆盖的 SQL 形态**」的**准确边界**说明

### 不做

| 项 | 理由 |
|---|---|
| 改 **NEX-37 的依赖方向决策**（D12 选项 B） | 那是**操作者已裁决**的**已定稿条款**，本任务不动 |
| 改 `nexus-execution` / `nexus-repo` 的**业务代码** | 缺陷在**扫描工具**，不在业务代码 |
| 引入真正的 SQL 解析器（如 `sqlparser`） | 那是**新增依赖** + 依赖方向影响，**超出范围**；词法级修���足够 |
| 修围栏子句匹配的大小写 / 空格不敏感问题 | 见「关联发现」—— 那是**次要**问题，**独立登记** |
| 改 `scripts/check-deps.sh` | 规则 7 / 规则 8 预审已验证**有效** |

## Acceptance Criteria

- [ ] #1 `sql_words()` 剥离 `/* ... */` 注释 —— `INSERT /* c */ INTO workflow_runs` 提取出的表名是 `workflow_runs`
- [ ] #2 `sql_words()` 剥离 `-- ...` 行注释 —— `INSERT -- c\n INTO workflow_runs` 提取出的表名是 `workflow_runs`
- [ ] #3 schema 限定被正确剥离 —— `INSERT INTO public.workflow_runs` 与 `UPDATE public.workflow_runs` 与 `DELETE FROM public.workflow_runs` 的表名均为 `workflow_runs`
- [ ] #4 引号形式 `"public"."workflow_runs"` 与 `"workflow_runs"` 的表名均为 `workflow_runs`
- [ ] #5 `REPLACE INTO workflow_runs` 被识别为写操作
- [ ] #6 `MERGE INTO workflow_runs t USING s ON ... WHEN MATCHED THEN UPDATE SET ...` 被识别为写操作（目标表 `workflow_runs`）
- [ ] #7 `COPY workflow_runs (id) FROM STDIN` 被识别为写操作
- [ ] #8 `TRUNCATE TABLE workflow_runs` 被识别为写操作
- [ ] #9 `INSERT OR REPLACE INTO workflow_runs` 与 `UPDATE ONLY workflow_runs` 被识别为写操作（表名为 `workflow_runs`）
- [ ] #10 `ON CONFLICT DO UPDATE` 形态**不被重复计数**成两条写操作 —— 或明确记录「重复计数无害」并有测试说明
- [ ] #11 `crates/nexus-core/tests/token_scan.rs` 新增对抗输入测试**逐条覆盖** AC #1 ~ #9 的每个形态，且每条都断言「表名 = `workflow_runs`」
- [ ] #12 **故障注入实测**：把 `INSERT INTO public.workflow_runs (id) VALUES ($1)` 加进 `crates/nexus-repo/src/users.rs` 后，`nexus_repo_does_not_touch_workflow_runs` 与 `workflow_runs_appears_only_in_execution_owned_crates` **两条断言都变红**；注入后**还原**
- [ ] #13 读操作**仍不被误判**为写 —— `SELECT * FROM public.workflow_runs` 不算写；`CREATE TABLE public.workflow_runs (...)`（建表 DDL）不算写
- [ ] #14 字符串字面量里的内容**仍不算**写 —— `WHERE name = 'INSERT INTO workflow_runs'` 不算写
- [ ] #15 架构文档 §4.3.2 约束 2 补上**识别覆盖的 SQL 形态**的准确边界说明（明写当前**不**覆盖哪些形态，若还有）
- [ ] #16 `crates/nexus-core/tests/architecture.rs` 的失败消息让读到失败的人**知道边界在哪**（哪类形态不被识别）
- [ ] #17 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #18 `bash scripts/check-naming.sh` / `check-backlog.sh` / `check-specs.sh` / `check-docs.sh` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 18 条 AC，全部可由「提取结果等于某字符串」或
  「测试变红 / 不变红」或「脚本退出码」判定
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 架构文档 §4.2 / §4.3.2 存在；
  `crates/nexus-core/tests/support/token_scan.rs` / `token_scan.rs` / `architecture.rs` 均存在；
  NEX-37 任务文件存在（完成后在 `backlog/completed/`）
- 门禁 4（范围有界）：通过 —— 三个测试文件 + 一个架构文档章节，
  **不动**任何业务代码、**不动**依赖方向决策
- 门禁 5（受影响面被指名）：通过 ——
  `crates/nexus-core/tests/support/token_scan.rs`（**修改**）、
  `crates/nexus-core/tests/token_scan.rs`（**修改**）、
  `crates/nexus-core/tests/architecture.rs`（**修改**：失败消息）、
  `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`（**修改**：§4.3.2 边界说明）
- 门禁 6（完成态可描述）：通过 —— 写操作识别覆盖注释、schema 限定、
  `REPLACE` / `MERGE` / `COPY` / `TRUNCATE`；
  `INSERT INTO public.workflow_runs` 注入后**两条断言都变红**
- 门禁 7（无隐形依赖）：通过 —— 无前置任务；
  缺陷已由 NEX-37 预审**实测复现**（独立 worktree 注入），不是假设

**结论：pass**

## 关联发现（**不在本任务范围**）

NEX-37 预审同时发现**围栏子句匹配**的次要缺陷，另行登记：

- `crates/nexus-execution/tests/architecture.rs` 的 `the_execution_crate_does_not_inline_fenced_writes`
  用**子串**匹配 SQL 片段。改写措辞会漏：
  `generation=?`（无空格）、`GENERATION = ?`（大写）**均逃逸**。
- ⚠️ **这不是绕过路径** —— 绕过路径是「复制完整围栏模板到执行层」，
  无论怎么写都会被六子句里任意一条抓到（**当前行为正确**）。
  漏判只影响「**围栏退化**」（少抄一条子句）的检出，属于**次要**问题。

## 决策记录

### 类别：decide-and-proceed

- **决策**：本任务**单独登记**，**不**在 NEX-37 内就地修复。
- **依据**：NEX-37 触及**已定稿条款**（§6.3 第 5 项 crate 边界），
  按 `ai-sdlc-quality-gates.mdc` 评审发现的**契约级问题不得就地修复** ——
  修复后**必须重跑评审**。且当前代码**无实际违规**，
  强行塞进 NEX-37 会把「依赖方向落地」与「扫描器健壮性」两件事混在一个评审里。
- **可逆性**：高 —— 纯测试工具与文档修订，不触及生产代码。
- **影响面**：三个测试文件 + 架构文档 §4.3.2。

### 类别：decide-and-proceed

- **决策**：不引入 `sqlparser` 等真正的 SQL 解析器。
- **依据**：词法级修复（注释剥离 + schema 剥离 + 四种形态）足以覆盖
  PostgreSQL 的写操作形态；引入解析器会**新增依赖**（`nexus-core` 是依赖图的根），
  触及依赖方向，**超出范围**。
- **可逆性**：n/a（不动作）。
- **影响面**：无。
