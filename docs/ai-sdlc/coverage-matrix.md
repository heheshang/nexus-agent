# AI-SDLC 上游条款覆盖矩阵

> **本矩阵回答一个问题**：上游 AI-SDLC 的哪条规范，在本仓库**落了地**、落在**哪里**；
> 哪条**有意不适用**、为什么；哪条**待接入**、需要什么前提。

上游来源：`~/.claude/plugins/marketplaces/ai-sdlc/`
（`spec/` 规范性文档、上游 concepts 概念解释、`spec/rfcs/` RFC）——
**下文所有上游路径均相对于该目录**，不是本仓库路径。
上游版本：`ai-sdlc` 插件 `0.40.0`（`~/.claude/plugins/cache/ai-sdlc/ai-sdlc/0.40.0`）。

---

## 0. 前提：本仓库是 AI-SDLC 的**采纳者**，不是**实现者**

这条判断是整份矩阵的前提，**先说清楚，后面所有「不适用」都由此推出**。

| | AI-SDLC 框架本身 | 本仓库（NexusAgent） |
|---|---|---|
| 身份 | 框架的**实现者** —— 提供 `Pipeline` / `AgentRole` / `QualityGate` / `AutonomyPolicy` 等资源类型与执行管道 | 框架的**采纳者** —— 借用它的**流程规范与门禁理念**，用**自己的载体**落地 |
| 交付物 | `ai-sdlc` CLI、`pipeline-cli`、`orchestrator`、MCP server | Rust 后端服务 + 一套 `.cursor/rules/` 判据 + CI 门禁脚本 |
| 管道 | `ai-sdlc init` 生成的 `.ai-sdlc/` 配置，Step 0→13 自动编排 | **未接入管道**。用「手写流程 + CI 门禁」等价实现 |
| 资源类型 | 声明式 YAML / JSON Schema 资源 | Markdown 判据 + bash 门禁脚本 |

**推论**：上游那些**描述框架自身如何实现**的条款（资源 schema、编排器、CLI 行为）在本仓库
**不适用**；那些**描述工作该怎么被管理**的条款（DoR、门禁分级、决策分类、provenance）
**适用**，且已在 `.cursor/rules/` 落地。

**这不是「选择性忽略」** —— 每一处「不适用」在本文档 §5 都有具名理由。

---

## 1. 状态定义

| 状态 | 含义 | 判据 |
|---|---|---|
| **已适配** | 条款语义完整落地，判据可执行 | 本文档 §2–§4 给出的载体路径**真实存在**，且承载了条款的核心要求 |
| **部分适配** | 核心语义落地，但有意裁剪或简化 | 载体存在；裁剪部分**显式写明**裁掉了什么、为什么 |
| **有意不适用** | 本仓库的产品形态不需要这条 | 给出**具名理由**（不是「用不上」这种空话） |
| **待接入** | 需要管道 / 基础设施才成立 | 写出**前置条件**与**若不接入的现状** |

**四态穷尽**：任何一条上游条款都必须落在其中一态。
「未提及」不允许 —— 空白本身就是静默失效（见 NEX-21 的教训）。

---

## 2. `spec/` 规范性文档（9 份）

