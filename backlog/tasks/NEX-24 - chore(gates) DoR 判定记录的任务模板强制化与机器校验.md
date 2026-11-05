---
id: NEX-24
title: "chore(gates): DoR 判定记录的任务模板强制化与机器校验"
status: To Do
assignee: []
labels: [ai-sdlc, governance, gates]
priority: medium
dependencies:
  - NEX-22
references:
  - "docs/ai-sdlc/metrics-snapshot-2026-11.md §六"
  - "docs/ai-sdlc/metrics-playbook.md §1.1"
  - "docs/ai-sdlc/dor-playbook.md"
  - ".cursor/rules/ai-sdlc-dor-gate.mdc"
  - "backlog/README.md"
---

## Description

**S9（度量与校准）** 快照发现的第一条流程缺口。

### 观察到的（事实）

`docs/ai-sdlc/metrics-snapshot-2026-11.md` 的「任务有效性」表里：

> DoR 判定记录覆盖率 **5/22** —— 早期 18 个任务没有 `## DoR 判定` 段落。

21 个已归档任务里，只有 **NEX-19 / NEX-20 / NEX-21 / NEX-22 / NEX-23**
五个有 `## DoR 判定` 段落。

### 为什么这是**缺口**而不是「正常」

`ai-sdlc-dor-gate.mdc` §5 要求「**判定记录必须落到任务文件里**，
否则下次接手的人无法复核」。

而 `ai-sdlc-observability.mdc` §1 的「任务有效性」维度要求
「DoR 通过率」有明确的**分母**。

**分母现在是不确定的** —— 因为「有记录的任务」和「全部任务」不是同一个集合。
这导致「DoR 通过率 100%」这个数字**没有意义**：
它只覆盖了最近 5 个任务，而那 5 个恰好是**流程已经成熟之后**做的。

> 「看起来覆盖了，实际没有」的**又一次** —— 只是这次在**流程层**而不是代码层。

### 早期任务为什么没有

那 18 个任务（NEX-2..NEX-18）完成于「DoR 判定记录必须落盘」这条要求写进规则**之前**。
所以它们**不是违规** —— 它们是**规则生效前**的历史。

**因此本任务不追溯补写那 18 份记录** ——
补写是**伪造审计条目**（`ai-sdlc-observability.mdc` §3.3：
「审计条目一旦写入即不可变。更正通过新增条目完成」，
而这里根本没有原始条目可更正）。

### 本任务要做什么

让**未来**的任务**不可能**漏掉判定记录：

1. `backlog/README.md` 的任务模板把 `## DoR 判定` 段落列为**必填**；
2. `scripts/check-backlog.sh` 增加一项检查：`tasks/` 与 `completed/` 下的任务
   **必须**包含 `## DoR 判定` 段落；
3. 门禁**必须被验证过会红**（注入一个缺段落的任务，看它是否报出来）。

## 范围边界

### 做

- `backlog/README.md` 模板更新
- `scripts/check-backlog.sh` 增加检查项
- 该检查项的**负向测试**（注入故障验证会红）

### 不做

| 项 | 理由 |
|---|---|
| 追溯补写早期 18 个任务的 DoR 判定 | 那是**伪造审计条目** |
| 修改 DoR 的**七项判据**本身 | 判据没失效 —— 失效的是**记录载体** |
| 引入 DoR 门禁的严重度变更 | `ai-sdlc-observability.mdc` §2：严重度调整**需 RFC 修订**，不是本任务 |

## Acceptance Criteria

- [ ] #1 `backlog/README.md` 的任务模板把 `## DoR 判定` 标为**必填段落**，并写明缺失的后果
- [ ] #2 `scripts/check-backlog.sh` 新增检查项：`tasks/` 与 `completed/` 下的任务文件**必须**含 `## DoR 判定` 段落
- [ ] #3 该检查项的**负向测试**有记录 —— 注入一个缺段落的临时任务文件，验证门禁**报出**该文件名与缺失的段落
- [ ] #4 负向测试后**临时文件被清理**（不留在仓库里）
- [ ] #5 `scripts/check-backlog.sh` 的输出文案说明「为什么」要这条检查（不只是「检查什么」）
- [ ] #6 `AGENTS.md` §4 的提交前门禁清单**不需要改**（脚本名未变）—— 若有编号引用则同步更新
- [ ] #7 `bash scripts/check-backlog.sh` 通过；`cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #8 CI 全部门禁全绿

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

8 条 AC 全部可二元判定 —— 每条都指明「脚本输出什么」或「文件含某段」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。
「是否追溯补写早期任务」是**显式排除**（范围边界表），不是未决问题。

### #3 引用可解析

- `docs/ai-sdlc/metrics-snapshot-2026-11.md §六` —— 存在
- `docs/ai-sdlc/metrics-playbook.md §1.1` —— 存在
- `docs/ai-sdlc/dor-playbook.md` —— 存在
- `.cursor/rules/ai-sdlc-dor-gate.mdc` —— 存在
- `backlog/README.md` —— 存在

### #4 范围有界

「做」3 项 / 「不做」3 项。
**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、DoR 判据本身。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `backlog/README.md` | 模板说明 |
| `scripts/check-backlog.sh` | **新增一项检查** |
| `docs/ai-sdlc/metrics-snapshot-2026-11.md` | 可能加一句「已登记任务」的标注 |

**不改**任何 Rust 代码 —— 这是纯流程/脚本层改动。

### #6 完成态可描述

- `check-backlog.sh` 有一条新检查，**注入故障时它会红**；
- 全部任务文件都含 `## DoR 判定` 段落；
- 全部脚本测试绿。

### #7 无隐形依赖

- 只用 bash 与 `grep` —— `check-backlog.sh` 已经在用这些；
- 不新增依赖、不改 `Cargo.toml`。

### 结论

**pass** —— 七项全过，可进入实现（S4）。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：**不追溯补写**早期 18 个任务的 DoR 判定记录。
- **依据**：`ai-sdlc-observability.mdc` §3.3「审计条目一旦写入即不可变。
  更正通过新增条目完成，不修改旧条目」。而这 18 个任务**根本没有**条目 ——
  补写等于**伪造从未发生过的审计记录**。
  那会让审计链**看起来完整**而实际是虚构的 —— 比缺失更危险。
- **可逆性**：不可逆（伪造的条目无法被识别为伪造）。
  所以**不做**。
- **影响面**：`docs/ai-sdlc/metrics-snapshot-2026-11.md` 的「DoR 分母」说明
  改为「**可判定的 5 个**」并**显式说明**为什么不是 22 个。

### 类别：decide-and-proceed

- **决策**：用**门禁检查**（机器校验）而不是**规则措辞**来强制记录。
- **依据**：`ai-sdlc-decision-authority.mdc` §5「不静默偏离」与
  NEX-22 的教训 #4「**没验证过会红的门禁，等于没有门禁**」。
  规则措辞（「必须」）在本仓库已经被验证过会**被绕过** ——
  早期 18 个任务就是证据。
- **可逆性**：纯脚本改动，`git revert` 即可。
- **影响面**：只影响 `scripts/check-backlog.sh`。

---

## 实现过程中的发现

（待补）
