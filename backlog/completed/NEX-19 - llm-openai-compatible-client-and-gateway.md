---
id: NEX-19
title: "feat(llm): 落地 OpenAI 兼容供应商客户端与网关回退边界"
status: Done
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

## DoR 判定

**判定时间**：2026-11-05　**判定人**：AI 代理（Level 2 / `ai-with-review`）

### Stage A（确定性判据）

- 门禁 1（AC 可二元测试）：通过 —— 12 条 AC，均可写成「命令通过 / 断言成立」。
  数量在 1–20 区间，无空 AC。
- 门禁 2（无未决标记）：通过 —— 正文（去 frontmatter）无
  `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `we'll figure out` / `decide later` / `up to the dev`。
- 门禁 3（引用可解析）：通过 —— 三个 `references` 路径均存在：
  `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`（§2.3.2 与 §3.3 均存在）、
  `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md`（§3.1.5 存在）、
  `docs/operations/p3-plan.md`。
- 门禁 4（范围有界）：**语义判定，见 Stage B**。
- 门禁 5（受影响面被指名）：**语义判定，见 Stage B** —— 判定记录在下方「受影响面」小节，
  该小节与本任务同处一文件，随任务一起被评审与归档。
- 门禁 6（完成态可描述）：**语义判定，见 Stage B**。
- 门禁 7（无隐形依赖）：通过 —— 三个依赖 NEX-16 / NEX-17 / NEX-18 已在
  `backlog/completed/`；正文无 `requires` / `depends on` / `after X ships` 表述。

### Stage B（语义判定）

- 门禁 4（范围有界）：通过 —— 改动全部落在 `nexus-llm` 一个 crate 内，
  无跨 crate 契约变更、无新增 workspace 依赖。落地为 4 个新文件 + 2 个新测试文件。
- 门禁 5（受影响面被指名）：通过 —— 受影响面明确如下：

  | 文件 | 性质 |
  |---|---|
  | `crates/nexus-llm/src/provider.rs` | 新增 —— OpenAI 兼容客户端（`chat` / `chat_stream`） |
  | `crates/nexus-llm/src/gateway.rs` | 新增 —— 网关装配与回退边界 |
  | `crates/nexus-llm/src/lib.rs` | 修改 —— 导出新模块 |
  | `crates/nexus-llm/tests/gateway_contract.rs` | 新增 —— 网关行为层测试 |
  | `crates/nexus-llm/tests/provider_contract.rs` | 新增 —— 客户端契约测试（含 mock HTTP server） |
  | `crates/nexus-llm/tests/architecture.rs` | 修改 —— 补结构层断言 |

  **不改**：`nexus-core`（错误码 / 事件 / ID 类型）、`nexus-model`、
  架构文档已定稿条款、根 `Cargo.toml`（不引入新依赖）。

- 门禁 6（完成态可描述）：通过 —— 上线后调用方（`nexus-service` / `nexus-agent`）可以
  用 `Gateway::chat` / `Gateway::chat_stream` 发起一次模型调用，
  遇到上游故障时**自动回退**到备用供应商；流已开始时**拒绝**回退并返回明确错误。

### 契约影响五问（S4 前置，架构文档 §3 落地纪律）

| 问 | 答 |
|---|---|
| 错误码值是否改变？ | **否** —— 只复用 `nexus-core` 已定稿的
  `BudgetExceeded` / `RateLimited` / `ModelUnavailable` / `ModelTimeout` /
  `ProviderNotFound` / `Internal` / `ServiceUnavailable` 七个码，
  落在 `docs/specs/p3/contracts/error-codes.json` 白名单内 |
| 事件名与负载形状是否改变？ | **否** —— 本任务不产生领域事件（`usage` 事件属 `nexus-service` 层） |
| ID 底层类型是否改变？ | **否** —— 本任务不涉及 ID |
| crate 依赖方向是否改变？ | **否** —— `nexus-llm` 仍不依赖 `nexus-repo`；不引入任何新依赖 |
| 对外响应形状是否改变？ | **否** —— `nexus-llm` 尚无对外 HTTP 契约（那是 P5 `nexus-api` 的事） |

### 复杂度打分（`ai-sdlc-autonomy.mdc` §2）

| 维度 | 估计 | 依据 |
|---|---|---|
| 契约影响 | 低 | 不触及错误码 / 事件 / ID 类型 |
| 依赖面 | 中 | 跨 `reqwest` / `tokio` / HTTP 协议形状，但**不跨 crate** |
| 可逆性 | 低 | 纯新增代码，回滚成本低 |
| 探索性 | 中 | mock HTTP server 与 SSE 首事件边界的解析形状需探索 |
| 并发/一致性 | 中 | 流式锁定的时序点（yield 之前）易错 |

**最高分 5（中）→ 路由 `ai-with-review`**：AI 产出 + 人工批准。
**不属 `fully-autonomous`** —— 依赖面与并发一致性两项未取「低」，
按 `ai-sdlc-autonomy.mdc` §2「任一不满足即降级」不得升到完全自治。

**结论：pass**

---

## Acceptance Criteria