| 上游文档 | 状态 | 本仓库载体 / 理由 |
|---|---|---|
| `spec/spec.md` | **已适配** | 核心资源模型 → `.cursor/rules/ai-sdlc-workflow.mdc`（S0–S9 阶段表）、`ai-sdlc-artifacts.mdc`（三层产出物）、`ai-sdlc-quality-gates.mdc`（门禁）、`ai-sdlc-autonomy.mdc`（自治）、`ai-sdlc-agents.mdc`（角色）、`ai-sdlc-adapters.mdc`（适配器）。**裁剪**：§5.1–§5.5 的**资源类型 schema** 不落地（本仓库不实现框架） |
| `spec/policy.md` | **已适配** | 三级执行等级、评估流水线、override 语义 → `ai-sdlc-quality-gates.mdc` §1/§2/§6。**裁剪**：§4 的声明式 YAML / Rego 表达式 → 本仓库用 **bash 门禁脚本**表达 |
| `spec/autonomy.md` | **已适配** | 四级自治、晋级降级、复杂度路由 → `ai-sdlc-autonomy.mdc` §1–§5。**裁剪**：晋级判据的**指标阈值**未量化（待 S9 度量校准，见 `metrics-playbook.md`） |
| `spec/agents.md` | **已适配** | 角色定义、编排模式、handoff 契约、审计轨迹 → `ai-sdlc-agents.mdc` §1–§6。**裁剪**：§5 五层记忆模型、§6 Agent Card / A2A 发现 → 本仓库**不用持久化代理记忆**，不用 A2A 做流程编排 |
| `spec/adapters.md` | **已适配** | 接口契约、凭据策略、配置与密钥、适配器注册 → `ai-sdlc-adapters.mdc` §2–§6 + `docs/ai-sdlc/adapter-inventory.md`。**裁剪**：§4–§5 的**注册表 / 发现机制**（`metadata.yaml`、registry、git ref）不落地 —— 本仓库适配器是**仓库内的 Rust crate**，靠 `scripts/check-deps.sh` 静态校验，不靠运行时注册 |
| `spec/metrics.md` | **部分适配** | 五类指标、provenance、审计留痕 → `ai-sdlc-observability.mdc` §1–§5。**裁剪**：§3 的 **OpenTelemetry 集成**（trace / metrics / logs 的具体 exporter）→ 本仓库目前**只落地流程侧的审计留痕**，服务侧 OTel 待接入（见 `metrics-playbook.md` §5） |
| `spec/glossary.md` | **已适配** | 术语 → `docs/ai-sdlc/glossary.md`。**裁剪**：只收录**本仓库实际用到**的术语 —— 上游术语表里 Soul / Variant / Journey / PPA 等产品侧词汇不在收录范围（理由见 §5） |
| `spec/primer.md` | **有意不适用** | 上游 primer 是**框架的入门读物**（介绍 Pipeline 资源、CLI 用法）。本仓库不是框架的采纳教程 —— 流程读物已由 `docs/operations/ai-sdlc-workflow-handbook.md` 承担 |
| `spec/design-intent.md` | **有意不适用** | 设计意图文档（Design Intent Document）是**前端设计系统**侧的产物格式。本仓库是 Rust 后端服务，无设计系统、无 Soul DID、无前端一致性义务 |

---

## 3. 上游 concepts 概念解释（20 篇 + 索引）

概念文档是**解释性**的 —— 它们解释「为什么这么设计」，不是「必须怎么做」。
本仓库只在**解释与已落地判据相关**时才需要它们。

