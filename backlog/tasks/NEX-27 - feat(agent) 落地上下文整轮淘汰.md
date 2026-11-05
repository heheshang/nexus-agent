---
id: NEX-27
title: "feat(agent): 落地上下文整轮淘汰"
status: To Do
assignee: []
labels: [agent, compaction, budget]
priority: medium
dependencies:
  - NEX-26
references:
  - "docs/specs/p4/spec.md §3.2、§6.2"
  - "docs/specs/p4/tasks.md T-002"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.2.1"
---

## Description

**决策 D5 的落地**：上下文超出预算时，按**整轮**删除历史，**不生成摘要**。

### 为什么整轮淘汰，而不是按条裁剪

按条裁剪会留下**孤立**的 `tool_result` —— 一个没有对应 `tool_call` 的工具结果。
那直接**破坏模型协议**：模型看到一条无主的工具结果，无法判断它对应哪次调用。

参考架构 §2.2 的取舍说明原文：「它比『按条裁剪』正确得多 —— 按条裁剪会留下孤立 tool_result，
直接破坏模型协议」。

**一轮**的完整定义：一个 user 消息 + 其后的 assistant 消息 + **所有**并行 tool 结果。

### 为什么**不**生成摘要

这是 OQ-1 的决策。三条依据（详见 Spec §6.2）：

1. **可验证性** —— 整轮淘汰的每条不变量都能写成二元断言；
   摘要的正确性**在原理上无法断言** —— 我们只能断言「调用了摘要模型」，
   不能断言「摘要没丢事实」。
2. **事实漂移** —— 摘要错了，模型就基于错误事实推理。
3. **注入面** —— 工具输出是**不可信输入**；摘要把「被丢弃的数据」变成「被保留的上下文」，
   是注入面的**放大**。

### 保留什么

| 保留 | 理由 |
|---|---|
| 所有 system 消息 | system 是**指令**，不能丢 |
| 首个 user 之前的前缀 | 那是运行开始时的上下文设定 |
| 最新一轮 user | 当前要回答的问题 |

### 裁剪的时机

参考架构 §2.2 的 `BeforeModelRewriteState`：
「**每步**模型调用前按 `InputTokenBudget` 重新裁剪历史」。

**每步都重跑** —— 因为工具结果会**增长**历史，
第 1 步裁剪过的历史在第 3 步可能又超预算了。

## 范围边界

### 做

- `compact_input_history` 纯函数
- 整轮淘汰的算法
- 预算估算（消息 + 工具定义）
- 裁剪后复核

### 不做

| 项 | 理由 |
|---|---|
| 摘要压缩 | 决策 D5 明确排除 |
| `nexus-llm::bound_chat_request` 的改动 | 那是**网关层**的预算裁剪（输入侧），本任务是 **Agent 层**的**历史**裁剪，两者不重叠 |
| 真实 tokenizer | 参考架构 §3.2.5 明说可选接，但要接受「tokenizer 与厂商不一致」的新不确定性来源；P4 用**保守估算** |

## Acceptance Criteria

- [ ] #1 `crates/nexus-agent/src/compaction.rs` 实现 `compact_input_history` —— **纯函数**，不修改入参
- [ ] #2 保留**所有** system 消息
- [ ] #3 保留**首个 user 之前的前缀**
- [ ] #4 保留**最新一轮 user** 轮
- [ ] #5 历史 user 轮按**整轮**删除 —— 该轮的 assistant 消息与**所有**并行 tool 结果**一起**被删，不留孤立 tool_result
- [ ] #6 **不生成摘要** —— 保留的消息是原样的，未新增任何摘要消息
- [ ] #7 裁剪后**复核**协议完整性 —— 裁剪结果仍然配对完整
- [ ] #8 仍超预算时返回 `ErrorCode::BudgetExceeded`（30006）
- [ ] #9 边界值双向测试：恰好在预算内通过、超出即拒绝
- [ ] #10 工具定义计入预算
- [ ] #11 配对校验**先于**裁剪 —— 配对断裂时即使超预算，报的也是配对错误
- [ ] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「某个函数保留/删除了什么」或「返回什么错误码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

「用摘要还是整轮淘汰」是 OQ-1，已由操作者**落定**（决策 D5）——
落定的依据与反方论点写在 Spec §6.2，**不是**未决问题。

「用真实 tokenizer 还是保守估算」是**显式排除**（范围边界表），
排除的代价已写进 AC #9 的双向边界测试。

### #3 引用可解析

- `docs/specs/p4/spec.md §3.2、§6.2` —— 存在
- `docs/specs/p4/tasks.md T-002` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.2.1` —— 存在

### #4 范围有界

单 crate 改动（`nexus-agent`）。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-agent/src/compaction.rs` | **新增** —— 裁剪实现 |
| `crates/nexus-agent/src/protocol.rs` | **只读** —— 复用配对校验（T-001 交付） |

**不新增依赖。**

### #6 完成态可描述

ReAct 循环每步都能拿到一个**在预算内**且**协议完整**的历史；
旧轮次按整轮丢弃，最新一轮与 system 指令保留。

### #7 无隐形依赖

- NEX-26 已登记在 `backlog/tasks/`，交付协议完整性校验（本任务 AC #7 / #11 依赖它）
- `nexus-core` / `nexus-llm` **已**在 `nexus-agent` 的依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，**但需 NEX-26 完成后开工**（门禁 7 依赖已登记的上游任务）。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：整轮淘汰，**不**生成摘要。
- **依据**：可验证性（摘要的正确性不可断言）/ 避免事实漂移 / 不放大 prompt injection 面。
  详见 `docs/specs/p4/spec.md` §6.2，含反方论点与回应。
- **可逆性**：中 —— 换成摘要需要**重写**裁剪函数与相关不变量测试，但不影响其他契约。
- **影响面**：`crates/nexus-agent/src/compaction.rs` 及其不变量测试。

### 类别：decide-and-proceed

- **决策**：预算用**保守估算**（序列化字节 + framing），不接真实 tokenizer。
- **依据**：参考架构 §3.2.5 —— 真实 tokenizer 会引入「tokenizer 与厂商不一致」的新不确定性来源。
  与 `nexus-llm::budget` 的既有口径一致（保守上界，不是精确 token 数）。
- **可逆性**：中 —— 换 tokenizer 会改变预算判定结果。
- **影响面**：预算估算函数。

---

## 实现过程中的发现

（待补）
