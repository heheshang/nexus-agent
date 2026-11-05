# docs · 文档总索引

本目录是 NexusAgent 的**文档层**。按「谁写、谁读、多久改一次」分四类。

| 类别 | 目录 | 读者 | 变更频率 |
|---|---|---|---|
| **架构权威** | `architecture/` | 所有人 | 低（已定稿条款不轻易改） |
| **流程规范** | `ai-sdlc/`、`operations/` | 代理 + 维护者 | 中 |
| **产出物** | `specs/`、`rfcs/` | 评审者 + 接手者 | 高（随任务推进） |
| **运行说明** | `operations/` | 运维 + 维护者 | 中 |

---

## 1. 架构权威

> `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` 是
> **错误码表、事件清单、ID 类型表、crate 边界的唯一权威**。
> 冲突时以它为准（`AGENTS.md` §0 权威顺序第 1 位）。

| 文档 | 一句话作用 |
|---|---|
| [`architecture/2026-11-04-nexusagent-rust-architecture-design.md`](architecture/2026-11-04-nexusagent-rust-architecture-design.md) | **唯一权威** —— 架构与契约的定稿条款（错误码、事件、ID 类型、crate 边界、验证分层） |
| [`architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md`](architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md) | 参考实现（Go/Eino）到 Rust 的**迁移对照** —— 差异与理由 |
| [`architecture/2026-11-03-nexusagent-eino-reference-architecture.md`](architecture/2026-11-03-nexusagent-eino-reference-architecture.md) | 面向 `nexus-lab` 重建的参考架构说明（**历史文档**，定位已变更） |

---

## 2. 流程规范

> **判据在 `.cursor/rules/`**，本节是**导航与操作说明**。两者不重复、不矛盾。

| 文档 | 一句话作用 |
|---|---|
| [`operations/ai-sdlc-workflow-handbook.md`](operations/ai-sdlc-workflow-handbook.md) | AI-SDLC 流程手册（叙事版）—— 九个阶段在防什么、决策者在哪里 |
| [`ai-sdlc/README.md`](ai-sdlc/README.md) | AI-SDLC 文档导航 —— 从哪读起 |
| [`ai-sdlc/coverage-matrix.md`](ai-sdlc/coverage-matrix.md) | 上游 AI-SDLC 规范（9 spec + 20 concepts + 57 RFC）的逐条适配状态 |
| [`ai-sdlc/glossary.md`](ai-sdlc/glossary.md) | 本仓库用到的 AI-SDLC 术语表（中文释义 + 上游出处 + 本仓库载体） |
| [`ai-sdlc/dor-playbook.md`](ai-sdlc/dor-playbook.md) | DoR 门禁的执行步骤、Stage A 检查清单、判定记录模板、澄清回路 |
| [`ai-sdlc/review-playbook.md`](ai-sdlc/review-playbook.md) | 三类 remit 检查项、评审者独立性、迭代上限、评审结论模板 |
| [`ai-sdlc/metrics-playbook.md`](ai-sdlc/metrics-playbook.md) | 五类指标的分母与口径、快照模板、校准流程、审计留痕 |
| [`ai-sdlc/metrics-snapshot-2026-11.md`](ai-sdlc/metrics-snapshot-2026-11.md) | 度量快照 · 2026-11 —— 五类指标的实际口径与待校准项 |
| [`ai-sdlc/adapter-inventory.md`](ai-sdlc/adapter-inventory.md) | 外部系统清单、凭据来源、失败翻译规则、未接入项 |

---

## 3. 产出物

| 文档 | 一句话作用 |
|---|---|
| [`specs/README.md`](specs/README.md) | **Tier 2 · Spec 层**产物的结构与门禁说明 |
| [`specs/p3/spec.md`](specs/p3/spec.md) | P3 LLM 网关的契约（范围、红线、核心契约、已定稿决策） |
| [`specs/p3/tasks.md`](specs/p3/tasks.md) | P3 的任务清单（`T-NNN` ↔ `NEX-nn` 一一对应） |
| [`specs/p3/contracts/invariants.json`](specs/p3/contracts/invariants.json) | P3 的机器可读不变量（每条指向一个**真实存在的测试**） |
| [`specs/p3/contracts/error-codes.json`](specs/p3/contracts/error-codes.json) | P3 允许触碰的业务码白名单 |
| [`rfcs/README.md`](rfcs/README.md) | **Tier 1 · RFC 层**产物的结构与 OQ 禁令 |

---

## 4. 运行说明

| 文档 | 一句话作用 |
|---|---|
| [`operations/configuration.md`](operations/configuration.md) | 配置三层来源、`NX_` 环境变量映射表全表 |
| [`operations/p1-plan.md`](operations/p1-plan.md) | P1 阶段（核心类型）的计划与红线 |
| [`operations/p2-plan.md`](operations/p2-plan.md) | P2 阶段（基础设施）的计划与红线 |
| [`operations/p3-plan.md`](operations/p3-plan.md) | P3 阶段（LLM 网关）的计划与红线 |
| [`operations/p3-runbook.md`](operations/p3-runbook.md) | P3 阶段（LLM 网关）的运行说明 —— 供应商配置、凭据、熔断单副本粒度、可观测与回滚 |

---

## 5. 文档之外

| 位置 | 内容 |
|---|---|
| `AGENTS.md` | 代理常驻指令入口 + 权威顺序 |
| `.cursor/rules/` | **流程判据**（阶段、产出物、退出条件、失败处理） |
| `backlog/` | Tier 3 产出物（任务载体），格式见 `backlog/README.md` |
| `scripts/check-*.sh` | 组织纪律门禁（CI 强制） |

---

## 6. 文档纪律

### 6.1 链接必须可解析

`docs/` 内的相对链接由 `bash scripts/check-docs.sh` 校验存在性。
**路径失效 = 门禁红** —— 与规则文件引用路径的校验是同一条道理。

### 6.2 本索引必须完整

`check-docs.sh` 会断言 `docs/` 下的每个 `.md` **都出现在本索引里**。
新增文档不登记 → 门禁红。这防止「文档写了但没人找得到」。

### 6.3 spec 与 status 分离

| | 含义 |
|---|---|
| **spec** | 期望状态（人写，系统读）—— 架构文档、任务文件的 AC |
| **status** | 观测状态（系统写，人读）—— 代码现状、任务状态、CI 结果 |

任务文件里的 `## Acceptance Criteria` 是**期望**，勾选框是**观测** ——
**只在实际验证通过后才勾选**。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `AGENTS.md` | 权威顺序与常驻指令 |
| `../backlog/README.md` | 任务载体格式与生命周期 |
| `../.cursor/rules/ai-sdlc-workflow.mdc` | 流程总纲（判据入口） |