- [x] #1 `nexus-llm` 有 OpenAI 兼容客户端：`chat`（非流式）与 `chat_stream`（流式）
- [x] #2 客户端**不查库** —— 描述符由调用方传入
- [x] #3 网关装配顺序为：预算裁剪 → 熔断 → 信号量 → 重试 → 供应商 → fallback
- [x] #4 **预算裁剪在每次尝试都重跑**（含 fallback 尝试）—— 有测试断言
- [x] #5 非流式失败时**可回退**到 fallback 供应商
- [x] #6 **流式首事件前**失败可回退
- [x] #7 **流式首事件后不可回退** —— 有测试断言回退被拒绝（这是 P3 完成判据的核心）
- [x] #8 fallback 与 primary **是同一供应商**时**不切换**
- [x] #9 两边都失败时，错误信息**同时含** primary 与 fallback 的信息
- [x] #10 流式锁定的时序点正确：锁定发生在 **yield 之前**（下游真正 poll 到的时刻）
- [x] #11 客户端用**本地 mock HTTP server** 测试（不依赖真实模型服务）
- [x] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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

### 1. `AppError` 会**丢掉**上游状态码 —— 回退错误因此少了关键信息

`retry_do` 的错误类型是 `AppError`（重试判据取自 `AppError::retry_policy()`）。
但 `AppError` 只有**业务码**，没有「上游返回了 502 还是 503」。

回退要求「两边都失败时错误信息**同时含** primary 与 fallback 的信息」——
如果两边都只剩 `ModelUnavailable`，错误信息就是：

```
主供应商与备用供应商均失败：primary: 上游返回错误；fallback: 上游返回错误
```

**这等于没报** —— 运维看到的是「两个供应商都挂了」，但 502（上游抖动）
与 503（上游过载）的处置完全不同。

修法：在重试的闭包里**额外留一份原始 `LlmError`**（`Arc<Mutex<Option<>>>`），
失败时优先用它汇报。

> 代价：`LlmError` 必须 `Clone` —— 而 `std::io::Error` **不实现 `Clone`**。
> 所以 `UpstreamIo` 变体从持有 `std::io::Error` 改成持有 `String`
> （IO 错误只需要给人看，不需要结构化处理）。

> 教训：**错误的「可组合性」比「信息完整」更难事后补**。
> 一旦错误要穿过 `AppError` 走一圈，细节就永久丢了。

### 2. 变异测试抓到了自己的测试**太弱**

「错误信息含双方」这条断言最初只检查**供应商代号**。
变异测试（把 fallback 的失败原因从错误信息里删掉）**没有让测试变红** ——
因为代号还在。

 Strengthening 后（断言**双方各自的失败原因**），
同样的变异立刻被抓出来。

**这次变异同时暴露了一个真实缺陷**：`LlmError::Upstream` 的 `Display`
只写「上游返回错误」，**不带状态码**。修掉了。

> 教训：**「没被变异测试抓住的断言」等于没有断言**。
> 这条已在 `docs/specs/p3/contracts/invariants.json` 之外的
> 实现记录里留痕 —— 参考 NEX-14 的同类教训。

### 3. 「回退不可用」的判定必须是**类型**上的，不能是运行时标志

参考实现的 `StreamSession` 用 `locked_provider: Option<ProviderId>`
同时表达「选了谁」和「是否已开始」—— 这导致 `fallback()` 之后
`started()` 竟变成 true，自相矛盾（`examples/demo-stream-fallback` 记了这个坑）。

本实现把它拆成**两个类型**：

```rust
pub enum StreamOutcome {
    Stream(StreamHandle),          // 已建立 —— 不可回退
    FailedBeforeStart(LlmError),    // 首事件前失败 —— 可回退
}
```

回退的**唯一入口**是 `FailedBeforeStart`。
要回退就必须先拿到一个「流没建立起来」的错误 ——
编译期就没法在流已建立后回退。

结构层测试 `the_stream_outcome_separates_before_start_from_after_start`
钉住这个形状。

### 4. 结构层扫描**必须剥离注释**

新增的断言「控制面字段不得进入上游请求」第一次跑就**误报**了 ——
因为转换函数的**文档注释**里写着「`dangerous` 绝不发给模型」，
那是说明性文字，不是实际使用。

修法与 `scripts/check-deps.sh` 规则 6b 相同：**先剥离注释再扫描**。

> 这已经是本仓库第三次踩这个坑了。**注释里的关键词会让源码扫描失真** ——
> 扫描类断言必须是「剥注释后扫」。

### 5. `base_url` 的 `/v1` 后缀要**幂等**处理

OpenAI 兼容供应商的 `base_url` 惯例是 `https://host/v1`，
但也有配置写成 `https://host`。

实现里统一成：**已带 `/v1` 的不重复追加，没带的补上**。
否则会出现 `https://host/v1/v1/chat/completions`。

### 交付物

| 项 | 数量 | 位置 |
|---|---|---|
| 上游协议类型 | 8 | `crates/nexus-llm/src/protocol.rs` |
| 协议转换 | 1 | `crates/nexus-llm/src/convert.rs` |
| SSE 解析 | 1 | `crates/nexus-llm/src/sse.rs` |
| 供应商客户端 | 1 | `crates/nexus-llm/src/provider.rs` |
| 网关装配 | 1 | `crates/nexus-llm/src/gateway.rs` |
| 网关行为层测试 | 7 | `crates/nexus-llm/tests/gateway_contract.rs` |
| mock HTTP server | 1 | `crates/nexus-llm/tests/support/mock_server.rs` |
| 结构层断言 | +3 | `crates/nexus-llm/tests/architecture.rs` |

**测试总数**：88（51 单元 + 7 架构 + 3 负向 + 7 契约 + 20 网关）

**变异测试**：2 处注入故障，均被抓出（同供应商切换判定、fallback 错误信息）。
