---
id: NEX-22
title: "docs(ai-sdlc): 补齐 AI-SDLC 文档层"
status: In Progress
assignee: []
labels: [ai-sdlc, governance, docs]
priority: medium
dependencies:
  - NEX-21
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.3"
  - "docs/operations/ai-sdlc-workflow-handbook.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/spec.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/policy.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/autonomy.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/agents.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/adapters.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/metrics.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/glossary.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0011-definition-of-ready-gate.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0035-decision-catalog-operator-routing.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0046-attested-reviewer-independence.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0052-staged-review-pipeline.md"
---

## Description

NEX-21 把 AI-SDLC 的**规范**补齐到了 `.cursor/rules/`（判据层），但**文档层**仍有缺口。

### 缺口是什么

上游 AI-SDLC 插件仓库（`~/.claude/plugins/marketplaces/ai-sdlc/`）的规范面是：

- `spec/`：9 份规范性文档（`spec.md` / `policy.md` / `autonomy.md` / `agents.md` / `adapters.md` / `metrics.md` / `glossary.md` / `primer.md` / `design-intent.md`）
- `spec/rfcs/`：60+ 份 RFC
- `docs/concepts/`：23 篇概念解释

本仓库**只**落了其中一部分条款，**没有任何一份文档说明**：

1. 上游哪些条款已被本仓库适配、落在哪个载体上；
2. 哪些条款**有意不适用**（例如产品侧 Soul / Variant / Journey / PPA 优先级算法 —— 本仓库是 Rust 后端，不是 AI-SDLC 框架的采纳者）；
3. 哪些条款**待接入**（例如 staged review pipeline、reviewer independence 三层 attestation —— 需要 `ai-sdlc` 管道才成立）。

没有这份矩阵，读者只能**逐条翻规则去猜**覆盖情况 —— 这正是 NEX-21 §「实现过程中的发现」§1 说的「看起来覆盖了，实际没有」的同一类静默失效。

### 同时缺的还有

| 缺口 | 后果 |
|---|---|
| `docs/` 没有总索引 | 读者不知道有哪些文档、从哪读起 |
| 没有 DoR 手册 | 门禁判据在规则里，但**怎么逐项执行、怎么写判定记录**只有规则没有示例 |
| 没有评审手册 | 三类 remit、评审者独立性、迭代上限、provenance 的**操作步骤**散在规则里 |
| 没有度量手册 | 五类指标只列了维度，**口径与分母**没定义 |
| 没有适配器清单 | 外部系统边界只给了契约表，没给**本仓库实际接了哪些**、凭据怎么走、失败怎么翻译 |
| 没有术语表 | 上游术语（enforcement level / handoff contract / conformance level…）在规则里直接用英文，中文读者需要自己猜 |
| `NEX-21` 已提交但任务仍停在 `tasks/` 且 `status: In Progress` | backlog 与 git 历史不一致 —— 违反 `ai-sdlc-artifacts.mdc` §6 的归档纪律 |

**目标**：把文档层补齐到与规则层**对称** —— 判据在 `.cursor/rules/`，解释与覆盖情况在 `docs/`，
两者不矛盾、互相可解析。

### 与 NEX-21 的关系

NEX-21 交付的是**判据**（每条规则的阶段、产出物、退出条件）。
本任务交付的是**索引与覆盖说明**（哪些判据从哪来、哪些上游条款不落地、怎么走一遍）。

**本任务不改任何规则文件的判据** —— 只新增文档 + 补一条文档链接门禁 + 归档 NEX-21。

## DoR 判定

- 门禁 1（AC 可二元测试）：通过 —— 14 条 AC，均可写成「文件存在 / 命令通过 / 断言成立」
- 门禁 2（无未决标记）：通过
- 门禁 3（引用可解析）：通过 —— 引用的上游规范文件均在本机插件仓库存在；
  本仓库路径由 `scripts/check-rules.sh` 与新增的 `scripts/check-docs.sh` 自动校验
- 门禁 4（范围有界）：通过 —— 只改 `docs/`、`scripts/`、`.github/workflows/ci.yml`、`AGENTS.md`、
  `.cursor/rules/` 的「相关文件」表格、`backlog/`，不含 Rust 代码
- 门禁 5（受影响面被指名）：通过 —— `docs/README.md`、`docs/ai-sdlc/*.md`、`scripts/check-docs.sh`、
  `.github/workflows/ci.yml`、`backlog/completed/NEX-21 - chore(rules) 补齐 AI-SDLC 全流程规范.md`