| 上游文档 | 状态 | 说明 / 载体 |
|---|---|---|
| `index.md` | **有意不适用** | 上游概念索引，指向框架自身的概念集。本仓库的对应索引是 `docs/README.md` |
| `spec-driven.md` | **已适配** | 三层模型（RFC → Spec → Task）与「什么跳过哪层」的判据 → `ai-sdlc-artifacts.mdc` §1–§4、`docs/specs/README.md`。**落地差异**：本仓库 Tier 2 **不强制 `plan.md`**（顺序已由 `tasks.md` 的 `T-NNN` + `backlog/` 的 `dependencies` 表达），理由见 `ai-sdlc-artifacts.mdc` §4 |
| `dor-gate.md` | **已适配** | DoR 七项 rubric、Stage A/B、严重度 → `ai-sdlc-dor-gate.mdc` §2–§4，操作步骤见 `docs/ai-sdlc/dor-playbook.md`。**落地差异**：上游由 `cli-dor-check.mjs` 自动执行，本仓库**手工执行 + 判定记录落任务文件** |
| `two-tier-pipeline.md` | **部分适配** | Tier 1（会话内 slash command）→ 本仓库**等价实现**是 Cursor/Claude Code 会话内的代理工作；Tier 2（库调用 / API key 驱动）**不适用** —— 本仓库不提供程序化执行入口 |
| `staged-review.md` | **待接入** | 分阶段评审管道（判定层 → 强模型出计划 → 弱模型跑探针 → 强模型裁决）。**前置条件**：需要探针执行器与 judgment layer，依赖 AI-SDLC 管道。**现状**：本仓库用**三类并行评审**（`ai-sdlc-agents.mdc` §2），成本更高但流程等价。**若不接入**：评审成本高于上游，findings 的证据链靠人工引证而非探针产物 |
| `reviewer-independence.md` | **部分适配** | 独立性三层（`none` / `attested` / `isolated`）。本仓库落地**第一层**：评审者 ≠ 作者、AI 作者的改动需额外一名评审人（`ai-sdlc-quality-gates.mdc` §4.1）。**`attested` / `isolated` 待接入** —— 需要沙箱边界与 CI 独占签名密钥 |
| `proof-of-execution.md` | **部分适配** | 「评审确实发生过」→ 本仓库靠**不可变审计留痕**（CI 日志 + PR 评审记录 + 任务文件的 DoR 判定段）证明。**裁剪**：上游的 **Merkle 签名信封**不落地 —— 本仓库用 git 历史 + CI 产物作不可变凭据 |
| `judgment-layer.md` | **待接入** | 统一判定层（三种结果，其中一种是「什么都不做」）。**前置条件**：需要跨 harness 的判定服务。**现状**：本仓库的判定分散在各门禁脚本中 |
| `adopter-translators.md` | **有意不适用** | 「自带上游转换器」模式（把 Linear / Notion 的产出翻译成 backlog 任务）。本仓库的产出物**原生就是** backlog 任务格式，无翻译层需求 |
| `autonomous-orchestrator.md` | **待接入** | 自主编排器（按依赖就绪前沿自动挑任务）。**前置条件**：`ai-sdlc init` 接入管道。**现状**：任务调度由人 / 代理手动执行 |
| `cross-harness-review.md` | **待接入** | 跨 harness（Codex / Claude Code）评审的独立性规则。**前置条件**：需要多 harness 的执行与证据汇总基础设施。**现状**：评审在本会话内单 harness 完成 |
| `design-review-architecture.md` | **有意不适用** | 设计评审架构 —— 前端设计系统侧 |
| `operator-tui.md` | **有意不适用** | 框架自带的操作者 TUI。本仓库的操作界面是 **Cursor + CI + Markdown**，不引入终端 UI |
| `per-repo-governance.md` | **已适配** | 每仓库可配置治理 → 本仓库的治理载体是 `.cursor/rules/` + `scripts/check-*.sh` + `.github/workflows/ci.yml`。**落地差异**：上游是 `.ai-sdlc/` 下的声明式配置，本仓库是**规则文件 + 脚本**（判据可执行，且被 CI 强制） |
| `session-hierarchy.md` | **待接入** | 会话层级（三层）并行派发。**前置条件**：管道编排。**现状**：本仓库的并行只在**评审阶段**（三类评审并行），无会话层级 |
| `substrate-contract.md` | **有意不适用** | Substrate Contract 是 Soul DID 作用域的契约对象。**前提**：本仓库无 Soul DID 概念（见 §5） |
| `untrusted-contributor-verification.md` | **有意不适用** | 非受信贡献者 PR 的零信任验证（四阶段 UCVG）。**前提**：本仓库当前**无外部贡献者 PR 流程**，贡献者均在信任名单内。**若将来开放外部 PR**：这是必须引入的第一道门 |
| `usage-ledger-model-routing.md` | **部分适配** | 用量台账 + 按证据的模型路由。本仓库落地**经济效率**维度（`ai-sdlc-observability.mdc` §1）但**不记账 token 消耗**；模型路由由**配置决定**而非按证据动态切换。**前置条件**：可观测的 token 计量 |
| `variants.md` | **有意不适用** | Soul Variant 模式 —— 产品侧 |
| `zero-trust-untrusted-pr-verification.md` | **有意不适用** | 同 `untrusted-contributor-verification.md` |

