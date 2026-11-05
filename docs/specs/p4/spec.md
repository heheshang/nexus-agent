# Spec · P4 Agent 运行时

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 权威顺序：架构文档 > 本 Spec > `backlog/tasks/*`（Task 层）。
> 本文件是**契约**，不是进度记录 —— 进度看 `backlog/tasks/` 的任务状态。
> 分层判定记录见 [`../p4-tiering.md`](../p4-tiering.md)。

**特性**：`nexus-agent` —— ReAct 循环、上下文预算（整轮淘汰）、事件 sink 与背压。
**架构文档阶段**：§6.1 的 P4 行。
**完成判据**：**ReAct 循环有测试**（拆成可验证子目标，见 §5）。

---

## 1. 范围

### 1.1 在范围内

| 项 | 说明 |
|---|---|
| `ChatModel` trait | `nexus-agent` **自定义** trait，方法签名用 `nexus-llm` 的**数据类型**（决策 D4） |
| ReAct 循环本体 | 迭代上限、单步超时、总超时三个安全阀；终止条件 |
| 上下文历史裁剪 | `compact_input_history` —— **整轮淘汰**（决策 D5），协议完整性校验**先于**裁剪 |
| 工具 trait | `InvokableTool` —— 工具错误**结构化回灌**，不抛异常终止循环 |
| 事件 sink 与背压 | `nexus-agent` 自己的 sink（**不复用** `nexus-core` 的 `EventSink`，见 §3.5）；双通道（可丢 / 必达） |
| 终态事件 | `done` / `error` 的**终态分类**（取消 ≠ 失败，见 §3.6） |

### 1.2 不在范围内

| 项 | 理由 |
|---|---|
| **协议类型下沉到 `nexus-model`** | 决策 D4 的**后续任务**（触发条件见 §6.3），不属 P4 |
| SSE 桥接 / 半帧处理 | 属 P5（`nexus-api`） |
| 危险工具的**授权边界** | 属 P8（`nexus-tool` / `nexus-mcp`）—— P4 只做「工具错误结构化回灌」的**语义** |
| Schema 子集白名单校验 | 属 P8；P4 只把工具的 `parameters` 原样透传给模型 |
| 多智能体路由 / Host | 属后续阶段；P4 的 ReAct 循环是**单 Agent** |
| Checkpoint / 中断恢复 | 属 P6（`nexus-workflow` + `nexus-execution`） |
| 长期记忆召回 / RAG 组装 | 属 P7；P4 的输入历史由调用方**组装好**传入 |
| 真实上游连通性 | P4 不依赖真实模型服务；用**本地 mock** 测 |
| `nexus-agent` 的**装配层** | 属 P5/P6 —— P4 只提供 crate 内的运行时，**谁**构造它由上层决定 |

---

## 2. 不可让步的红线

### 2.1 契约不可改

`nexus-core` 的 **42 个业务码、12 个事件名、18 个 ID 类型已定稿**（架构文档 §6.3）。

P4 **复用**下列码值，**不得新增**：

| 码值 | 枚举 | HTTP | `RetryPolicy` | P4 用途 |
|---|---|---|---|---|
| `30002` | `MaxIteration` | 500 | `Terminal` | 超出迭代上限 |
| `30006` | `BudgetExceeded` | 500 | `Terminal` | 裁剪后仍超预算 |
| `10001` | `InvalidParam` | 400 | `Terminal` | 工具调用协议非法（ID 缺失 / 重复 / 配对断裂） |
| `70001` | `ToolNotFound` | 404 | `Terminal` | 工具不存在 |
| `70002` | `ToolFailed` | 500 | `UnknownOutcome` | 工具执行失败（**结构化回灌**，不终止循环） |
| `70003` | `ToolTimeout` | 504 | `UnknownOutcome` | 工具执行超时 |
| `50001` | `ModelUnavailable` | 502 | `Retryable` | 模型调用失败 |
| `50002` | `ModelTimeout` | 504 | `UnknownOutcome` | 单步 / 总超时 |
| `90003` | `ServiceUnavailable` | 503 | `Retryable` | 运行被取消 |
| `90001` | `Internal` | 500 | `Terminal` | 内部状态不一致（如工具结果与调用不配对） |

