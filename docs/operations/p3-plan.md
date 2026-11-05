# P3 · LLM 网关

架构文档 §6.1 的 P3 阶段：**`nexus-llm` —— 供应商、预算、重试、熔断**。

**状态：🚧 进行中**

> **本文件是进度视图，不是契约。**
>
> AI-SDLC 三层模型（Tier 1 RFC / Tier 2 Spec / Tier 3 Task）下，
> P3 属于 **Spec 层**（一个特性拆成多个任务）。契约在：
>
> | 产物 | 路径 | 作用 |
> |---|---|---|
> | Spec | [`docs/specs/p3/spec.md`](../specs/p3/spec.md) | 范围、红线、核心契约、已定稿决策 |
> | Tasks | [`docs/specs/p3/tasks.md`](../specs/p3/tasks.md) | 逐任务 AC（spec-kit `v0.8-headings` 布局） |
> | Contracts | [`docs/specs/p3/contracts/`](../specs/p3/contracts/) | 机器可读的不变量与错误码白名单 |
>
> 本文件只回答「**进展到哪了**」。

## 完成判据

架构文档 §6.1：**「预算裁剪与回退边界有测试」**

拆成两个可验证的子目标：

1. **预算裁剪有测试** —— 上下文/输出预算在**调用前**被拒绝，不等上游返回 400。
2. **回退边界有测试** —— 非流式可回退、流式**首事件前**可回退、**首事件后**不可回退。

### 「有测试」的**防伪**要求

沿用 NEX-14 的教训：测试「存在」不等于「真的跑了」。

| 假象形态 | 防线 |
|---|---|
| 预算裁剪只在 happy path 测了「不超预算」 | 必须同时测**超预算被拒**与**边界值恰好通过** |
| 回退只测了「流开始前可回退」 | 必须测「流开始后回退被拒」—— 只测前者等于没测边界 |
| 预算裁剪测试没覆盖 fallback 路径 | 每次尝试（含 fallback）都要**重跑**裁剪 —— 参考实现的明确不变量 |
| 熔断测试只测「打开」不测「预热期」 | 预热期样本不足时**不得**评估错误率 —— 否则窗口零值会被误判为真实失败 |

> 这四条防伪要求已落进 `docs/specs/p3/spec.md` §5.1，
> 并由 `docs/specs/p3/contracts/invariants.json` 的 19 条不变量逐条对应。

## 任务拆解

| 任务 | Spec 层 | 内容 | 依赖 | 对应设计条款 | 状态 |
|---|---|---|---|---|---|
| **NEX-16** | T-001 | `nexus-llm` 的领域类型与供应商描述符 | — | §2.3.2、§2.6.2、§4.2 差异 2 | ✅ 完成 |
| **NEX-17** | T-002 | 上下文预算裁剪（`bound_chat_request`） | NEX-16 | §3.1.1、参考架构 §3.1.1 | ✅ 完成 |
| **NEX-18** | T-003 | 重试（指数退避 + 抖动）与熔断器（含预热期） | NEX-16 | §2.6.3、参考架构 §3.1.2 / §3.1.3 | ✅ 完成 |
| **NEX-19** | T-004 | OpenAI 兼容供应商客户端 + 网关装配 + 回退边界 | NEX-16/17/18 | §2.3.2、§3.3、参考架构 §3.1.5 | ✅ 完成 |
| **NEX-20** | T-005 | P3 收口：架构文档同步 + 熔断粒度声明 + CI 门禁 | NEX-16..19 | §5.2、§5.3、§5.4、§6.1 | ✅ 完成 |

依赖图：

```
NEX-16 / T-001 (类型 + 描述符)
  ├── NEX-17 / T-002 (预算裁剪)
  ├── NEX-18 / T-003 (重试 + 熔断)
  └── NEX-19 / T-004 (供应商客户端 + 网关 + 回退边界)
        └── NEX-20 / T-005 (收口)
```