---

## 4. `spec/rfcs/`（57 份）

### 4.1 直接落地为判据的 RFC

| RFC | 主题 | 状态 | 本仓库载体 |
|---|---|---|---|
| RFC-0011 | Definition-of-Ready 门禁 | **已适配** | `ai-sdlc-dor-gate.mdc`（七项、Stage A/B、严重度）+ `docs/ai-sdlc/dor-playbook.md` |
| RFC-0035 | 决策目录与操作者路由 | **已适配** | `ai-sdlc-decision-authority.mdc` §2（决策三分类）、§4（升级流程）—— 本仓库的核心条款 |
| RFC-0012 | 两层管道架构 | **部分适配** | Tier 1 等价实现（会话内代理）；Tier 2 不适用 —— 见上游概念文档 `two-tier-pipeline.md` 行 |
| RFC-0014 | 依赖图组合 | **部分适配** | 依赖表达 → `backlog/` 的 `dependencies` + `docs/specs/<feature>/tasks.md` 的 `T-NNN`。**裁剪**：无运行时依赖图编排器（无管道） |
| RFC-0016 | 估算校准（T-Shirt 尺寸） | **部分适配** | 本仓库用**复杂度分数 1–10** 代替 T-Shirt 尺寸（`ai-sdlc-autonomy.mdc` §2.1）。**裁剪**：分数与分数的映射表未量化（待度量校准） |
| RFC-0036 | spec-kit 桥接 | **已适配** | `docs/specs/<feature>/tasks.md` 采用 spec-kit `v0.8-headings` 布局（`docs/specs/README.md` §「`tasks.md` 的布局」）。**裁剪**：上游的 `import-spec` CLI 不落地 |
| RFC-0037 | 采纳者项目上下文继承 | **已适配** | `AGENTS.md` + `.cursor/rules/` —— 常驻上下文载体 |
| RFC-0038 | 采纳者自定义评审扩展点 | **已适配** | `ai-sdlc-agents.mdc` §1 的固定角色表（Developer / Code Reviewer / Test Reviewer / Security Reviewer / Refinement Reviewer） |
| RFC-0039 | 采纳者自定义门禁扩展点 | **已适配** | `scripts/check-*.sh` + `.github/workflows/ci.yml` —— 七道门禁的实现 |
| RFC-0048 | 每仓库可配置治理 | **已适配** | 见 §3 `per-repo-governance.md` 行 |
| RFC-0042 | 执行证明（Proof of Execution） | **部分适配** | 审计留痕落地；Merkle 签名信封不落地 —— 见上游概念文档 `proof-of-execution.md` 行 |

### 4.2 待接入的 RFC（需要管道 / 基础设施）

| RFC | 主题 | 前置条件 | 若不接入的现状 |
|---|---|---|---|
| RFC-0002 | 管道编排 | `ai-sdlc init` | 阶段由人工 / 代理按阶段表推进 |
| RFC-0010 | 并行执行与 worktree 池 | 管道 + worktree 池 | 并行只在评审阶段（三类评审） |
| RFC-0015 | 自主管道编排器 | 管道 + 自主调度 | 调度由人决定 |
| RFC-0023 | 操作者 TUI | TUI 运行时 | 无终端 UI（用 Cursor + CI） |
| RFC-0026 | 探索工作流模式 | 管道 | 探索在会话内进行，不产出结构化产物 |
| RFC-0032 | 成本治理缝 | token 计量 | 无成本核算 |
| RFC-0033 | 治理报告层 | 治理数据汇聚 | 度量手工汇总（见 `metrics-playbook.md`） |
| RFC-0041 | conductor-worker 进程架构 | 多进程管道 | 单会话多代理 |
| RFC-0044 | worktree head-parent 串行化 | worktree 池 | 单分支工作 |
| RFC-0045 | 采纳者健康上报 | 上游账号 | 无上报 |
| RFC-0047 | 可重推导的隔离锚点 | CI 独占签名密钥 | 独立性只有第一层 |
| RFC-0049 | 统一判定层 | 判定服务 | 判定分散在门禁脚本 |
| RFC-0050 | 用量台账与模型路由 | token 计量 + 证据路由 | 模型由配置静态决定 |
| RFC-0051 | 会话层级并行派发 | 管道 | 无会话层级 |
| RFC-0052 | 分阶段评审管道 | judgment layer + 探针执行器 | 三类并行评审（成本更高，流程等价） |
| RFC-0053 | 即时上下文引擎 | 上下文预算服务 | 提示自足 + 上下文预算纪律（`ai-sdlc-agents.mdc` §6） |
| RFC-0054 | PR 生命周期事件驱动派发 | PR webhook | 手动触发 |
| RFC-0056 | 可组合工作流图 | 管道 | 线性阶段表 |
| RFC-0059 | 临时执行器 | 管道 | 常规代理会话 |
| RFC-0060 | 分布式 LLM worker 调度 | 分布式 worker | 单机 |