> **`ToolFailed` 的 `UnknownOutcome` 分类是关键** —— 工具可能**已产生副作用**
> （支付、写库）。它**不**终止 ReAct 循环：错误**结构化回灌**给模型，
> 让模型自己决定下一步。但**它绝不能**被当作可自动重试（见 §2.4）。

### 2.2 依赖方向

**不新增任何 crate 依赖方向。** P4 **不**改动 `scripts/check-deps.sh` 的依赖表。

`nexus-agent` 的既有依赖（`nexus-core` / `nexus-config` / `nexus-model` / `nexus-llm` /
`nexus-tool` / `nexus-observability` / `nexus-security`）**全部保留**，
**不新增**任何内部 crate。

由 `scripts/check-deps.sh` 规则 7（`deps_of` ↔ `Cargo.toml` 一致性）+ 规则 2 强制。

### 2.3 不引入 `Box<dyn>` 于热路径

`ChatModel` / `InvokableTool` 的 trait **由 `nexus-agent` 定义**（consumer-defined trait，
架构文档 §4.1.2），但 `nexus-agent` **内部**用**泛型 + 静态分发**。

`#[async_trait]` 只在**需要 `dyn`** 时才用 —— 参考架构 §2.2 的性能提示：
「`nexus-agent` 内部用泛型 + AFIT 获得静态分发，只有在 `nexus-service` 装配层才 `dyn` 化」。

**工具注册表**是**异构集合**（内置 / MCP / 用户自定义工具混在一起）——
那是架构文档 §2.3.3 明确保留的 `dyn` 场景，所以 `InvokableTool` **必须** `dyn` 兼容。

### 2.4 失败语义必须显式建模

| 语义 | P4 行为 |
|---|---|
| `Retryable` | **不**在 ReAct 循环内自动重试 —— 重试是**网关层**（`nexus-llm`）的职责 |
| `UnknownOutcome` | **绝不**自动重试（工具可能已产生副作用） |
| `Terminal` | 不重试 |

> **P4 不重写重试判据** —— 一律取 `nexus-core` 的 `RetryPolicy`
> （架构文档 §2.6.3）。这与 `nexus-llm` 的纪律一致。

### 2.5 不引入 `unsafe`

workspace 已 `forbid`。

### 2.6 内部原因不外泄

`AppError.cause` **绝不**进入事件负载或对外响应（架构文档 §2.6.5）。
`nexus-agent` 产出的事件负载只用 `nexus-core` 的 `EventPayload` 类型 —— 它们**没有** `cause` 字段。

---

## 3. 核心契约

### 3.1 ReAct 循环（管道顺序**有讲究**）

```
run(input, tools, cancel)
  → 协议完整性校验（tool_call ↔ tool_result 配对）        ← **先于**任何裁剪
  → 上下文裁剪（整轮淘汰）                                  ← 每步都重跑
  → 模型调用（ChatModel::chat）
  → 校验本步输出（tool_call ID / 最终答案）
  → 有工具调用？
      ├─ 否 → 发 done，终态
      └─ 是 → 并发执行工具（Semaphore 限流）
                → 工具结果**结构化回灌**为 tool 角色消息
                → 发 tool_result 事件
                → 回到循环开头
```

**三个安全阀**（参考架构 Phase 1「必抄的不变量」②）：

| 阀 | 触发 | 终态 |
|---|---|---|
| 最大迭代 | 迭代次数 > `max_iterations` | `error` / `MaxIteration`（30002） |
| 单步超时 | 单次模型调用超时 | `error` / `ModelTimeout`（50002） |
| 总超时 | 整个运行超时 | `error` / `ModelTimeout`（50002） |

