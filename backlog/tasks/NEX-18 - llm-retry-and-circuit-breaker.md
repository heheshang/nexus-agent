---
id: NEX-18
title: "feat(llm): 落地重试退避与熔断器"
status: Done
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
- 达到阈值 → 打开 `open_duration`（默认 60s），并**重置窗口**、`filled = 0`
- 冷却结束后自然进入半开试探

**熔断是进程内状态**（本阶段的明确决策）—— 必须在文档与公开接口注释里写明
「熔断是单副本粒度」。

### 取消不计入熔断样本

参考实现 `internal/llm/gateway.go:346-349`：调用方取消**不计入**熔断样本。
这是熔断正确性的关键 —— 否则用户取消会误伤供应商。

## Acceptance Criteria

- [x] #1 有 `retry_do` 等价实现：指数退避 + **抖动** + 取消立即中止
- [x] #2 重试判据用 `nexus-core` 的 `RetryPolicy` —— `Retryable` 才重试，`UnknownOutcome` / `Terminal` **立即返回**
- [x] #3 有测试断言 `UnknownOutcome`（如 `ModelTimeout`）**不重试** —— 这是本设计相对参考实现的**关键改进**，必须被测试钉住
- [x] #4 退避延迟有**上限**（默认 2s），且**抖动**存在（有测试断言延迟不全相同，或抖动范围被断言）
- [x] #5 等待退避期间**取消能立即中止**（不等到延迟结束）
- [x] #6 熔断器用 `std::sync::Mutex`（**不是** `tokio::sync::Mutex`）—— 熔断操作是纯同步的、不跨 await
- [x] #7 熔断器有**预热期**：样本数不足时**不评估**错误率，不因窗口零值误判为失败
- [x] #8 熔断打开后**拒绝**调用；冷却结束后**允许**试探（半开）
- [x] #9 打开熔断时窗口被**重置**（`filled = 0`）
- [x] #10 取消**不计入**熔断样本（调用方取消不污染错误率）
- [x] #11 熔断状态的**单副本粒度**写进公开接口的文档注释与 crate 级文档
- [x] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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

### 1. `std::sync::Mutex` **不可重入** —— 自锁是真实风险

最初把「窗口有效长度」抽成了一个方法 `window_len()`，它内部再加一次锁。
而调用点 `record()` **已经持着锁**。

`std::sync::Mutex` 不可重入 → **自锁死锁**。

`tokio::sync::Mutex` 也是不可重入的，但那个死锁发生在 async 上下文，
栈帧不显眼；`std` 版本在同步上下文里自锁，症状是「测试直接卡死」。

**修法**：在已持锁的路径里**只读 `inner` 本身**，不调用任何会再加锁的方法。

> 通用教训：抽「读字段」的方法时要问一句 —— **它会不会再加锁？**
> 这与 NEX-11 的「锁解决并发、删除解决污染」是同一类直觉：
> **能少一层抽象就少一层**，尤其在持锁路径上。

### 2. 熔断器的 `Clone` 必须**共享状态** —— 否则「同一供应商共用一个熔断器」不成立

最初给 `CircuitBreaker` 手写了一个 `Clone` 实现，克隆出**独立的** `Inner`。

那么 `BreakerRegistry::get_or_create` 每次返回的克隆体**各有各的窗口** ——
「同一个供应商共用一个熔断器」这个核心语义就**静默失效**了：
A 副本（其实是同一个进程内的两个克隆体）各算各的错误率，
熔断会「对同一个供应商各算各的」。

**修法**：状态装在 `Arc<Mutex<Inner>>` 里，`Clone` 派生即可 —— 天然共享。

并加了一条测试 `a_cloned_breaker_shares_the_same_state` **钉死**这个语义，
以及一条并发测试 `recording_from_multiple_threads_is_safe`。

> 这类 bug **不会报错** —— 只是行为不对。
> 「克隆共享状态」这种语义必须由**测试**钉住，靠代码评审的自觉是不够的。

### 3. 「成功不进熔断样本」是个**错误的直觉**

我最初写测试时断言「只有失败计入样本」—— 写完发现不对：
**成功也进窗口**，它是「一次成功调用」的样本，作用是**拉低错误率**。

真正**完全不进窗口**的只有两类：

- **取消** —— 用户取消不是供应商的过错（AC #10）
- **熔断拒绝** —— 我们**主动**拒绝的，不是供应商失败

所以 `CallOutcome` 有四个变体（`Success` / `Failure` / `Canceled` / `Rejected`），
而不是三个。区分「成功进窗口」与「取消不进窗口」是这条不变量的核心。

### 4. 预热期的语义要说清楚，否则容易被误改

参考实现的注释原文：「预热期：样本不足不评估（窗口零值 false 不代表真实失败）」。

窗口里的初始值都是 `false`（成功），所以**初始错误率是 0**——
如果样本不足就评估，初始状态**不会**误判为失败。
但「样本不足就不评估」是**更安全**的规则：它保证熔断只由**足够多的实际样本**驱动。

已用**变异测试**验证这条断言有效：去掉预热期检查后，
6 个熔断测试立刻变红。

### 5. `tokio::sync::Mutex` 不会立刻报错 —— 所以必须有结构层断言

用 `tokio::sync::Mutex` 替代 `std::sync::Mutex` **不会编译失败**：
它的 guard 是 `Send`，允许跨 await。代价是**悄悄失去**「编译器强制你不在持锁时 await」
这个保护 —— 锁在 await 期间被释放，另一个 task 拿到锁时状态已经变了。

这是**静默的**行为退化，所以必须有**结构层断言**钉住：
`the_breaker_uses_a_standard_mutex_not_a_tokio_mutex` 扫描源码。

> 断言扫描时**必须剥离注释** —— 文档注释里解释「为什么不用 `tokio::sync::Mutex`」
> 是正常文字，不是实际使用。
> 这与 `scripts/check-deps.sh` 规则 6b 的手法一致（那里踩过同样的坑）。

### 6. 变异测试 —— 验证测试真的在测东西

沿用 NEX-12 / NEX-13 的做法：

| 变异 | 被哪个测试抓住 |
|---|---|
| 去掉预热期检查 | 6 个熔断测试变红 |
| 让 `UnknownOutcome` 也重试（退回参考实现的缺陷） | `unknown_outcome_is_not_retryable` + `unknown_outcome_is_not_retried` 变红 |

### 7. 未做的事（明确划界）

- **不实现**供应商客户端与网关装配（属 NEX-19）。
- **不实现**跨副本的熔断状态外置 —— 那是后续特性，
  本阶段明确声明「熔断是单副本粒度」。
- **不改** `nexus-core` 的任何错误码 —— 重试判据直接复用 `RetryPolicy`，
  `nexus-llm` **不另写一套**。
