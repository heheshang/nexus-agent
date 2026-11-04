---
id: NEX-18
title: "feat(llm): 落地重试退避与熔断器"
status: Todo
assignee: []
labels: [llm, retry, circuit-breaker, p3]
priority: high
dependencies:
  - NEX-16
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.1.2"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.1.3"
  - "docs/operations/p3-plan.md"
---

## Description

**P3 完成判据的第二块基石：重试与熔断。**

这两者都是「失败语义」的实现 —— 架构文档 §2.6.3 说「失败分类是第一公民」。

### 重试（指数退避 + 抖动）

参考实现 `internal/llm/retry.go:24`：

- 默认 **3 次**尝试、**100ms** 基础延迟、**2s** 上限
- 延迟 `base_delay << attempt`，超过上限则截断，再加 `rand.Int63n(delay/4)` 的抖动
- `ctx.Done()` 立即中止等待
- **非可重试错误立即返回**

**抖动是必需的** —— 没有抖动，多个并发请求会在同一时刻重试，形成同步风暴。

**重试判据来自 `nexus-core` 的 `RetryPolicy`**（§2.6.3）：
- `Retryable` → 可自动重试
- `UnknownOutcome` → **禁止**自动重试（请求可能已到达上游并产生副作用，重试会重复扣费）
- `Terminal` → 重试无意义

> 这正是本设计相对参考实现的**关键改进**：
> 参考实现的 `IsRetryable` 把 504（超时，可能已执行）与 502（明确拒绝）一视同仁。
> `nexus-core` 的 `RetryPolicy` 区分二者 —— **不要**在 `nexus-llm` 里另写一套判断。

### 熔断（环形缓冲 + 预热期）

参考实现 `internal/llm/fallback.go:13`：

- 环形缓冲保存最近 N 次调用结果（默认 N=20）
- **预热期**：`filled < size` 时**不评估**错误率
  （注释原文：「预热期：样本不足不评估（窗口零值 false 不代表真实失败）」）
- 达到阈值 → 打开 `open_duration`（默认 60s），并**重置窗口为全 true**、`filled = 0`，
  冷却结束后自然进入半开试探
- `allow()` 只看「当前时间 > 打开截止时刻」

**熔断是进程内状态**（本阶段的明确决策）—— 必须在文档与公开接口注释里写明
「熔断是单副本粒度」。

### 取消不计入熔断样本

参考实现 `internal/llm/gateway.go:346-349`：调用方取消**不计入**熔断样本。
这是熔断正确性的关键 —— 否则用户取消会误伤供应商。

## Acceptance Criteria

- [ ] #1 有 `retry_do` 等价实现：指数退避 + **抖动** + 取消立即中止
- [ ] #2 重试判据用 `nexus-core` 的 `RetryPolicy` —— `Retryable` 才重试，`UnknownOutcome` / `Terminal` **立即返回**
- [ ] #3 有测试断言 `UnknownOutcome`（如 `ModelTimeout`）**不重试** —— 这是本设计相对参考实现的**关键改进**，必须被测试钉住
- [ ] #4 退避延迟有**上限**（默认 2s），且**抖动**存在（有测试断言延迟不全相同，或抖动范围被断言）
- [ ] #5 等待退避期间**取消能立即中止**（不等到延迟结束）
- [ ] #6 熔断器用 `std::sync::Mutex`（**不是** `tokio::sync::Mutex`）—— 熔断操作是纯同步的、不跨 await
- [ ] #7 熔断器有**预热期**：样本数不足时**不评估**错误率，不因窗口零值误判为失败
- [ ] #8 熔断打开后**拒绝**调用；冷却结束后**允许**试探（半开）
- [ ] #9 打开熔断时窗口被**重置**（`filled = 0`）
- [ ] #10 取消**不计入**熔断样本（调用方取消不污染错误率）
- [ ] #11 熔断状态的**单副本粒度**写进公开接口的文档注释与 crate 级文档
- [ ] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## 备注

- 抖动用 `rand::random_range`（rand 0.10 API）—— `rand` 已在 workspace 依赖里。
  ⚠️ **实测确认 API 形状**（`RngExt` trait 上的方法），不要照抄旧版 rand 的 `thread_rng().gen_range()`。
- `tokio::select!` 里的 `biased;` 很重要 —— 保证取消分支**优先于**业务分支被检查。
  否则在高负载下取消可能被饿死。
- 熔断器**不是** async —— `allow()` / `record()` 都是同步方法。
  用 `std::sync::Mutex` 还有一个好处：编译器会强制你**不在持锁时 await**
  （guard 非 `Send`）。
- 熔断器的 `record` 要能区分「成功 / 失败 / 取消」三态 ——
  取消**不记为失败**（AC #10）。
- 本任务**不实现**供应商客户端与网关装配（属 NEX-19）——
  但重试与熔断的**可测性**要留出来：熔断器可以直接用单测驱动（不需要真的 HTTP 服务）。

## 实现过程中的发现

（待补）
