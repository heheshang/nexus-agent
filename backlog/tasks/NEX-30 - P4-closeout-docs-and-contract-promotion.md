---
id: NEX-30
title: "P4-closeout-docs-and-contract-promotion"
status: To Do
assignee: []
labels: [docs, closeout, p4]
priority: medium
dependencies:
  - NEX-26
  - NEX-27
  - NEX-28
  - NEX-29
references:
  - "docs/specs/p4/spec.md §5、§6"
  - "docs/specs/p4/tasks.md T-005"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.4"
---

## Description

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

### 这个任务的**特殊职责**：契约升格

`docs/specs/p4/contracts/invariants.json` 里的不变量在 Spec 阶段是 `planned` ——
**证据尚未产生**（这正是 S2 必须先于 S4 的必然结果）。

本任务的职责是**把 `planned` 全部升格为 `verified`**，
并且**验证每个测试真的存在**。

`bash scripts/check-specs.sh` 的规则 3b 会在本任务归档后要求：
所有 `planned` 条目必须已升格 —— 否则门禁红。

**这不是「勾选框」** —— 升格前必须确认测试**真的存在**且**真的断言了那条不变量**。

### 为什么需要规则 3b

P3 暴露了一个顺序问题：P3 的 Spec 是**实现之后**才补的
（`9e620dd` 在 `4cd6c31` 之后），所以规则 3 一开始就能全绿 ——
**因为证据早就存在了**。

P4 走的是**正确的顺序**（S2 产出物先于 S4 实现），
所以规则 3 需要能表达「契约已声明、证据待产生」这个中间态，
并**在收口时强制它归零**。

**规则 3b 就是那个「强制归零」** —— 没有它，「planned」会变成一个可以永久停留的状态。

## 范围边界

### 做

- 架构文档 §5.4 / §6.1 的 P4 条目同步
- `contracts/invariants.json` 的 `planned` → `verified` 升格
- `nexus-agent` 的 crate 级文档
- `check-specs.sh` 规则 3b 的实现与**故障注入验证**

### 不做

| 项 | 理由 |
|---|---|
| 补写缺失的测试 | 那是 T-001~T-004 的职责；本任务**只核对**，发现缺失就**退回**对应任务 |
| 修改 P3 的契约 | P3 已交付，契约已定稿 |
| 实现后续任务 F1 / F2 | 属**独立**任务，且需人工授权后才创建 |

## Acceptance Criteria

- [ ] #1 架构文档 §5.4 不变量表已补 P4 已验证项（逐条对应 `contracts/invariants.json` 的 25 条）
- [ ] #2 架构文档 §6.1 的 P4 行已标记为完成，并写明完成判据的达成方式
- [ ] #3 `docs/specs/p4/contracts/invariants.json` 中所有 `planned` 条目**已升格**为 `verified`
- [ ] #4 **每个 `planned` 条目引用的测试都真的存在** —— `bash scripts/check-specs.sh` 规则 3b 通过
- [ ] #5 架构文档 §2.6.4.1 的业务码表未被改动（P4 不新增码值），有 diff 证据
- [ ] #6 架构文档 §2.2.4.1 的事件清单未被改动（P4 不新增事件名），有 diff 证据
- [ ] #7 `nexus-agent` 的 crate 级文档写明「负责什么 / 不负责什么」「trait 由本 crate 定义」「不新增依赖方向」
- [ ] #8 决策 D4~D7 的**反方论点与回应**已写入 Spec §6，且后续任务 F1 / F2 的**触发条件**明确
- [ ] #9 `scripts/check-specs.sh` 规则 3b 已实现，且**用注入故障验证过会红**（把一个 `planned` 留在已完成的 Spec 里 → 门禁红）
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #11 CI 全部门禁全绿

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

11 条 AC 全部可二元判定 —— 每条都指明「文档里有什么」或「命令的退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p4/spec.md §5、§6` —— 存在
- `docs/specs/p4/tasks.md T-005` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.4` —— 存在

### #4 范围有界

文档 + 一个门禁脚本改动。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | **修改** —— §5.4 / §6.1 |
| `docs/specs/p4/contracts/invariants.json` | **修改** —— status 升格 |
| `crates/nexus-agent/src/lib.rs` | **修改** —— crate 级文档 |
| `scripts/check-specs.sh` | **修改** —— 规则 3b |

### #6 完成态可描述

架构文档显示 P4 已完成；契约里没有残留的 `planned`；
`check-specs.sh` 会在「已完成的 Spec 还留着 `planned`」时**主动报错**。

### #7 无隐形依赖

- NEX-26 / NEX-27 / NEX-28 / NEX-29 均已登记在 `backlog/tasks/`
- `scripts/check-specs.sh` **已**存在（**修改**而非新增）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，**但需 NEX-26 ~ NEX-29 完成后开工**。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：`check-specs.sh` 增加规则 3b —— 已归档（完成）的 Spec 不得残留 `planned` 不变量。
- **依据**：没有这条规则，`planned` 会变成一个**可以永久停留**的状态 ——
  「先声明契约，证据永远不补」的漏洞比「没有契约」更坏，因为门禁**看起来**是绿的。
  P3 的 Spec 是实现**之后**补的，所以没暴露这个问题；P4 走正确顺序，必然暴露。
- **可逆性**：高 —— 规则是脚本里的一段判定。
- **影响面**：`scripts/check-specs.sh`。

### 类别：decide-and-proceed

- **决策**：本任务**只核对**契约升格，**不补写**缺失的测试。
- **依据**：按 `ai-sdlc-decision-authority.mdc` §5「不扩大范围」——
  收口任务若顺手补测试，就把「核对判据」变成了「重做一遍」，
  收口任务就**失去了它的意义**（那正是 T-005 描述里写明的）。
  发现缺失就**退回**对应任务。
- **可逆性**：不适用。

---

## 实现过程中的发现

（待补）