> **T-NNN 是 Spec 层的编号，NEX-nn 是 Task 层的编号。**
> 两者一一对应，由 `scripts/check-specs.sh` 规则 2 双向校验。

### 已完成部分的交付概览

| 任务 | 交付物 | 验证 |
|---|---|---|
| NEX-16 | 9 个协议类型 + 16 个契约测试 + 3 个负向测试 | `nexus-llm/tests/types_contract.rs`、`tests/compile_fail/` |
| NEX-17 | `bound_chat_request` 纯函数 + 14 个测试 | `nexus-llm/src/budget.rs` |
| NEX-18 | `retry_do` + `CircuitBreaker` + 结构层断言 | `nexus-llm/src/retry.rs`、`src/breaker.rs`、`tests/architecture.rs` |
| NEX-19 | 上游协议类型 + 协议转换 + SSE 解析 + 供应商客户端 + 网关（含回退边界） | `nexus-llm/src/protocol.rs`、`src/convert.rs`、`src/sse.rs`、`src/provider.rs`、`src/gateway.rs` |
| NEX-20 | 架构文档同步（§4.3 / §5.3 / §5.4 / §6.1）+ 熔断粒度声明 + crate 级文档 | `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`、`crates/nexus-llm/src/lib.rs` |

## 两条不可让步的红线

### 1. 契约不可改

`nexus-core` 的 42 个业务码、12 个事件名、18 个 ID 类型**已定稿**（§6.3）。
P3 复用 `ErrorCode::ModelUnavailable` / `ModelTimeout` / `BudgetExceeded` / `RateLimited` / `ProviderNotFound` —— **不得新增码值**。

> `nexus-llm` 的领域错误是 `nexus-core::ModelError` 的**扩展**，不是替换。

### 2. 熔断是**进程内**状态 —— 且必须写进文档

参考实现的熔断是进程内（`internal/llm/fallback.go:4-5`），
多副本部署时会出现「A 副本熔断、B 副本继续打」的不一致
（参考架构文档 §5.3 局限 #3）。

**本阶段决策：进程内实现 + 文档显式声明「熔断是单副本粒度」。**

这不是「照抄局限」，而是**明确表态**：
外置到 Redis/PG 属于后续特性的范围，不在 P3。

> **决策类别：(a) decide-and-proceed** —— 可低成本回滚、影响面小、不触碰信任链与治理控制。

## 三条已定稿的技术决策

| 决策 | 选择 | 依据 |
|---|---|---|
| 供应商多态形态 | **enum + match**，不是 `Box<dyn>` | §2.3.2 —— 穷尽匹配 + 无虚调用 + 错误类型精确 |
| 供应商协议范围 | **只做 OpenAI 兼容**（deepseek / openai / moonshot / ollama 同一协议） | 本次决策；ollama 只是 `base_url` 变体 |
| 熔断状态 | **进程内**（`std::sync::Mutex` + 环形缓冲） | 本次决策 + 参考架构 §3.1.3 |

### 「enum 优于 dyn」在 P3 的具体落法

```rust
pub enum ModelBackend {
    OpenAiCompatible(OpenAiCompatibleClient),
}
```

供应商数量少，且**不需要**把不同实现放进同一个异构集合 —— §2.3.2 的判据
「需要 dyn」的三条（配置驱动 / 放进同一集合 / 跨 crate 的 `Box<dyn Trait>`）**都不成立**。

> ⚠️ 但「配置驱动」这一条**表面上成立** ——
> 用户在管理后台配置「用哪个模型」，运行时才知道是 deepseek 还是 ollama。
>
> **§2.3.2 明确回答了这一点**：用 enum + match，不用 `dyn`。
> 「运行时才知道」由 **enum 的变体**承载，不是由 `dyn` 承载。

## 已知风险与前置条件

