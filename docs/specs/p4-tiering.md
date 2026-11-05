# P4 Agent 运行时 — 分层判定（S1）

> 判据见 `.cursor/rules/ai-sdlc-workflow.mdc` §3 与 `ai-sdlc-autonomy.mdc` §2.1。
> 本文件是**判定记录**，不是产出物 —— 产出物见 [`p4/`](p4/spec.md)（Tier 2）。

## 诉求

架构文档 §6.1 的 **P4 阶段**：

| 项 | 内容 |
|---|---|
| 阶段 | **P4 Agent 运行时** |
| crate | `nexus-agent` |
| 范围 | ReAct、上下文预算 |
| 完成判据 | **ReAct 循环有测试** |
| 当前状态 | ✅ 完成（NEX-26..30） |

`nexus-agent` 交付前是 **7 行空壳**（只有 `CRATE_NAME`）；
**S4** 已交付 5 个模块（`traits` / `protocol` / `compaction` / `events` / `react`）。

## 判据回顾

`ai-sdlc-workflow.mdc` §3 的判据：

> 存在合理工程师会就正确做法产生分歧 → 升到 RFC；
> 一个特性能拆成多个任务 → 升到 Spec；否则直接进 Task。

## 判定：Tier 2 · Spec

### 不是 Tier 1 · RFC —— 但**有 2 个 RFC 级开放问题**

**先看 RFC 判据**：「存在合理工程师会就正确做法产生分歧」。

逐项检查 P4 的技术选择：

| 技术选择 | 会不会有分歧 | 依据 |
|---|---|---|
| ReAct 循环的自研 vs 移植 Eino | ❌ 不会 | 参考架构 §2.2 已**定稿**：「必须自研的原因：Eino 的 `adk` + `compose` 组合没有 Rust 对等实现」 |
| `ChatModel` / `InvokableTool` 的 trait 形态 | ❌ 不会 | 参考架构 §2.2 已给出 trait 边界草案 |
| 中间件 trait 形态 | ❌ 不会 | 同上，`ModelMiddleware` / `ToolMiddleware` 已给出 |
| 事件 sink 的背压实现（方案 A 零锁 vs 方案 B） | ❌ 不会 | 参考架构 §2.2 **推荐**方案 A（`AtomicU64` + `fetch_add`） |
| **上下文裁剪的语义**（整轮淘汰？摘要？） | ⚠️ **会** | 见 OQ-1 |
| **`nexus-agent` 是否直接依赖 `nexus-llm` 的具体类型** | ⚠️ **会** | 见 OQ-2 |

**结论**：P4 的**主干**（ReAct 循环、trait 边界、中间件、事件 sink）
在参考架构里**已经定稿**，不构成 RFC 级分歧。

但有 **2 个开放问题**的答案会**实质改变 crate 边界与核心契约** ——
它们符合 RFC 判据（「就正确做法产生分歧」）**且**是架构级的。

> 按 `ai-sdlc-decision-authority.mdc` §4.1，阻塞性 OQ **必须**升级并**登记**，
> **不得**在代码或注释里顺手决定。
> 本判定**不**替它们做决定 —— 见 §「登记的开放问题」。

### 是 Tier 2 · Spec —— 不是 Task 层

P4 **明显**能拆成多个任务：

| 子能力 | 可独立评审 | 依赖 |
|---|---|---|
| `ChatModel` / `InvokableTool` trait 边界 | ✅ | — |
| ReAct 循环本体（迭代上限、终止条件） | ✅ | trait 边界 |
| 上下文历史裁剪（`compact_input_history`） | ✅ | trait 边界 |
| 中间件链 | ✅ | trait 边界 |
| 事件 sink 与背压 | ✅ | — |
| 工具结果的结构化回灌 | ✅ | ReAct 循环 |
| `MaxIteration` 等错误码接入 | ✅ | ReAct 循环 |

**7 个可独立评审的子能力** —— 「一个特性拆成多个任务」的判据成立。

而且 P3 已经确立了**可借鉴的模板**：
`docs/specs/p3/{spec.md,tasks.md,contracts/}` 三件套 +
`invariants.json` 逐条指向真实测试名。

### 判定结论

**Tier 2 · Spec**。

- **不**升 Tier 1：主干已定稿，2 个 OQ **登记**后可在 Spec 内解决
  （若 OQ 的解答需要改**已定稿条款**，则**那时**才升 RFC）。
- **不**直接进 Task：7 个子能力必须先有**可追溯的契约**，
  否则「任务拆解」没有契约可依。

## 复杂度打分

按 `ai-sdlc-autonomy.mdc` §2.1 的五个维度，取**最高分**：

