# AI-SDLC 术语表

> 只收录**本仓库实际用到的** AI-SDLC 术语。
> 上游完整术语表见 `~/.claude/plugins/marketplaces/ai-sdlc/spec/glossary.md`（324 行，含大量本仓库不涉及的产品侧词汇）。
>
> 三列的含义：
> - **中文释义** —— 在本仓库语境下的确切含义（不是通用定义）
> - **上游出处** —— 该术语的规范定义位置
> - **本仓库载体** —— 在哪里落地

---

## A

### 适配器（Adapter）
为某个外部系统实现一个或多个**接口契约**的独立模块。
**载体**：`docs/ai-sdlc/adapter-inventory.md`、`.cursor/rules/ai-sdlc-adapters.mdc` §2。
上游：`spec/adapters.md` §2。

### 适配器绑定（Adapter Binding）
把某个工具集成为「可替换供应商」的声明式资源。
**载体**：**未落地** —— 本仓库的适配器是仓库内的 Rust crate，靠 `scripts/check-deps.sh` 静态校验，不做运行时绑定。
上游：`spec/spec.md` §5.5。

### 审批策略（Approval Policy）
阶段级配置，声明该阶段执行前是否需要批准、批准层级、超时时长、超时行为。
**载体**：`ai-sdlc-autonomy.mdc` §4。**注意本仓库不用 `auto-approve`** —— 它把「等不到人」变成「没人看也放行」，与 `hard-mandatory` 冲突。
上游：`spec/spec.md`。

### 自治等级（Autonomy Level）
0–3 的四级权限刻度。Level 0 只读；Level 3 在领域内自治。
**载体**：`ai-sdlc-autonomy.mdc` §1。
上游：`spec/autonomy.md`。

### 自治策略（Autonomy Policy）
声明各自治等级的晋级条件与降级触发的资源。
**载体**：`ai-sdlc-autonomy.mdc` §3–§4（以规则文件形态，而非 YAML 资源）。
上游：`spec/spec.md` §5.4。

---

## B

### 预算压力（Budget Pressure）
消费上升时自动把任务路由到更便宜模型的机制。
**载体**：**未落地** —— 本仓库的模型由配置静态决定（`nexus-llm` 的供应商描述符）。
上游：RFC-0004。

---

## C

### 复杂度分数（Complexity Score）
1–10 的数值，决定任务所需的**最低自治等级**与人工介入程度。
**载体**：`ai-sdlc-autonomy.mdc` §2.1（本仓库的打分维度表）。
上游：`spec/autonomy.md`。

### 条件（Condition）
资源 `status.conditions` 数组里的结构化状态项，含 `type` / `status` / `reason` / 时间戳。
**载体**：**未落地**（无资源模型）；**等价物**是任务 frontmatter 的 `status` 与 `## DoR 判定` 段落。
上游：`spec/spec.md` §6。

### 凭据策略（Credential Policy）
阶段级配置，声明 JIT（即时）凭据的 scope、存活时间与吊销行为。
**载体**：`ai-sdlc-adapters.mdc` §3。
上游：`spec/spec.md`。

### 复杂度路由（Complexity-based Task Routing）
按复杂度分数把任务分给不同角色的机制。
**载体**：`ai-sdlc-autonomy.mdc` §2。
上游：`spec/autonomy.md`。

---

## D

### DoR 门禁（Definition of Ready Gate）
**先于任何代码写入**的一道门禁：任务不通过七项 rubric 就不许开工。
**载体**：`.cursor/rules/ai-sdlc-dor-gate.mdc`（判据）、`docs/ai-sdlc/dor-playbook.md`（操作）。
上游：RFC-0011。

### 决策目录与操作者路由（Decision Catalog / Operator Routing）
把决策按类别路由到**有权限解决它的人**的机制。
**载体**：`ai-sdlc-decision-authority.mdc` §2（决策三分类：(a) decide-and-proceed / (b) timeboxed / (c) operator-only）。
上游：RFC-0035。

### 降级（Demotion）
因策略违规（安全事件、返工率过高、未授权访问）导致自治等级**自动下调**，并进入冷却期。
**载体**：`ai-sdlc-autonomy.mdc` §3.2。
上游：`spec/autonomy.md`。