### 4.3 有意不适用的 RFC（产品侧 / 框架实现侧）

以下 RFC 属**产品侧**（Soul DID / 设计系统 / 优先级算法 / 需求管理）或**框架实现侧**
（管道运行时、框架自身的 CLI 行为）。本仓库是 Rust 后端服务，不涉及这些领域。

| RFC | 主题 | 不适用理由 |
|---|---|---|
| RFC-0004 | 成本治理与归因 | 需要 token 计量台账；本仓库不按 agent 维度核算成本。待接入前提：token 计量 |
| RFC-0005 | 产品优先级算法（PPA） | **产品侧** —— 决定「先做什么」的排序算法。本仓库的任务优先级由人按 backlog 顺序决定，不引入自动排序 |
| RFC-0006 | 设计系统治理 v5 | **前端设计系统侧** |
| RFC-0007 | Figma Make 管道集成 | **设计工具侧** |
| RFC-0008 | PPA 三元组集成 | **产品侧** |
| RFC-0009 | 镶嵌式设计意图文档 | **设计系统侧**（本仓库无设计意图概念） |
| RFC-0013 | 产品优先实现策略 | **产品侧** |
| RFC-0017 | Soul Variant 模式 | **产品侧** |
| RFC-0018 | Soul Journey 模式 | **产品侧** |
| RFC-0019 | embedding 供应商适配器 | **框架侧适配器** —— 本仓库有 `nexus-rag` / `nexus-vector`，但那是**产品能力**（向量检索），不是 AI-SDLC 流程适配器 |
| RFC-0022 | 合规姿态审计面 | **前端合规侧**（WCAG 等） |
| RFC-0024 | 涌现式 issue 捕获与分诊 | **产品侧需求管理** —— 本仓库的 issue 入口是人工登记的 backlog 任务 |
| RFC-0025 | 框架质量监控 | **框架自身质量** —— 本仓库监控的是**本项目**的流程质量，两者不是一回事 |
| RFC-0027 | 设计一致性漂移检测 | **设计系统侧** |
| RFC-0028 | 工程轴基底强制 | **Soul 作用域** —— 无 Soul DID 概念 |
| RFC-0029 | 产品支柱架构愿景 | **产品侧** |
| RFC-0030 | 信号摄取管道 | **产品分析侧** |
| RFC-0031 | 校准驱动的 DID 修订 | **产品侧** |
| RFC-0040 | PPA 分诊诊断与信号源 | **产品侧** |
| RFC-0043 | 非受信贡献者 PR 验证 | **无外部贡献者流程**。**若将来开放外部 PR**：这是必须引入的第一道门（保护路径 + 差分测试 + 干净室签名） |
| RFC-0055 | 代理评测与计划门禁 | **框架自评测** —— 本仓库的门禁是 CI 脚本，不是对代理本身的评测集 |
| RFC-0058 | mods 上的会话层级 | **框架内部** —— mods 是上游框架的内部模块结构 |
| RFC-0061 | 分支外的评审裁决门禁 | 需要分支外的裁决存储 —— 本仓库的评审结论在 PR 上（分支内载体） |