| 维度 | 估计 | 理由 |
|---|---|---|
| **契约影响** | **4** | P4 会**新增**业务码（如 `MaxIteration`）—— 触及契约但**不改动**已有码值 |
| **依赖面** | **5** | 跨 crate（`nexus-agent` ↔ `nexus-llm` ↔ `nexus-tool`），且涉及依赖**方向**的决策（OQ-2） |
| **可逆性** | 3 | 无数据迁移，无不可逆状态变更 |
| **探索性** | 3 | 路径**已确定** —— 参考架构 §2.2 给了 trait 边界与实现清单 |
| **并发/一致性** | 4 | 涉及并发工具调用、背压、事件序号单调性 |

**最高分 = 5** → 复杂度**中** → 路由策略 **`ai-with-review`**（默认）。

**自治等级**：Level 2（独立实现 Task 层任务、自开分支、自开 PR），
但**合并前需人工批准**，且 **AI 作者的改动需额外一名评审**。

## 契约影响五问

按 `AGENTS.md` §3，实现前必须逐项回答（是 / 否）：

| # | 问题 | 回答 | 说明 |
|---|---|---|---|
| 1 | 改动**错误码值**吗？ | **否** | P4 **不新增**码值，只**复用** `nexus-core` 已定稿的 42 个（如 `MaxIteration` 30002） |
| 2 | 改动**事件名或负载形状**吗？ | **否** | P4 复用 `nexus-core` 的 12 个事件名与负载类型 |
| 3 | 改动 **ID 底层类型**吗？ | **否** | 复用 `nexus-core` 的 18 个 ID 类型 |
| 4 | 改动 **crate 依赖方向**吗？ | **否** | OQ-2 已落定为 D4：`nexus-agent` 自定义 trait、复用 `nexus-llm` **数据类型**、`Gateway` 经 trait 注入 —— `check-deps.sh` 的依赖表**不变** |
| 5 | 改动**对外响应形状**吗？ | **否** | P4 不产生 HTTP 响应（那是 P5） |

> 第 4 项已由 OQ-2 落定为**不改动** —— D4 **不新增**任何 crate 依赖方向，
> 因此**不触碰**架构文档 §6.3 的已定稿条款。
> 按 `ai-sdlc-decision-authority.mdc` §2.1，**收紧**依赖方向**不算削弱控制**，
> 所以这个决策**不需要**时间盒。

## 开放问题的落定（2026-11-05）

> 以下两个 OQ 由**操作者**于 2026-11-05 落定。
> 按 `ai-sdlc-decision-authority.mdc` §4.1，OQ 的解决权**专属操作者** ——
> 代理**只登记**并**呈现选项**，落定由操作者作出。

| OQ | 落定 | 决策编号 | 契约载体 |
|---|---|---|---|
| **OQ-1** 上下文裁剪语义 | **整轮淘汰**，不生成摘要 | **D5** | [`p4/spec.md §3.2、§6.2`](p4/spec.md) |
| **OQ-2** `nexus-agent` ↔ `nexus-llm` 依赖形态 | **自定义 trait + 复用 `nexus-llm` 数据类型** | **D4** | [`p4/spec.md §3.1、§6.1`](p4/spec.md) |

两条决策的**依据、反方论点与回应**写在 `p4/spec.md` §6.1 / §6.2 ——
包括「类型下沉到 `nexus-model` 才是终局形态」这条**成立**的反方论点，
以及它被登记为后续任务 **F1**（触发条件：P8）的原因。

## 登记的开放问题（历史记录）

> 以下保留 S1 判定时的**原始登记内容**，作为决策的**依据来源**。
> 它们**已落定**，不再是未决问题。

### OQ-1 — 上下文裁剪：整轮淘汰还是摘要？

| | |
|---|---|
| **问题** | 超出预算时，历史是**整轮淘汰**（丢最旧的完整轮次）还是**摘要压缩**？ |
| **来源** | 参考实现 `compactInputHistory`（`internal/agent/runtime/context_history.go:17-21`）是**整轮淘汰**且**明确不生成摘要**。但 Rust 版是否照搬？ |
| **选项 A** | **整轮淘汰**（照搬参考实现）—— 保留 system 消息 + 首个 user 之前的前缀 + 最新一轮 user；历史 user 轮按**整轮**删除（含该轮所有并行工具结果） |
| **选项 B** | **摘要压缩** —— 保留语义但丢失原文 |
| **选项 C** | **不裁剪，报错** —— 超预算直接失败，让上层决定 |
| **参考实现的理由** | 「不生成摘要，不删除任何持久历史」—— 摘要会引入**事实漂移**（摘要错了，模型就基于错误事实推理） |
| **影响面** | `compact_input_history` 的实现、ReAct 循环的裁剪时机、以及「协议完整性先于裁剪」这条不变量的具体形态 |
| **我的倾向** | **A**（整轮淘汰）—— 但**这是操作者的决定**，不在本判定里落定 |