### 决策记录（Decision Record）
代理做出的 (a) 类决策必须留下的结构化记录（类别 / 决策 / 依据 / 可逆性 / 影响面）。
**载体**：`ai-sdlc-decision-authority.mdc` §2.2。
上游：RFC-0035。

---

## E

### 执行等级（Enforcement Level）
门禁的严格度三档：`advisory`（只警告）/ `soft-mandatory`（拦截但可覆盖）/ `hard-mandatory`（拦截且不可覆盖）。
**载体**：`ai-sdlc-quality-gates.mdc` §1。
上游：`spec/policy.md` §2。

---

## F

### 失败策略（Failure Policy）
阶段失败时的处理策略：`abort` / `retry` / `pause` / `continue`。
**载体**：**部分落地** —— 语义由 `ai-sdlc-workflow.mdc` §2 的「失败处理」列承载（每阶段一个动作），但**未声明为资源**。
上游：`spec/spec.md`。

### 强制等级 = 执行等级（Enforcement Level）
见「执行等级」。中文里两者常混用，本仓库统一译作**执行等级**。

---

## G

### 根因分析取向（Root-cause Orientation）
见 §5「五步法」。

---

## H

### handoff 契约（Handoff Contract）
代理之间交接必须遵守的**版本化 schema**：每次交接产出可校验、可审计的结构化产物。
**载体**：`ai-sdlc-agents.mdc` §3–§4（含 YAML / JSON 载荷示例）。
上游：`spec/agents.md` §4。

---

## J

### 晋级（Promotion）
满足量化条件并经指定角色显式批准后，**自治等级上调**。**代理不得自我晋级**。
**载体**：`ai-sdlc-autonomy.mdc` §3.1。
上游：`spec/autonomy.md`。

### 角色-目标-背景故事（Role-Goal-Backstory）
定义代理身份的三要素模式：`role`（定位）、`goal`（可判定的目标）、`backstory`（职责边界语境）。
**载体**：`ai-sdlc-agents.mdc` §1。
上游：`spec/agents.md` §2.1。

---

## K

### 跨 harness 评审（Cross-harness Review）
在不同 harness（如 Codex / Claude Code）上分别执行评审，以获得独立视角。
**载体**：**未落地** —— 待接入（前置条件见 `docs/ai-sdlc/coverage-matrix.md` §4.2）。
上游：`docs/concepts/cross-harness-review.md`。

---

## L

### 门禁分级（Gate Severity）
`hard-mandatory` / `soft-mandatory` / `advisory` 三档 —— 见「执行等级」。本仓库额外用 **Block / Warn** 描述 **DoR rubric 项**的严重度，两者**不是同一套东西**：前者是门禁，后者是 rubric 项。
**载体**：`ai-sdlc-dor-gate.mdc` §4。
上游：`spec/policy.md` §2。

---

## M

### 每仓库可配置治理（Per-repo Configurable Governance）
治理策略随仓库走，而不是全局硬编码。
**载体**：`.cursor/rules/` + `scripts/check-*.sh` + `.github/workflows/ci.yml`。
上游：RFC-0048。

---

## N

### DoR 结论三态
`pass` / `needs-clarification` / `blocked`。
**载体**：`ai-sdlc-dor-gate.mdc` §5。
上游：RFC-0011。

---

## O

### 开放问题（Open Question, OQ）
**只能由操作者解决**的未决问题。代理可以发现、登记、呈现选项，但**不得消解**。
**载体**：`ai-sdlc-decision-authority.mdc` §4.1（OQ 三条禁令）。
上游：RFC-0035。

---

## P

### Pipeline（管道）
定义从触发到交付的完整 SDLC 工作流的资源类型。
**载体**：**未落地**（无资源模型）；**等价物**是 `ai-sdlc-workflow.mdc` §2 的 S0–S9 阶段表 + 手册里的全景图。
上游：`spec/spec.md` §5.1。

### Provenance（来源标注）
AI 生成产物的来源记录：模型标识、工具、提示哈希、时间戳、评审人、评审结论。
**载体**：`ai-sdlc-quality-gates.mdc` §5（最低要求：模型 + 工具 + 时间戳）。
上游：`spec/metrics.md` §4.1。

