---
id: NEX-46
title: "docs(execution): P6 收口 —— 架构文档同步与契约升格"
status: To Do
assignee: []
labels: [docs, closeout, p6]
priority: medium
dependencies:
  - NEX-37
  - NEX-38
  - NEX-39
  - NEX-40
  - NEX-41
  - NEX-42
  - NEX-43
  - NEX-44
  - NEX-45
references:
  - "docs/specs/p6/spec.md §6、§7"
  - "docs/specs/p6/tasks.md T-010"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1、§4.2、附录 C"
---

## Description

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

### 这个任务的**特殊职责**：契约升格

`docs/specs/p6/contracts/invariants.json` 里的 51 条不变量在 Spec 阶段是 `planned` ——
**证据尚未产生**（这正是 S2 必须先于 S4 的必然结果）。

本任务的职责是**把 `planned` 全部升格为 `verified`**，
并且**验证每个测试真的存在**。

`bash scripts/check-specs.sh` 的规则 3b 会在本任务归档后要求：
所有 `planned` 条目必须已升格 —— 否则门禁红。

**这不是「勾选框」** —— 升格前必须确认测试**真的存在**且**真的断言了那条不变量**。

### 另一个**特殊职责**：消解架构文档的**自相矛盾**

架构文档 §4.1 的 crate 图画着 `EXE --> REPO`，
而 §4.2 差异 1 与 §4.3 的表格（**修订前**）说「`nexus-execution` 不依赖 `nexus-repo`」。

> ⚠️ **T-001（决策 D12 · 选项 B）落地后，这个矛盾**已经**在 T-001 里被消解了** ——
> 修订走的是**条款**方向（改 §4.2 / §4.3 / §4.3.2 / §4.4），
> §4.1 的图**本就**与修订后一致。
>
> 所以本任务的 AC #3 的含义变了：
> 它**不再**是「消解一个仍然存在的矛盾」，
> 而是「**核对** T-001 的修订是否**一致地**覆盖了 §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 / §6.3
> **七处** —— 不得留半截矛盾」。
>
> 如果核对发现某处**漏改**（例如 §5.3 的架构测试表还写着旧的判定手段），
> **补改**它 —— 但那属于**修订一致性核对**，不是**消解矛盾**。

### 第三个**特殊职责**：故障注入

NEX-24 / NEX-25 / NEX-35 的教训：**没验证过会红的断言，等于没有断言**。

本任务必须对 P6 的关键断言做**故障注入**，验证**确实会红**。

## 范围边界

### 做

- 架构文档 §6.1 的 P6 条目同步 + §4.2 差异 1 的落地记录 + §4.1 / §4.2 矛盾的消解
- 架构文档**附录 C** 的 `UnknownOutcome` 项升格
- `contracts/invariants.json` 的 `planned` → `verified` 升格
- `docs/operations/` 的 P6 运行说明
- **故障注入**验证与记录

### 不做

| 项 | 理由 |
|---|---|
| 补写缺失的测试 | 那是 NEX-37 ~ NEX-45 的职责；本任务**只核对**，发现缺失就**退回**对应任务 |
| 修改 P3 / P4 / P5 的契约 | 已交付，契约已定稿 |
| 实现 F4 ~ F8 | 需人工授权后才创建 |

## Acceptance Criteria

- [ ] #1 架构文档 §6.1 的 P6 行标记为**完成**，并附**可核验**的交付摘要（不变量条数、测试数）
- [ ] #2 架构文档 §4.2 差异 1 的「落地记录」补入 T-001 的**实际形态**（依赖边方向、落在哪个 crate、模块名、测试名）
- [ ] #3 架构文档 §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 / §6.3 **七处**对 `nexus-execution → nexus-repo` 依赖方向与「`workflow_runs` **写操作**独占」的表述**一致**（T-001 已修订条款；本任务**核对**一致性，漏改处**补改** —— 不得留半截矛盾）
- [ ] #4 架构文档**附录 C** 的 `UnknownOutcome` 分类从「🟡 部分验证」升为「✅ 已验证」，并附可核验的测试名
- [ ] #5 `docs/specs/p6/contracts/invariants.json` 的全部不变量 `status` 提升为 `verified`
- [ ] #6 `bash scripts/check-specs.sh` 通过（规则 3b：任务全部归档后不得残留 `planned`）
- [ ] #7 **故障注入**：对 P6 的关键断言做注入，验证**确实会红** —— 至少覆盖：围栏 `generation` 条件、三元组校验、`manual` 策略的 `unknown` 化、取消的顺序、审批过期
- [ ] #8 故障注入的记录写进任务文件（含「注入什么 / 期望 / 实际」）
- [ ] #9 `docs/operations/` 有 P6 的运行说明（需要 PostgreSQL 17 的集成测试怎么跑、缺 `DATABASE_URL` 会 panic）
- [ ] #10 `bash scripts/check-docs.sh` 通过；`bash scripts/check-backlog.sh` 通过
- [ ] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #12 CI 全部门禁全绿

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「文档里有什么」或「命令的退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §6、§7` —— 存在
- `docs/specs/p6/tasks.md T-010` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1、§4.2、附录 C` —— 存在

### #4 范围有界

文档 + 契约文件。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | **修改** —— §4.1 / §4.2 / §6.1 / 附录 C |
| `docs/specs/p6/contracts/invariants.json` | **修改** —— status 升格 |
| `docs/operations/` 下的运行说明 | **修改/新增** |
| `docs/README.md` | **修改**（登记新文档） |
| `backlog/completed/NEX-46 - ...md` | **新增** —— 归档 |

### #6 完成态可描述

架构文档显示 P6 已完成；§4.1 与 §4.2 / §4.3 之间**不再有矛盾**；
附录 C 的 `UnknownOutcome` 项**已关闭**；契约里没有残留的 `planned`；
`check-specs.sh` 会在「已完成的 Spec 还留着 `planned`」时**主动报错**。

### #7 无隐形依赖

- NEX-37 ~ NEX-45 均已登记在 `backlog/tasks/`
- `scripts/check-specs.sh` / `check-docs.sh` / `check-backlog.sh` **已**存在（**核对**而非新增）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：架构文档 §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 / §6.3 之间的
  **一致性**由本任务**核对**（漏改处补改）。
- **依据**：D12 已由**操作者裁决为选项 B**（2026-11-06）——
  修订走的是**条款**方向（改 §4.2 / §4.3 / §4.3.2 / §4.4），
  §4.1 的 crate 图**本就**与修订后一致。
  因此本任务**不再**是「消解矛盾」，而是「核对修订的**一致性**」。
  权威文档**自相矛盾**比「没有文档」更危险：读者按 §4.1 去理解依赖方向会得到**错误**的结论。
- **可逆性**：高 —— 文档修订。
- **影响面**：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` §4.1 / §4.2 / §4.3 / §4.3.2 / §4.4 / §5.3 / §6.3。