### OQ-2 — `nexus-agent` 与 `nexus-llm` 的依赖形态

| | |
|---|---|
| **问题** | `nexus-agent` 的 `ChatModel` trait 与 `nexus-llm` 的 `Gateway` 是**什么关系**？ |
| **选项 A** | **`nexus-agent` 定义自己的 `ChatModel` trait**，由 `nexus-service` 装配层把 `nexus-llm::Gateway` **适配**成它 —— `nexus-agent` **不依赖** `nexus-llm` |
| **选项 B** | `nexus-agent` **直接依赖** `nexus-llm` 的类型（`ChatRequest` / `ChatResponse`） |
| **参考实现的理由** | 参考架构 §0.3 明确：「`host.HostRouter` 不直依赖 gateway …… **不要**让 `nexus-agent::host` 依赖 `nexus-llm` 的具体 `Gateway` 类型」—— 用 `Arc<dyn Fn>` 注入 |
| **但** | `nexus-llm` 已经**落地**了 `ChatRequest` / `ChatResponse` / `ToolDef` / `Usage` 等类型。**重复定义**一套是浪费 |
| **影响面** | crate 依赖方向、类型重复、装配层代码量 |
| **我的倾向** | **A 的变体** —— `nexus-agent` 定义 trait，但 trait 的**方法签名**用 `nexus-llm` 的**数据类型**（不是自己的类型），即「依赖数据、依赖接口，不依赖实现」。这需要**确认**这是否违反 §4.2 的依赖方向铁律 |

> **注意 OQ-2 的性质**：它触及**crate 依赖方向**——
> `ai-sdlc-decision-authority.mdc` §5 明确「架构文档 §6.3 的已定稿条款
> （……crate 边界……）**不得**在实现中重新决定」。
>
> 所以 **OQ-2 必须在 Spec 的「已定稿决策」里落定**，**不能**留到实现期。

## 下一步

| 阶段 | 产物 | 状态 |
|---|---|---|
| **S0** 需求捕获 | 本文件 | ✅ |
| **S1** 分层判定 | Tier 2 · Spec | ✅ |
| **S2** 产出物编写 | [`p4/spec.md`](p4/spec.md) + [`p4/tasks.md`](p4/tasks.md) + `p4/contracts/` | ✅ |
| **S3** DoR 门禁 | NEX-26 ~ NEX-30 的 DoR 判定记录（**已落盘**在各自任务文件里） | ✅ |
| **S4** 实现 | NEX-26 ~ NEX-29（trait 边界 / 整轮淘汰 / 事件 sink / ReAct 循环） | ✅ |
| **S5** 三层验证 | 65 个测试（26 单元 + 5 结构层 + 14 事件 + 20 ReAct） | ✅ |
| **S6** 评审 | 每个任务的**注入故障**验证（变异测试确认断言会红） | ✅ |
| **S7** 合并与交付 | NEX-30 收口（架构文档同步 + 契约升格 + 规则 3b 归零） | ✅ |

> **OQ-1 与 OQ-2 已于 2026-11-05 由操作者落定** ——
> 见上方「开放问题的落定」表。S2 因此**不再被阻塞**。
>
> **两个后续任务已登记但未创建**（Spec §6.3）：F1（协议类型下沉 `nexus-model`，
> 触发条件 P8）与 F2（`FinishReason` 增加 `Canceled` 变体，触发条件 P5）。
> 按 `ai-sdlc-decision-authority.mdc` §5「不扩大范围」，创建任务文件需**人工授权**。
>
> **P4 已于 2026-11-05 完成交付**（NEX-26~30 全部归档到 `backlog/completed/`）：
> 25 条不变量**全部 `verified`**，架构文档 §5.4 / §6.1 已同步，
> §2.6.4.1 业务码表与 §2.2.4.1 事件清单与 P3 收口时**逐字节一致**。

## 相关文件

| 文件 | 作用 |
|---|---|
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1` | P4 的范围与完成判据（**唯一权威**） |
| `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.2` | 自研 ReAct 运行时的 trait 边界与实现清单 |
| `docs/specs/p3/spec.md` | P3 Spec 的**模板**（本 P4 Spec 照它写） |
| `docs/specs/README.md` | Spec 层三件套的规范 |
| `.cursor/rules/ai-sdlc-workflow.mdc §3` | 分层判据 |
| `.cursor/rules/ai-sdlc-autonomy.mdc §2.1` | 复杂度打分维度 |