| 项 | 说明 | 应对 |
|---|---|---|
| **真实上游不可用** | P3 不依赖真实模型服务 | 供应商客户端用**本地 mock HTTP server** 测（wiremock 不在依赖里 → 用 `tokio` 手写最小 server） |
| **`reqwest` 的流式解析** | SSE 分帧易错 | 流式解析留到 P5（SSE 桥接）；P3 的流式只到「首事件边界」这一层 |
| **token 估算是启发式** | 参考实现自己标注「不是厂商计费或上下文保证」 | 预算裁剪用**保守的字节 + framing 估算**，并在文档里写明这是保守上界而非精确 token 数 |
| **重试的抖动需要随机源** | `rand` 已在 workspace 依赖里 | 用 `rand::random_range`（rand 0.10 API） |

## 验证方式

```bash
# 门禁（七道脚本 + 编译三连）
bash scripts/check-lock.sh && bash scripts/check-deps.sh \
  && bash scripts/check-naming.sh && bash scripts/check-backlog.sh \
  && bash scripts/check-specs.sh && bash scripts/check-rules.sh \
  && bash scripts/check-docs.sh \
  && cargo fmt --all --check \
  && cargo clippy --workspace --all-targets --locked -- -D warnings \
  && cargo test --workspace --locked
```

`scripts/check-specs.sh` 是 **Spec 层组织纪律**门禁（新增）：

| 规则 | 拦什么 |
|---|---|
| 1. Spec 三件套齐全 | 只做 Task 层、跳过 Spec 层 |
| 2. `tasks.md` ↔ backlog 双向对应 | 文档漂移（任务被删但文档还在 / 任务没登记进 Spec） |
| 3. 契约引用的测试必须存在 | 「空头支票」契约 —— 声称有测试验证，实际没有 |
| 4. 错误码白名单与 `nexus-core` 一致 | §6.3 已定稿条款漂移（改码值 / 加码值） |
| 5. `nexus-llm` 只用白名单内的码 | 偷偷新增业务码 |

> 三条防伪要求（NEX-14 的教训）同样适用于这个门禁：
> **门禁本身必须被验证过会红**。已用注入故障验证：
> 篡改码值 / 引用不存在的测试 / 引用不存在的 backlog 任务 —— 三条均被抓住。

## P3 完成状态（2026-11-05）

P3 **全部五个任务已完成**（NEX-16..20），完成判据「**预算裁剪与回退边界有测试**」成立。

| 判据 | 承担者 | 验证方式 |
|---|---|---|
| 预算裁剪有测试 | NEX-17 | 14 个单元测试 + P3-I01..I05 |
| 回退边界有测试 | NEX-19 | 7 个网关行为层测试 + 3 个结构层断言 + P3-I06..I09 |

**19 条不变量逐条指向真实存在的测试名**（`contracts/invariants.json`），
由 `scripts/check-specs.sh` 规则 3 持续校验。

### P3 **没有**覆盖的范围

写清楚这些，避免读者把「P3 完成」误读成「模型网关完备」：

| 项 | 为什么不在 P3 | 何时补 |
|---|---|---|
| **真实上游连通性** | 不依赖真实模型服务 | 集成环境（需 API key） |
| **成本核算的准确性** | token 估算是保守上界，不是厂商计费口径 | P4（用量对账） |
| **跨副本熔断一致性** | 熔断是进程内状态，单副本粒度（决策 D3） | 容量治理阶段（外置到 Redis/PG） |
| **ollama 原生协议** | 与 OpenAI 兼容是两套字段形状 | 需要时（决策 D2） |
| **SSE 完整分帧** | 属 P5 的 SSE 桥接 | P5 |
| **跨副本配额协调** | 属容量治理（`nexus-capacity`） | 容量治理阶段 |
| **`UnknownOutcome` 的端到端验证** | 「请求是否已到达上游」在网关层**不可观测** | P6（持久执行） |
| **enum 优于 `dyn` 的性能收益** | P3 只验证了**类型形状**，没实测性能 | 性能验证阶段（§5.6） |

