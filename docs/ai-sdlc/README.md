# AI-SDLC 文档导航

> 本目录是 AI-SDLC 在本仓库的**文档层**。
> **判据在 `.cursor/rules/`** —— 本目录不重复定义判据，只讲「怎么走一遍」与「覆盖了什么」。

---

## 从哪读起

| 你是 | 先读 |
|---|---|
| **第一次接触本仓库的流程** | `../operations/ai-sdlc-workflow-handbook.md`（流程手册，叙事版） |
| **准备开工，要过 DoR** | `dor-playbook.md`（DoR 操作手册） |
| **要提交评审** | `review-playbook.md`（评审操作手册） |
| **想知道上游哪条规范落地了** | `coverage-matrix.md`（覆盖矩阵） |
| **遇到不认识的术语** | `glossary.md`（术语表） |
| **接外部系统 / 配凭据** | `adapter-inventory.md`（适配器清单） |
| **做周期复盘 / 调流程参数** | `metrics-playbook.md`（度量与校准手册） |
| **看本周期实际测出来的数** | `metrics-snapshot-2026-11.md`（度量快照） |

---

## 文档清单

| 文档 | 一句话作用 |
|---|---|
| [`coverage-matrix.md`](coverage-matrix.md) | 上游 AI-SDLC 的 9 份规范 + 20 篇概念 + 57 份 RFC，逐条给出**适配状态**与本仓库载体 |
| [`glossary.md`](glossary.md) | 本仓库用到的 AI-SDLC 术语：中文释义 + 上游出处 + 本仓库载体 |
| [`dor-playbook.md`](dor-playbook.md) | DoR 七项的逐项执行步骤、Stage A 可执行检查、判定记录模板、澄清回路 |
| [`review-playbook.md`](review-playbook.md) | 三类 remit 的检查项、评审者独立性、迭代上限、结论模板、provenance 示例 |
| [`metrics-playbook.md`](metrics-playbook.md) | 五类指标的**分母与口径**、快照模板、校准流程、审计留痕 |
| [`adapter-inventory.md`](adapter-inventory.md) | 本仓库实际接触的外部系统、凭据来源、失败翻译、未接入项 |
| [`metrics-snapshot-2026-11.md`](metrics-snapshot-2026-11.md) | **度量快照** · 2026-11 —— 五类指标的实际口径、观察、判断与已登记的待校准项 |

---

## 与其他文档的关系

```
.cursor/rules/ai-sdlc-*.mdc     ← 判据（每条规则的阶段、产出物、退出条件、失败处理）
        ↑ 被引用
docs/ai-sdlc/*.md                ← 本目录：导航、操作步骤、覆盖情况
        ↑ 被引用
docs/operations/*.md             ← 运行说明（配置、阶段计划）
docs/specs/**, docs/rfcs/**      ← 产出物本身（Tier 2 / Tier 1）
backlog/**                       ← Tier 3 产出物
```

**冲突时以 `.cursor/rules/` 为准**（见 `AGENTS.md` §0 权威顺序）。

---

## 三条读法纪律

1. **规则与文档不重复**。
   规则里不写「哪些上游条款被采用」，文档里不写「判据是什么」。
   重复 = 两处漂移的起点。

2. **文档里的路径必须真实存在**。
   由 `bash scripts/check-docs.sh` 校验 —— 与 `scripts/check-rules.sh` 同一条道理：
   **指向具体产物的引用必须被机器校验存在性**。

3. **空白即缺陷**。
   覆盖矩阵里没有状态的上游条款，与不存在的路径一样是**静默失效**。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `../README.md` | `docs/` 总索引 |
| `../operations/ai-sdlc-workflow-handbook.md` | 流程手册（叙事版） |
| `../specs/README.md` | Spec 层（Tier 2）产物说明 |
| `../rfcs/README.md` | RFC 层（Tier 1）产物说明 |
| `../../.cursor/rules/ai-sdlc-workflow.mdc` | **判据的入口** |