> **三个阀的默认值由调用方传入**，`nexus-agent` **不内置**隐式默认值 ——
> 缺省值会让「没配」和「配成 0」无法区分（同 `nexus-llm` 的 `from_config` 纪律）。

### 3.2 上下文裁剪：**整轮淘汰**（决策 D5）

```
1. 校验协议完整性（tool_call ↔ tool_result 配对）—— **先于**丢弃任何历史
2. 估算总量（消息 + 工具定义）≤ 预算？ → 原样返回
3. 找出所有 user 消息位置
4. 从最早的 user 轮开始，**整轮**删除（到下一个 user 轮为止的所有消息，跳过 system）
   —— 直到总量 ≤ 预算
5. 仍超预算 → `BudgetExceeded`（30006）
6. 复核裁剪结果（协议完整性仍然成立）
```

**保留**：所有 system 消息、首个 user 之前的前缀、**最新一轮 user**。

**不生成摘要**（决策 D5），**不删除任何持久历史**。

| 被淘汰的 | 说明 |
|---|---|
| ✅ 整轮淘汰 | 一个 user 轮 = 该 user 消息 + 其后的 assistant 消息 + **所有**并行 tool 结果 |
| ❌ 按条淘汰 | 会留下**孤立**的 tool_result —— 直接破坏模型协议 |

**为什么整轮淘汰而不是摘要**（决策 D5 的依据）：

1. **可验证性** —— 整轮淘汰的每条不变量都能写成二元断言；摘要的正确性**在原理上无法断言**。
2. **事实漂移** —— 摘要错了，模型就基于错误事实推理。
3. **注入面** —— 工具输出是**不可信输入**；摘要把「被丢弃的数据」变成「被保留的上下文」。

> **裁剪是纯函数**：不修改入参，返回新的 `Vec<Message>`。

### 3.3 工具错误**结构化回灌**

工具**不抛异常**终止循环。失败被包成**观察**（observation），作为 `tool` 角色消息回灌给模型：

```json
{"error": "tool_failed", "message": "..."}
```

这是 Agent **自我修正**能力的前提 —— 模型看到错误可以换个参数重试。

| 工具结果 | 事件 `status` | 消息内容 |
|---|---|---|
| 成功 | `ToolStatus::Ok` | 工具输出 |
| 失败 | `ToolStatus::Failed { code }` | `{"error": ..., "message": ...}` |

**但**：工具失败**不终止** ReAct 循环（除非失败本身是协议非法）。

### 3.4 工具并发限流

同一轮内**多个**工具调用**并发**执行，用 `Semaphore` 限流。

参考实现的默认是 `MaxConcurrentTools = 4`。**限流值由调用方传入**（同 §3.1 的安全阀纪律）。

### 3.5 事件 sink：**不复用** `nexus-core` 的 `EventSink`

`nexus-core::EventSink` 的形态是 `emit(&mut self)` —— **同步**、**单写者**、序号由**调用方**同步分配。

P4 的 ReAct 循环是**并发**的（多工具并发、模型调用与事件发送交错），需要**异步**发送 + **背压**。
所以 P4 **自己**定义 sink，**不复用** `nexus-core` 的那个。

但**序号单调**这条不变量**必须**成立 —— P4 的 sink 用**单点分配**（参考架构 §2.2 方案 C）：

```
ingress mpsc（生产者只管 send）→ 唯一的序号分配 task → output mpsc（消费者读的流）
```

这样 `Seq` 分配与写入**天然原子**，且**没有锁**。

> **⚠️ 这是与 `nexus-core::EventSink` 的**并存**，不是替换** ——
> `nexus-core` 的是**进程内单次运行流**（`&mut self` 同步分配）；
> P4 的是**并发生产者 + 背压**的异步通道。
> 两者形状不同，强行统一会让其中一个变形。

### 3.6 终态分类：**先固化错误，再取消**