- 门禁 6（完成态可描述）：通过 —— 读者能通过 `docs/README.md` 找到 AI-SDLC 的全部文档，
  并能通过覆盖矩阵判断某条上游条款在本仓库是否落地
- 门禁 7（无隐形依赖）：通过 —— NEX-21 已完成（`backlog/completed/`）

**结论：pass**

## Acceptance Criteria

- [x] #1 `docs/README.md` 存在，索引 `docs/` 下**全部**文档，按类别分组，
      每个条目有**一句话作用说明**与相对链接
- [x] #2 `docs/ai-sdlc/coverage-matrix.md` 存在，逐条列出上游 `spec/` 9 份文档、
      `docs/concepts/` 23 篇、`spec/rfcs/` 全部 RFC 的**适配状态**
      （已适配 / 部分适配 / 有意不适用 / 待接入），每条给出**本仓库载体路径**或**不适用理由**
- [x] #3 覆盖矩阵中每条「已适配」项指向的路径**真实存在**，由 `scripts/check-docs.sh` 校验
- [x] #4 覆盖矩阵显式列出**有意不适用的上游条款**及其理由
      （产品侧 Soul / Variant / Journey / PPA / 设计系统合规等与本仓库无关的部分），
      不留下「未提及」的空白
- [x] #5 `docs/ai-sdlc/glossary.md` 存在，收录本仓库规则与文档中出现的
      AI-SDLC 专有术语，给出**中文释义 + 上游出处 + 本仓库载体**三列
- [x] #6 `docs/ai-sdlc/dor-playbook.md` 存在：给出 DoR 七项的**逐项执行步骤**、
      Stage A 的**可执行检查清单**、判定记录模板、澄清回路的操作步骤
- [x] #7 `docs/ai-sdlc/review-playbook.md` 存在：给出三类 remit 的**检查项**、
      评审者独立性判据、迭代上限的操作流程、评审结论模板、provenance 标注示例
- [x] #8 `docs/ai-sdlc/metrics-playbook.md` 存在：五类指标**每个都写明分母与口径**，
      给出指标快照模板与校准流程（含「校准产物是规则修订而非单次豁免」的约束）
- [x] #9 `docs/ai-sdlc/adapter-inventory.md` 存在：列出本仓库**实际接触的外部系统**
      （PostgreSQL / Redis / S3 / Milvus / 模型供应商 / GitHub），
      每项给出契约类型、凭据来源（`NX_` 映射）、失败翻译规则、当前状态
- [x] #10 `scripts/check-docs.sh` 存在并**被验证过会红**（注入故障的负向测试有记录），
      校验：`docs/` 内相对链接可解析、`docs/README.md` 索引覆盖全部文档
- [x] #11 `scripts/check-docs.sh` 被接入 `.github/workflows/ci.yml`
- [x] #12 `AGENTS.md` 与 `.cursor/rules/ai-sdlc-workflow.mdc` 的「相关文件」表格
      指向新增文档；规则文件的**阶段 / 产出物 / 退出条件判据未被改动** ——
      唯一改动是**门禁清单的同步**（新增 `check-docs.sh` 一道，
      连带「六道门禁」→「七道门禁」的全部枚举点），已在「实现过程中的发现」§3 列全
- [x] #13 `backlog/completed/NEX-21 - chore(rules) 补齐 AI-SDLC 全流程规范.md` 存在，
      其 frontmatter `status` 为完成态；`backlog/tasks/` 下**不再有** NEX-21
- [x] #14 `bash scripts/check-rules.sh`、`bash scripts/check-docs.sh`、`bash scripts/check-backlog.sh`、
      `bash scripts/check-specs.sh` 全通过；`cargo fmt --all --check`、
      `cargo clippy --workspace --all-targets --locked -- -D warnings`、
      `cargo test --workspace --locked` 通过（本次不含 Rust 代码，应零影响但仍须验证）

## 备注

- 本任务**不改**任何 Rust 代码，不改错误码 / 事件名 / ID 类型，不改 crate 边界。
- 本任务**不改**架构文档的已定稿条款（§6.3）—— 只补流程侧文档。
- 覆盖矩阵里的「有意不适用」判断属于**适配决策**，不是上游条款的否定。
  本仓库是 **AI-SDLC 的采纳者**（用它的流程规范），不是它的**实现者**
  （不实现 Pipeline / AgentRole / QualityGate 等资源类型）——
  这一条判断是整份矩阵的**前提**，写在矩阵开头。
- 「全流程规范」的**判据**在 NEX-21 交付的 `.cursor/rules/`；
  本任务交付的是**导航与覆盖说明**，两者**不重复**：
  规则里不写「哪些上游条款被采用」，文档里不写「判据是什么」。
