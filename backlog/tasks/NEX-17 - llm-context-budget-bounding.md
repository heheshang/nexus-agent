---
id: NEX-17
title: "feat(llm): 落地上下文预算裁剪"
status: Todo
assignee: []
labels: [llm, budget, p3]
priority: high
dependencies:
  - NEX-16
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.1.1"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.1.1"
  - "docs/operations/p3-plan.md"
---

## Description

**P3 完成判据的第一半：预算裁剪有测试。**

参考实现的 `boundChatDescriptor`（`internal/llm/context_budget.go:14`）在
**每次模型尝试之前**（含 fallback 与后续 tool_result 轮）重新执行预算裁剪。

### 参考实现的裁剪算法（逐条照搬语义）

1. **拷贝**描述符（不修改原值）—— Rust 版返回**新描述符**（所有权决定）。
2. 确定输出上限：
   - `limit = desc.max_output`（0 则回落 `desc.max_tokens`）
   - `requested = req.max_tokens`（0 则默认 1024，且不超过 `context_window / 4`）
   - 最终 `requested = min(requested, limit)`
3. 若 `context_window > 0`：
   - 把消息与工具序列化为 OpenAI 兼容请求体
   - 加上 framing 开销 `64 + 16 * len(messages) + 32 * len(tools)`
   - 若 `requested >= context_window` 或 `len(raw) > context_window - requested - framing`
     → `ErrBudget`

### 关键语义：**预算不足在调用前被拒**

不是在等上游返回 400 —— 这让预算错误可以在**重试之前短路**
（重试同样会超预算，重试是浪费）。

### 为什么用「字节 + framing」而不是精确 token

参考实现的注释原文（`context_budget.go:10-13`）：

> 使用序列化 UTF-8 字节 + framing 作为保守本地预算，而不是精确 / 厂商无关的 token 计数

本设计沿用这个**保守估算**：
- 序列化后的字节数是 token 数的**上界**（token 至少 1 字节）
- framing 是每条消息 / 每个工具的协议开销

**必须在文档里写明**：这是**保守上界**，不是精确 token 数；
它保证「不会超」，但不保证「充分利用」。

## Acceptance Criteria

- [ ] #1 有纯函数 `bound_chat_descriptor(desc, req) -> Result<BoundedRequest, AppError>`，**不修改**入参
- [ ] #2 输出上限计算符合参考语义：`requested = min(req.max_tokens 或默认, desc.max_output 或 desc.max_tokens)`，且 `requested` 不超过 `context_window / 4`
- [ ] #3 framing 开销为 `64 + 16 * len(messages) + 32 * len(tools)`，与参考实现一致
- [ ] #4 预算不足时返回 `ErrorCode::BudgetExceeded`，且**不发起任何上游请求**
- [ ] #5 预算错误是 **`Terminal`**（不可重试）—— 已有 `nexus-core` 的 `retry_policy()` 保证，**有测试断言**
- [ ] #6 **边界值测试**：恰好等于预算上限 → **通过**；超出 1 字节 → **拒绝**
- [ ] #7 **超预算被拒**的测试与**不超预算通过**的测试**都有**（不能只测一边）
- [ ] #8 `context_window == 0` 时**不裁剪**（0 表示不限）
- [ ] #9 工具定义计入预算
- [ ] #10 裁剪是**纯函数** —— 有测试断言同样的输入产生同样的输出，且入参未被修改
- [ ] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## 备注

- 裁剪返回**新描述符 / 新请求**（参考实现也是「拷贝描述符，不修改原值」）——
  Rust 版的所有权让这一点成为自然结果，不需要 `clone()`。
- **每次尝试都要重跑**裁剪 —— 包括 fallback 与后续 tool_result 轮。
  这条不变量在 NEX-19 的网关装配里落地，本任务提供纯函数。
- 错误码用 `nexus-core` 的 `ErrorCode::BudgetExceeded`（30006）——
  **不新增码值**（§6.3 已定稿）。
- 「预算不足不重试」这一点要与 `nexus-core` 的 `RetryPolicy` 对齐 ——
  `BudgetExceeded` 的 `retry_policy()` 已经是 `Terminal`，本任务只需**断言**它，
  不要在 `nexus-llm` 里另写一套重试判断。
- **反例必须测**：请求体本身就已超过窗口（`requested >= context_window`）→ 拒绝。
  这与「请求体 + framing 超出剩余额度」是**两个不同的拒绝条件**。

## 实现过程中的发现

（待补）