| 情况 | 终态事件 |
|---|---|
| 取消 | `done` / `finish_reason = Stop`（**不是** `error`）—— 取消**不是失败** |
| 单步 / 总超时 | `error` / `ModelTimeout`（50002） |
| 迭代超限 | `error` / `MaxIteration`（30002） |
| 预算不足 | `error` / `BudgetExceeded`（30006） |
| 工具结果与调用不配对 | `error` / `Internal`（90001） |
| 其他错误 | `error` / `AppError` 的码 |
| 正常 | `done` / `finish_reason = Stop` |

**关键顺序**：先固化错误，再 `cancel()` —— 否则供应商失败会被 `cancel` 覆盖成「用户取消」
（参考实现 `eino_runner.go:216-218` 的注释原文）。

> **⚠️ 取消的 `finish_reason` 表达**：架构文档 §2.2.4.1 的 `done` 事件只写了
> `finish_reason`，而 `nexus-core` 的 `FinishReason` enum 目前只有
> `Stop` / `ToolCalls` / `Length` —— **没有** `Canceled` 变体。
>
> P4 **不新增** `FinishReason` 变体（那是架构文档 §6.3 已定稿的事件契约）。
> 取消**通过** `done` + `FinishReason::Stop` 表达，
> **取消的事实由指标与日志承载**（`RequestStatus::Canceled` 已是 P3 交付的形状）。
>
> 这个**表达力缺口**已登记为后续任务（见 §6.3）。

### 3.7 终态事件**必达**，增量**可丢**

沿用架构文档 §3.2 的双通道：

| 通道 | 事件 |
|---|---|
| **可丢** | `delta` / `reasoning` |
| **必达** | 其余全部（含 `tool_call` / `tool_result` / `done` / `error`） |

背压超时 → **主动取消**整个运行（不是无限阻塞）。

---

## 4. 安全契约

### 4.1 工具输出是不可信输入

工具输出**不得**被当作指令执行。它只是**数据**，回灌给模型时作为 `tool` 角色消息。

工具输出**超长**必须截断 —— 否则一个返回 10MB 的工具会**撑爆**上下文预算。

### 4.2 工具定义不参与「给模型看」之外的语义

`ToolDef` 的控制面字段（`dangerous` / `replay_policy` / `source_identity` /
`binding_version` / `capability`）由 `nexus-llm` 保持**不参与序列化**（P3 已交付）。
P4 **不改动**这条 —— 它**原样复用** `nexus-llm` 的 `ToolDef`。

### 4.3 凭据不外泄

P4 **不接触**任何凭据 —— `ModelDescriptor.api_key` 由 `nexus-llm` 承载，
P4 只通过 `ChatModel` trait 与模型交互，**看不到** `api_key`。

---

## 5. 完成判据与「防伪」要求

架构文档 §6.1：**「ReAct 循环有测试」**

拆成**五个**可验证子目标：

1. **ReAct 循环有测试** —— 循环能跑通：模型要求工具 → 工具执行 → 结果回灌 → 模型给出最终答案。
2. **迭代上限有测试** —— 超过 `max_iterations` 时以 `MaxIteration` 终止。
3. **整轮淘汰有测试** —— 超预算时按**整轮**删除，且协议完整性保持。
4. **协议完整性先于裁剪有测试** —— 配对断裂时**报错**，而不是静默裁剪。
5. **事件序号单调有测试** —— 并发工具调用下，`seq` 严格单调、无重复、无空洞。

### 5.1 「有测试」的**防伪**要求

沿用 NEX-14 的教训：测试「存在」不等于「真的跑了」。

