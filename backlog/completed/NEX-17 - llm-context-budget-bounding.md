---
id: NEX-17
title: "feat(llm): 落地上下文预算裁剪"
status: Done
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

- [x] #1 有纯函数 `bound_chat_request(desc, req) -> Result<BoundedRequest, AppError>`，**不修改**入参
- [x] #2 输出上限计算符合参考语义：`requested = min(req.max_tokens 或默认, desc.max_output 或 desc.max_tokens)`，且 `requested` 不超过 `context_window / 4`
- [x] #3 framing 开销为 `64 + 16 * len(messages) + 32 * len(tools)`，与参考实现一致
- [x] #4 预算不足时返回 `ErrorCode::BudgetExceeded`，且**不发起任何上游请求**
- [x] #5 预算错误是 **`Terminal`**（不可重试）—— 已有 `nexus-core` 的 `retry_policy()` 保证，**有测试断言**
- [x] #6 **边界值测试**：恰好等于预算上限 → **通过**；超出 1 字节 → **拒绝**
- [x] #7 **超预算被拒**的测试与**不超预算通过**的测试**都有**（不能只测一边）
- [x] #8 `context_window == 0` 时**不裁剪**（0 表示不限）
- [x] #9 工具定义计入预算
- [x] #10 裁剪是**纯函数** —— 有测试断言同样的输入产生同样的输出，且入参未被修改
- [x] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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

### 1. 函数名从 `bound_chat_descriptor` 改成 `bound_chat_request` —— 描述符不是返回值

原计划叫 `bound_chat_descriptor`，因为参考实现叫 `boundChatDescriptor`。

但 Rust 版返回的**不是描述符** —— 它返回 [`BoundedRequest`]：
`ModelDescriptor` 是「模型的**配置**」（供应商、上下文窗口、单价），
而 `BoundedRequest` 是「这次**请求**的裁剪结果」（消息、工具、确定后的输出上限、
估算出的字节数）。

描述符是**输入**，请求是**输出** —— 名字跟着语义走。

### 2. 「上下文窗口」是**总额度**，输出要占掉一部分

参考实现的第 3 步「`requested` 不超过 `context_window / 4`」容易被忽略 ——
它不是随便挑的常数，是**给输出留余量**：
上下文窗口是「输入 + 输出」的总额度，输入占满窗口就没有空间生成输出了。

所以有两个**不同的**拒绝条件：

1. `reserved >= window` —— 预留的输出本身就吃掉了整个窗口
2. `body_bytes + framing > window - reserved` —— 请求体 + framing 超出剩余额度

两个条件各有测试（`output_reserving_the_whole_window_is_rejected` /
`oversized_body_is_rejected_before_any_upstream_call`）——
**只测一个就等于没测边界**。

### 3. 序列化失败**不能 panic** —— 退回字符数

`estimate_body_bytes` 序列化消息与工具时可能失败（某个字段里装了非法 JSON 值）。

那在真正发请求时也会失败，属于**上游请求构造错误** ——
不该在预算阶段 panic。做法是退回字节长度估算。

> 顺带一个 UTF-8 的细节：字符数 ≤ 字节数**不成立**（多字节字符的字节数更多），
> 所以 `chars().count()` 是**下界**，可能低估。
> 宁可低估也不要误拒合法请求 —— 但方向上仍取字节长度（更接近真实请求体）。

### 4. 「纯函数」需要**两个**测试 —— 只测一个测不出突变

`bounding_is_pure` 断言「同输入同输出」，`bounding_does_not_mutate_the_input`
断言「入参未被修改」。

这两个是**不同的**性质 —— 一个函数完全可以原地改入参且返回相同结果，
那样「纯」的断言就通过了。**两个都要测。**

### 5. 14 个测试，全部 AC 有对应

| AC | 测试 |
|---|---|
| #2 | `unspecified_max_tokens_falls_back_to_the_default` / `max_output_is_the_ceiling` / `max_output_zero_falls_back_to_max_tokens` / `requested_output_never_exceeds_a_quarter_of_the_window` |
| #3 | `framing_overhead_matches_the_reference_formula` |
| #4 | `oversized_body_is_rejected_before_any_upstream_call` |
| #5 | `budget_error_is_terminal_not_retryable` |
| #6 | `a_body_that_exactly_fits_is_accepted` / `one_byte_over_the_limit_is_rejected` |
| #7 | 两个方向的测试都有（上表） |
| #8 | `unknown_context_window_means_no_bounding` |
| #9 | `tools_contribute_to_the_budget` |
| #10 | `bounding_is_pure` / `bounding_does_not_mutate_the_input` |
| 拒绝条件二 | `oversized_body_is_rejected_before_any_upstream_call` |

> 契约侧对应 `docs/specs/p3/contracts/invariants.json` 的 `P3-I01` ~ `P3-I04`。

### 6. 未做的事（明确划界）

- **不实现**网关装配 —— 「每次尝试都重跑裁剪」在 NEX-19 落地。
- **不裁剪历史** —— 本阶段只确定**输出上限**，消息序列原样保留。
- **不引入分词器** —— 真实的 token 计数依赖厂商分词器，
  那会引入「分词器与厂商不一致」的新不确定性来源。

