---
id: NEX-35
title: "docs(api): P5 收口 —— 架构文档同步与契约升格"
status: To Do
assignee: []
labels: [docs, closeout, p5]
priority: medium
dependencies:
  - NEX-31
  - NEX-32
  - NEX-33
  - NEX-34
references:
  - "docs/specs/p5/spec.md §5、§6"
  - "docs/specs/p5/tasks.md T-005"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1"
---

## Description

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

### 这个任务的**特殊职责**：契约升格

`docs/specs/p5/contracts/invariants.json` 里的不变量在 Spec 阶段是 `planned` ——
**证据尚未产生**（这正是 S2 必须先于 S4 的必然结果）。

本任务的职责是**把 `planned` 全部升格为 `verified`**，
并且**验证每个测试真的存在**。

`bash scripts/check-specs.sh` 的规则 3b 会在本任务归档后要求：
所有 `planned` 条目必须已升格 —— 否则门禁红。

**这不是「勾选框」** —— 升格前必须确认测试**真的存在**且**真的断言了那条不变量**。

### 另一个**特殊职责**：故障注入

NEX-24 / NEX-25 的教训：**没验证过会红的断言，等于没有断言**。

本任务必须对 P5 的关键断言做**故障注入**，验证**确实会红**。

## 范围边界

### 做

- 架构文档 §6.1 的 P5 条目同步 + §5.4 的不变量表补 P5 已验证项
- `contracts/invariants.json` 的 `planned` → `verified` 升格
- `docs/operations/` 的 P5 运行说明（SSE 端点怎么访问 / 怎么解析）
- **故障注入**验证与记录

### 不做

| 项 | 理由 |
|---|---|
| 补写缺失的测试 | 那是 T-001~T-004 的职责；本任务**只核对**，发现缺失就**退回**对应任务 |
| 修改 P3/P4 的契约 | 已交付，契约已定稿 |
| 实现 F3（`FinishReason::Canceled`） | 需人工授权后才创建 |

## Acceptance Criteria

- [ ] #1 架构文档 §6.1 的 P5 行标记为**完成**，并附**可核验**的交付摘要（不变量条数、测试数）
- [ ] #2 `docs/specs/p5/contracts/invariants.json` 的全部不变量 `status` 提升为 `verified`
- [ ] #3 `bash scripts/check-specs.sh` 通过（规则 3b 要求：任务全部归档后不得残留 `planned`）
- [ ] #4 **故障注入**：对 P5 的关键断言做注入，验证**确实会红** —— 至少覆盖：帧格式、分隔符残留、事件名映射、cause 泄露
- [ ] #5 故障注入的记录写进任务文件（含「注入什么 / 期望 / 实际」）
- [ ] #6 `docs/operations/` 有 P5 的运行说明（`/metrics` 已在 P3 runbook；SSE 端点需补「怎么访问 / 怎么解析」）
- [ ] #7 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #8 CI 全部门禁全绿

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

8 条 AC 全部可二元判定 —— 每条都指明「文档里有什么」或「命令的退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §5、§6` —— 存在
- `docs/specs/p5/tasks.md T-005` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1` —— 存在

### #4 范围有界

文档 + 一个门禁脚本改动。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | **修改** —— §5.4 / §6.1 |
| `docs/specs/p5/contracts/invariants.json` | **修改** —— status 升格 |
| `docs/operations/` 下的运行说明 | **修改/新增** |
| `backlog/completed/NEX-35 - ...md` | **新增** —— 归档 |

### #6 完成态可描述

架构文档显示 P5 已完成；契约里没有残留的 `planned`；
`check-specs.sh` 会在「已完成的 Spec 还留着 `planned`」时**主动报错**。

### #7 无隐形依赖

- NEX-31 ~ NEX-34 均已登记在 `backlog/tasks/`
- `scripts/check-specs.sh` **已**存在（**修改**而非新增）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-31 ~ NEX-34 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：故障注入的**最小覆盖面**是「帧格式、分隔符残留、事件名映射、cause 泄露」四类。
- **依据**：这四类分别对应 P5 的四类**静默失效**形态 ——
  帧格式错与事件名错会让前端**收不到事件**，
  分隔符残留会让服务端**死循环**，
  cause 泄露会让**内部细节**外泄。
  其余断言（如缓冲字节数 > 0）由「任意切分点」这类**更强**的测试**间接覆盖**。
- **可逆性**：不适用。
- **影响面**：仅验证行为，不改实现。