| 假象形态 | 防线 |
|---|---|
| ReAct 循环只测了「模型直接给答案」的一步 | 必须测**至少两轮**（模型要求工具 → 回灌 → 再问）的完整链路 |
| 迭代上限只测了「刚好等于上限」 | 必须测**超出**上限的那一轮触发 `MaxIteration` |
| 整轮淘汰只测了「删掉了一条消息」 | 必须断言「该轮的**所有**并行工具结果**一起**被删」 |
| 裁剪测试只测了「不超预算」 | 必须测**超预算被拒**与**边界值恰好通过**两个方向 |
| 协议完整性只测了「正常配对」 | 必须测**配对断裂报错**（孤立 tool_result / 缺失 tool_result） |
| 事件序号只测了「单线程递增」 | 必须测**并发**工具调用下的单调性 |
| 工具失败测了「返回错误」 | 必须断言**循环未终止**（错误回灌后模型仍能给出答案） |
| 取消测了「返回错误」 | 必须断言终态是 `done`（**不是** `error`）—— 取消 ≠ 失败 |

---

## 6. 已定稿的技术决策

| 编号 | 决策 | 选择 | 依据 | 类别 |
|---|---|---|---|---|
| D4 | `nexus-agent` ↔ `nexus-llm` 的依赖形态 | **自定义 trait + 复用 `nexus-llm` 数据类型** | 见 §6.1 | (a) decide-and-proceed |
| D5 | 上下文裁剪语义 | **整轮淘汰**，不生成摘要 | 见 §6.2 | (a) decide-and-proceed |
| D6 | 迭代上限与并发限流的默认值的**归属** | **调用方传入**，`nexus-agent` 不内置默认值 | 见 §6.4 | (a) decide-and-proceed |
| D7 | 事件 sink 的归属 | **P4 自定义**（不复用 `nexus-core::EventSink`） | 见 §6.5 | (a) decide-and-proceed |

### 6.1 D4 —— `nexus-agent` 自定义 trait，复用 `nexus-llm` 数据类型

**决策**：`nexus-agent` 定义自己的 `ChatModel` trait，**方法签名**用
`nexus-llm::types::{ChatRequest, ChatResponse, StreamEvent}`；
`nexus-llm::Gateway` 经由该 trait **注入**，P4 **不引用** `Gateway` 的具体类型。

**依据**：

1. 参考架构 §2.2 的禁令对象是**具体 `Gateway` 实现类型**
   （「**不要**让 `nexus-agent::host` 依赖 `nexus-llm` 的具体 `Gateway` 类型」），
   **不是**协议数据类型 —— 复用数据类型**不违反**该禁令。
2. `scripts/check-deps.sh` 已允许 `nexus-agent → nexus-llm`，
   D4 **不改动任何依赖方向铁律** —— 即**不触碰**架构文档 §6.3 已定稿条款。
3. **零类型重复** —— 自建一套 `Message` / `ToolDef` 会导致两套语义漂移。
4. **不动已交付的 P3 代码** —— P3 的 19 条不变量路径不受影响。

**反方论点与回应**（决策 rubric §4）：

> 「架构文档 §4.2 差异 2 明说 `nexus-model` 就是给**跨 crate 共享**的 DTO 用的，
> 而 `nexus-llm/src/types.rs` 的模块文档说「不需要 `nexus-model` 承载」，
> 那是在**只有一个共享方**时的判断；P4 一引入第二个共享方，那个前提就**失效**了。
> 类型下沉到 `nexus-model` 才是终局形态。」

**回应**：这条论点**成立且重要** —— 它正是 §6.3 登记的后续任务的**触发条件**。
但排序上仍选 D4，理由是：(1) **不扩大范围** —— 迁移 P3 已交付代码并连带改写其
契约文件，是 P4 完成判据（「ReAct 循环有测试」）**之外**的附带重构；
(2) D4 **不违反**任何参考架构或仓库铁律，类型下沉只是「更整洁」，不是「更合规」；
(3) 类型下沉的收益在 **P8**（`nexus-tool` 需要同一套类型）时才真正兑现。

**可逆性**：高 —— 把类型下沉是**纯增量**的机械重构，不改变 trait 签名语义。
**影响面**：`nexus-agent` 的 `src/react.rs`（trait 定义）+ `Cargo.toml`（**无变化**）。

### 6.2 D5 —— 整轮淘汰，不生成摘要

**决策**：超预算时按**整轮**删除历史（保留 system 消息 + 首个 user 之前的前缀 +
最新一轮 user），**不生成摘要**。