> **注意 RFC-0043 的特殊性**：它今天「不适用」是因为**没有外部贡献者**，
> 而不是因为「不需要安全」。一旦有外部 PR 提交，它就是**必须引入**的门禁，
> 且**优先于**其他一切新增门禁。

---

## 5. 「有意不适用」的汇总与理由

把上表里的「不适用」归因，会收敛到**四条根因**。每一条都不是「懒得做」。

| 根因 | 涉及条款 | 判据 |
|---|---|---|
| **A. 产品侧领域** —— Soul DID / Variant / Journey / 设计系统 / 合规 / 优先级算法 / 需求管理 | §2 的 `primer.md`、`design-intent.md`；§3 的 `design-review-architecture.md`、`substrate-contract.md`、`variants.md`、`operator-tui.md`、`adopter-translators.md`；§4.3 的大部分 | 本仓库是 **Rust 后端服务**，无前端、无设计系统、无产品组合管理、无用户旅程建模 |
| **B. 框架实现侧** —— 资源 schema、编排器、CLI、管道运行时 | `spec/spec.md` 的资源类型部分、§4.2 全部、`spec/schemas/**` | 本仓库是**采纳者**：借用判据，不实现框架。框架的运行时由上游提供 |
| **C. 尚无对应基础设施** —— 沙箱、签名密钥、token 计量、worktree 池、分诊信号源 | `reviewer-independence.md` 的 `attested`/`isolated`、`proof-of-execution.md` 的签名信封、`usage-ledger-model-routing.md`、RFC-0043/0047/0050/0053 | 需要**先有基础设施**，再谈条款落地。**顺序不能反** —— 先接判据而没有基础设施，会造出「无法执行的判据」 |
| **D. 无外部参与者** —— 外部贡献者 PR、外部需求入口 | `untrusted-contributor-verification.md`、`zero-trust-untrusted-pr-verification.md`、RFC-0043 | 当前贡献者均在信任名单内。**引入外部 PR 的同时必须引入对应门禁** |

**根因 C 与 D 是「待接入」而不是「不适用」** —— 它们有明确的前置条件，
前置条件具备后条款即生效。上表 §4.2 与 §3 的「待接入」行都写明了前置条件。

---

## 6. 本矩阵的维护纪律

### 6.1 何时必须更新

| 事件 | 必须更新 |
|---|---|
| 新增 / 修改 `.cursor/rules/` 下的规则文件 | §2–§4 中对应行的载体路径 |
| 上游 AI-SDLC 插件升级 | 全文重扫 §2–§4，核对上游文件清单有无增减 |
| 接入管道（`ai-sdlc init`） | §4.2 的「待接入」逐行重新判定；§0 的前提表需要改写 |
| 开放外部贡献者 PR | §5 根因 D 失效，RFC-0043 从「不适用」改为「必须引入」 |

### 6.2 机器校验

本矩阵中「已适配 / 部分适配」行引用的路径，由 `bash scripts/check-docs.sh` 校验存在性。
**路径失效 = 门禁红** —— 这与 `scripts/check-rules.sh` 对规则文件的校验是同一条道理。

### 6.3 禁止的写法

- ❌ 载体路径写成散文（「见规则文件」）而不给路径 —— 不可校验。
- ❌ 「不适用」不写理由 —— 空白即静默失效。
- ❌ 「待接入」不写前置条件 —— 无法判断何时该接。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `docs/README.md` | `docs/` 总索引 |
| `docs/ai-sdlc/README.md` | AI-SDLC 文档导航 |
| `docs/operations/ai-sdlc-workflow-handbook.md` | 流程手册（叙事版） |
| `.cursor/rules/ai-sdlc-workflow.mdc` | 阶段表与权威顺序（**判据的入口**） |
| `scripts/check-docs.sh` | 本矩阵引用路径的存在性校验 |