- **一处必要的规则文件改动**：新增一道门禁会让「门禁条数」这个**事实**变化，
  所有枚举它的地方必须同步 —— 否则规则与仓库状态漂移。
  改动只涉及**清单条目与计数**，不涉及任何阶段判据。

## 实现过程中的发现

### 1. 「上游路径」与「本仓库路径」在同一个反引号里，是一类**必然复发**的歧义

覆盖矩阵里既要引用上游的 `spec/adapters.md`、又要引用本仓库的
`docs/ai-sdlc/adapter-inventory.md`。两者都写成反引号包路径，
**门禁无法区分** —— `check-docs.sh` 的第 3 项检查报出：

```
✗ coverage-matrix.md：引用了不存在的路径 'docs/concepts/'
```

这不是门禁的误报，是**文档的歧义**被门禁抓出来了。

修法有两条，**都用了**：

1. **文档侧**：上游路径一律加「上游」限定（`上游 docs/concepts/`），
   或改写成不与本仓库路径同形（`上游概念文档 two-tier-pipeline.md`）。
2. **门禁侧**：门禁只校验**本仓库路径** —— 上游路径不在管辖范围，
   因为上游目录**不在本仓库里**，校验它等于要求本机装了那个插件。

> 教训：**门禁抓到的「误报」要先判断是门禁错了还是文档错了。**
> 这次的答案是「文档错了」—— 因为读者也会困惑。

### 2. 门禁的**索引完整性**比链接检查更能防静默失效

`check-docs.sh` 的第 2 项断言 `docs/` 下每个 `.md` **都出现在 `docs/README.md` 里**。

第 1 项（链接可解析）只能防「链接指错了」，
**防不住「文档写了但没人找得到」** —— 后者恰恰是文档层最常见的失效：
一份没人索引的文档，等于没写。

注入故障 2（造一个未登记的 `.md`）验证了这条断言会红。

### 3. 「七道门禁」不是加一道脚本那么简单

新增 `check-docs.sh` 后，**所有枚举门禁条数的地方都要跟着改**：

| 位置 | 改动 |
|---|---|
| `.cursor/rules/ai-sdlc-quality-gates.mdc` §3 | 清单从 10 条变 11 条，第 9 条的注释变成第 10 条 |
| `.cursor/rules/ai-sdlc-workflow.mdc` §2 | S4 的退出条件「六道门禁」→「七道门禁」 |
| `.cursor/rules/ai-sdlc-implementation.mdc` §6 | 提交纪律的命令清单 |
| `.cursor/rules/ai-sdlc-agents.mdc` §1 | Developer 角色的 Goal |
| `AGENTS.md` §4 | 提交前门禁清单 + §1 阶段表 |
| `docs/operations/ai-sdlc-workflow-handbook.md` | 自检表 + 门禁分层表 |
| `scripts/dev.sh` | `audit` 命令 |

**漏改任何一处，就留下一处「数字对不上」的漂移** ——
而数字漂移比路径漂移更难发现，因为没有人会去数。

### 4. 门禁的负向测试是**必需项**，不是加分项

`scripts/check-docs.sh` 写完后做了四个注入故障的负向测试：

| 注入的故障 | 期望 | 实际 |
|---|---|---|
| 文档里写一个断链 `](does-not-exist.md)` | 红 | ✓ 红，报出文件与目标路径 |
| 造一个未登记进索引的 `.md` | 红 | ✓ 红，指出「未出现在 docs/README.md 的索引中」 |
| 索引里列一个不存在的文档 | 红 | ✓ 红，指出「索引列出了不存在的文档」 |
| 覆盖矩阵引用不存在的仓库路径 | 红 | ✓ 红，报出文件名与路径 |

**没验证过会红的门禁，等于没有门禁** —— 沿用 NEX-14 与 NEX-21 的教训。

### 5. `git mv` 归档时，`status` 要**同时**改

NEX-21 已提交（`9643972`）但任务仍停在 `backlog/tasks/` 且 `status: In Progress` ——
`check-backlog.sh` 的第 2 项只检查 `completed/` 下的任务，
**查不出「任务做完了却没归档」**。

这是门禁的盲区：**它防「归档了但没标完成」，不防「做完了但没归档」**。
后者靠提交纪律（合入 `main` 的同一提交内 `git mv`）。

> 教训：**门禁的检查项与它声称防的失效模式要一一对齐**。
> 声称防 A 实际防 B，比没防更危险。
> 「做完了没归档」这一条**目前靠人工**，已登记为后续可补的门禁项。