**依据**：见 §3.2 的三条（可验证性 / 事实漂移 / 注入面）。

**反方论点与回应**：

> 「整轮淘汰是**信息损失**。用户在第 2 轮说「只用 CSV 不要用 JSON」，
> 第 40 轮时该约束被整轮淘汰，模型就又开始输出 JSON ——
> 而摘要方案能保住它。成熟产品都在做压缩，整轮淘汰是**功能倒退**。」

**回应**：这条论点**成立**，且正是它让「整轮淘汰」成为**默认**而非**终局**。
但它不能推翻 D5 作为 P4 的第一落点：(1) 参考实现基于同一理由**主动否掉**了摘要；
(2) 摘要的正确性**不可断言** —— P4 的完成判据会退化成一堆**测不出语义**的测试；
(3) 工具输出是不可信输入，摘要把「被丢弃的数据」变成「被保留的上下文」。

**可逆性**：中 —— 换成摘要需要**重写**裁剪函数与相关不变量测试，但不影响其他契约。
**影响面**：`nexus-agent` 的裁剪模块 + 其不变量测试。

### 6.3 后续任务（带触发条件）

| # | 后续工作 | 触发条件 |
|---|---|---|
| F1 | **协议类型下沉到 `nexus-model`** | 当**第二个** crate 需要同一套协议类型时 —— 即 **P8**（`nexus-tool` 需要 `ToolDef` / `Message`）。届时把 `nexus-llm::types` 迁到 `nexus-model`，`nexus-agent` 与 `nexus-llm` 各自依赖 `nexus-model`，`agent → llm` 边消失 |
| F2 | `FinishReason` 增加 `Canceled` 变体 | 当**协议层**（P5，`nexus-api` 的 SSE 桥接）需要**向前端区分**「用户取消」与「正常结束」时。**当前** P4 用 `done` + `Stop` 表达取消（§3.6），事实由指标与日志承载 |

> **这两个后续任务尚未登记为 `backlog/tasks/` 里的独立任务** ——
> 按 `.cursor/rules/ai-sdlc-decision-authority.mdc` §5「不扩大范围」，P4 的 Spec **只登记**，
> **不**在本阶段创建任务文件（创建需要 S2 的完整产出物与 S3 的 DoR 判定）。
> 登记本身需要人工授权。

### 6.4 D6 —— 限流与迭代上限的默认值归调用方

**决策**：`max_iterations` / 单步超时 / 总超时 / 工具并发上限 **全部由调用方传入**，
`nexus-agent` **不内置**隐式默认值。

**依据**：缺省值会让「没配」与「配成 0」**无法区分** ——
这正是 `nexus-llm::GatewayConfig::from_config` 的纪律（0 值必须回退到缺省）。
把默认值放在**装配层**（P5/P6）而不是**运行时 crate**，让「配置在哪」这个问题只有一个答案。

**可逆性**：高 —— 加一个 `Default` impl 是纯增量。
**影响面**：`nexus-agent` 的构造函数签名。

**反方论点与回应**（决策 rubric §4）：

> 「不内置默认值会让每个调用方都要重复填一遍同样的数字 ——
> 这是**样板代码**，而且极易出现「有的地方填 3，有的地方填 30」的不一致。
> 成熟做法是库给一个保守默认值，调用方按需覆盖。」

**回应**：这条论点**部分成立**，但它误判了这些参数的**性质**。
迭代上限与超时不是「性能调优旋钮」，而是**安全边界** ——
它们的默认值会**决定**系统在什么条件下失控。

默认值的真正风险是**静默降级**：调用方忘了传，拿到一个「看起来在跑、
实际上三轮就停」的运行时，而**没有任何信号**说明这件事发生过。

所以本 crate 提供 `ReactConfig::default()`（**测试与示例**用），
但**运行时路径要求显式构造** —— `ReactLoop::new` 接收的是**完整的**
`ReactConfig`，没有「部分字段缺省」的中间态。