---

## Q

### 门禁（Quality Gate）
针对开发活动评估的策略规则，带明确的执行等级。
**载体**：`ai-sdlc-quality-gates.mdc` §2–§3。
上游：`spec/spec.md` §5.3。

### 决策请求（Request）
准入流水线的输入 —— 在本仓库是 PR。
**载体**：`ai-sdlc-quality-gates.mdc` §2 的六步准入表。
上游：`spec/policy.md` §3。

---

## R

### 路由策略（Routing Strategy）
按复杂度分数把任务分给代理的四种方式：`fully-autonomous` / `ai-with-review` / `ai-assisted` / `human-led`。
**载体**：`ai-sdlc-autonomy.mdc` §5。**本仓库默认 `ai-with-review`**。
上游：`spec/autonomy.md`。

### 复核循环（Reconciliation Loop）
观测当前状态 → 与期望状态求差 → 采取行动弥合差距的持续过程（Kubernetes 控制器模式）。
**载体**：**部分落地** —— 本仓库有「**校准**」概念（`ai-sdlc-observability.mdc` §2），但没有常驻控制器。
上游：`spec/spec.md` §9。

---

## S

### spec/status 分离（Spec/Status Split）
把**期望状态**（人写，系统读）与**观测状态**（系统写，人读）分开。
**载体**：`ai-sdlc-artifacts.mdc` §5。
上游：`spec/spec.md` §4。

### 严重度（Severity）
DoR rubric 项的严重度：`Block` / `Warn`。**默认全是 Block**。
**载体**：`ai-sdlc-dor-gate.mdc` §4。
上游：RFC-0011。

---

## T

### 三层模型（Three-tier Model）
Tier 1 RFC（决策）→ Tier 2 Spec（契约包）→ Tier 3 Task（交付物）。
**层级独立可用**，但任何工作**都不能跳过 Task 层**。
**载体**：`ai-sdlc-artifacts.mdc` §1、`docs/specs/README.md`。
上游：`docs/concepts/spec-driven.md`。

---

## W

### 五步法（Five-step Review）
修复或新增规则前的必走顺序：**质疑需求 → 删除 → 简化 → 加速 → 最后才自动化**。顺序不可调换。
**载体**：`ai-sdlc-decision-authority.mdc` §3。
上游：`spec/policy.md` §6。

---

## X

### 需求侧系统 / 证据路由（Signal / Evidence-based Routing）
从支持渠道、CRM、分析平台摄取信号，喂给优先级算法。
**载体**：**未落地** —— 产品侧，见 `docs/ai-sdlc/coverage-matrix.md` §5 根因 A。
上游：RFC-0030 / RFC-0040。

---

## Y

### 硬边界（Hard Boundary）
阶段之间**不可跳过**的关键交接点。本仓库有两道：**S3→S4**（DoR 先于任何代码写入）、
**S6→S7**（评审先于合并）。
**载体**：`ai-sdlc-workflow.mdc` §2。
上游：RFC-0011 / RFC-0042（精神一致）。

---

## Z

### 增量子代理（Ephemeral Executor）
为单次任务临时起、用完即弃的执行器。
**载体**：**未落地** —— 待接入（前置条件见 `docs/ai-sdlc/coverage-matrix.md` §4.2）。
上游：RFC-0059。

---

## 术语的读法

遇到本仓库文档里的英文术语时：

1. 先查本文档；
2. 查不到 → 查 `.cursor/rules/` 对应规则文件的「相关文件」表；
3. 还查不到 → 查上游 `~/.claude/plugins/marketplaces/ai-sdlc/spec/glossary.md`；
4. **三者都查不到 → 视为文档缺陷**，按 `docs/ai-sdlc/coverage-matrix.md` §6.3 的纪律登记任务。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `docs/ai-sdlc/coverage-matrix.md` | 上游条款的适配状态 |
| `docs/ai-sdlc/README.md` | AI-SDLC 文档导航 |
| `.cursor/rules/ai-sdlc-workflow.mdc` | 权威顺序与阶段表 |
| `~/.claude/plugins/marketplaces/ai-sdlc/spec/glossary.md` | 上游完整术语表 |
