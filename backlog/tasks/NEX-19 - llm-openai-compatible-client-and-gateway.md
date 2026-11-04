---
id: NEX-19
title: "feat(llm): 落地 OpenAI 兼容供应商客户端与网关回退边界"
status: Todo
assignee: []
labels: [llm, provider, gateway, fallback, p3]
priority: high
dependencies:
  - NEX-16
  - NEX-17
  - NEX-18
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.3.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.1.5"
  - "docs/operations/p3-plan.md"
---

## Description

**P3 完成判据的第二半：回退边界有测试。**

本任务把前三个任务的机制装成网关，并实现 OpenAI 兼容的供应商客户端。

### 供应商范围（本次决策）

**只做 OpenAI 兼容协议** —— deepseek / openai / moonshot 等都是同一套协议；
本地 ollama 也是同一协议的 `base_url` 变体（`/v1` 路径 + OpenAI 兼容端点）。

不做 ollama 原生协议（`/api/chat`）—— 那是**另一套**字段形状，
在只有一个供应商时引入它没有收益。

### 网关调用管道（参考架构 §2.4）

```
Chat / ChatStream
  → 预算裁剪（每次尝试都重跑，含 fallback）
  → 熔断 allow?  → 拒绝 → 走 fallback
  → 供应商并发信号量
  → 重试（指数退避 + 抖动）
  → OpenAI 兼容供应商
  → 失败 → fallback（且 provider 与 primary 不同才切）
```

**顺序有讲究**：熔断在最外层，避免已知故障的供应商消耗配额。

### 回退的**边界**（本任务的核心）

| 场景 | 能否回退 | 依据 |
|---|---|---|
| 非流式调用失败 | ✅ 可回退 | 参考实现 `gateway.go:133-142` |
| 流式：**首事件到达前**失败 | ✅ 可回退 | 参考实现 `gateway.go:180-190` |
| 流式：**首事件之后**失败 | ❌ **不可** | 已经吐出的 token 无法撤回 |

架构文档 §3.3 明确：**流式响应一旦开始向客户端发送，就不能回退到另一个模型** ——
客户端已经收到一半内容，切换模型会产生内容拼接错乱。

**Rust 特有的陷阱**：`Stream` 是**惰性**的 —— 「已经发给客户端了」不是「调用了 `send`」，
而是「下游 poll 到了这个 item」。所以锁定点必须在**实际 yield 之前**。

> `examples/demo-stream-fallback` 已经验证过这个形状 ——
> **保留 demo 作为回归基准**，生产实现把它的形状搬进 `nexus-llm`。

### fallback 的另一个条件

参考实现只在 **fallback 的 provider 与 primary 不同**时才切换。
两边都失败 → 错误信息**同时含** primary 与 fallback 的信息。

## Acceptance Criteria

- [ ] #1 `nexus-llm` 有 OpenAI 兼容客户端：`chat`（非流式）与 `chat_stream`（流式）
- [ ] #2 客户端**不查库** —— 描述符由调用方传入
- [ ] #3 网关装配顺序为：预算裁剪 → 熔断 → 信号量 → 重试 → 供应商 → fallback
- [ ] #4 **预算裁剪在每次尝试都重跑**（含 fallback 尝试）—— 有测试断言
- [ ] #5 非流式失败时**可回退**到 fallback 供应商
- [ ] #6 **流式首事件前**失败可回退
- [ ] #7 **流式首事件后不可回退** —— 有测试断言回退被拒绝（这是 P3 完成判据的核心）
- [ ] #8 fallback 与 primary **是同一供应商**时**不切换**
- [ ] #9 两边都失败时，错误信息**同时含** primary 与 fallback 的信息
- [ ] #10 流式锁定的时序点正确：锁定发生在 **yield 之前**（下游真正 poll 到的时刻）
- [ ] #11 客户端用**本地 mock HTTP server** 测试（不依赖真实模型服务）
- [ ] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## 备注

- **不引入新依赖**：`wiremock` 不在 workspace 依赖里 → 用 `tokio` 手写最小 mock server
  （`hyper` 已在 workspace 依赖里，但用最简的 `TcpListener` + 手写 HTTP 响应更轻）。
  > 若实现时发现手写 HTTP 太脆弱，**停下并记录**，不要擅自往根 manifest 加依赖
  > （根 manifest 的依赖变更属于跨 crate 的决定，需要任务授权）。
- **流式解析的完整 SSE 分帧留到 P5**（SSE 桥接）。
  本任务的流式只到「首事件边界」这一层 —— 能判断「首事件到达前 / 后」。
- `reqwest` 的流式响应用 `bytes_stream()` —— `bytes` 已在 `nexus-llm` 的依赖里。
- **上游 URL 的 SSRF 风险**：`base_url` 来自配置/数据库，属可信来源；
  但**请求里不回显** `base_url` 细节到用户可见消息（§2.6.5 内部原因不外泄）。
- 客户端的凭据（`api_key`）用 `secrecy::SecretString` 承载（`nexus-security` / `secrecy` 已在依赖里）——
  **`Debug` 不得打印明文**。
- 供应商的**响应体解析失败**属于 `ModelError::Malformed` → `ErrorCode::Internal`。
  不要把它映射成 `ModelUnavailable`（那会让它被自动重试）。

## 实现过程中的发现

（待补）