> **落地记录（NEX-29）**：实现时确实遇到了这个问题 ——
> `ReactConfig` 实现了 `Default`，但那只是为了让**测试**少写样板。
> 文档里明确标注了「生产路径必须**显式**构造」。

### 6.5 D7 —— P4 自定义事件 sink

**决策**：P4 定义自己的异步 sink（单点分配序号 + 双通道 + 背压），
**不复用** `nexus-core::EventSink`。

**依据**：见 §3.5 —— `nexus-core` 的是同步单写者，P4 需要并发生产者 + 背压。
强行统一会让其中一个变形。

**反方论点与回应**（决策 rubric §4）：

> 「两套事件 sink 就是**两套语义** —— 同一个 `Event` 类型有两种发法，
> 下游要判断『这个事件该走哪条路』，而架构文档 §6.3 已经把事件契约定稿了。
> 更糟的是：**序号的语义可能不一致** ——
> `nexus-core` 的序号由调用方分配，P4 的由 sink 分配，
> 两条流合并到前端时，序号会**打架**。
> 正确做法是**扩展** `nexus-core::EventSink`，让它支持异步多写者。」

**回应**：这条论点的**前半段**（两套语义）成立，但**结论**不成立。

1. **序号不会打架** —— P4 的 sink 与 `nexus-core` 的 sink **不在同一条流上**。
   P4 的事件由 P4 的 sink 产生，走 P4 的通道；`nexus-core` 的 sink 服务的是
   P1/P2/P3 的**单写者**流程。两者**从不合并**。
   真正需要合并时（P5 的 SSE 桥接），那是**另一个任务**要解决的问题 ——
   届时应该由 P5 决定「前端如何看到两条流」，而不是让 P4 提前变形。

2. **扩展 `nexus-core::EventSink` 会破坏已定稿条款** ——
   架构文档 §6.3 把「单写者事件流」列为已定稿，且 §2.2.3 的核心不变量是
   「序号必须严格单调、无重复、无空洞，且与写入顺序一致」。
   把 `&mut self` 的同步写入改成异步多写者，会让「单写者」这个
   **编译期保证**变成运行时约定 —— 那正是 §3.2.3 批评的做法。

3. **「复用」的定义要清楚** —— P4 **复用了事件负载类型**
   （`nexus_core::EventPayload`），所以**前端协议没有分叉**。
   分叉的只是**发送机制**，而发送机制是**内部实现细节**。

**可逆性**：中 —— sink 是内部实现细节，但事件**负载类型**（`nexus-core::EventPayload`）是共享的。
**影响面**：`nexus-agent` 的事件模块。

> **落地记录（NEX-28）**：D7 的实现里冒出一个**方案 C 的变体** ——
> 参考架构 §2.2 的字面描述是「两个 output 通道 + 消费端 `select` 合并」，
> 但那会**破坏序号单调**（可丢通道排着 `seq=5`、必达通道排着 `seq=6`，
> 消费端可能先读到 6）。
> 实际实现是**单一 ingress + 单一 output + 两个信号量**：
> 序号按入队顺序分配（全局 FIFO），消费端看到的序号严格递增，
> 而「可丢」的积压**不挤占**「必达」的容量。
>
> 这也印证了 §3.2.3 的批评：**「保留槽位」是运行时约定**，
> 而**两个信号量**是把容量语义**在类型上分开**。

---

## 7. 验证方式

```bash
# 门禁
bash scripts/check-lock.sh      # 依赖锁定纪律
bash scripts/check-deps.sh      # crate 依赖方向铁律
bash scripts/check-naming.sh    # 标识符不得含中文
bash scripts/check-backlog.sh   # backlog 组织纪律
bash scripts/check-specs.sh     # Spec 层组织纪律
bash scripts/check-rules.sh     # 规则文件一致性
bash scripts/check-docs.sh      # 文档层一致性
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是**独立 workspace**，验证命令为 `cd examples && cargo test --workspace`。
