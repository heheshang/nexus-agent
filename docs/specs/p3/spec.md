# Spec · P3 LLM 网关

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 权威顺序：架构文档 > 本 Spec > `backlog/tasks/*`（Task 层）。
> 本文件是 **契约**，不是进度记录 —— 进度看 `docs/operations/p3-plan.md`。

**特性**：`nexus-llm` —— 供应商接入、上下文预算、重试、熔断、失败转移。
**架构文档阶段**：§6.1 的 P3 行。
**完成判据**：**预算裁剪与回退边界有测试**（拆成两个可验证子目标，见 §5）。

---

## 1. 范围

### 1.1 在范围内

| 项 | 说明 |
|---|---|
| 网关协议类型 | `Message` / `Role` / `ToolCall` / `ToolDef` / `ChatRequest` / `Usage` / `ChatResponse` / `StreamEvent` / `ModelDescriptor` |
| 上下文预算裁剪 | 调用**前**拒绝超预算请求，纯函数 |
| 重试 | 指数退避 + 抖动，判据取 `nexus-core` 的 `RetryPolicy` |
| 熔断 | 环形缓冲 + 预热期，进程内状态 |
| 供应商客户端 | OpenAI 兼容协议（`chat` / `chat_stream`） |
| 网关装配 | 管道顺序 + fallback 边界 |

### 1.2 不在范围内

| 项 | 理由 |
|---|---|
| 真实上游连通性 | P3 不依赖真实模型服务；客户端用本地 mock HTTP server 测 |
| 成本核算的准确性 | token 估算是**保守上界**，不是厂商计费口径 |
| 跨副本熔断一致性 | 熔断是**进程内**状态，单副本粒度（§6 决策 D3） |
| ollama 原生协议（`/api/chat`） | 与 OpenAI 兼容是**两套**字段形状；ollama 走 `/v1` 兼容层 |
| SSE 完整分帧 | 属 P5（`nexus-api` 的 SSE 桥接）；P3 只到「首事件边界」 |
| 跨副本配额协调 | 属容量治理（`nexus-capacity`） |

---

## 2. 不可让步的红线

### 2.1 契约不可改

`nexus-core` 的 **42 个业务码、12 个事件名、18 个 ID 类型已定稿**（架构文档 §6.3）。

P3 **复用**下列码值，**不得新增**：

| 码值 | 枚举 | HTTP | `RetryPolicy` | P3 用途 |
|---|---|---|---|---|
| `30006` | `BudgetExceeded` | 500 | `Terminal` | 预算不足（调用前拒绝） |
| `40001` | `RateLimited` | 429 | `Retryable` | 上游限流 |
| `50001` | `ModelUnavailable` | 502 | `Retryable` | 上游错误 / 熔断打开 / 流已开始 |
| `50002` | `ModelTimeout` | 504 | `UnknownOutcome` | 上游超时 |
| `50003` | `ProviderNotFound` | 404 | `Terminal` | 供应商不存在 |
| `90001` | `Internal` | 500 | `Terminal` | 响应格式非法（**不是** `ModelUnavailable`） |
| `90003` | `ServiceUnavailable` | 503 | `Retryable` | 调用被取消 |

> `nexus-llm` 的领域错误（`LlmError`）是 `nexus-core` 的**扩展**，不是替换。
> `nexus-llm` **不另写**一套重试判据。

### 2.2 依赖方向

`nexus-llm` 的 `Cargo.toml` **不得**出现 `nexus-repo` —— **网关不查库**。
`model_configs` → `ModelDescriptor`（含解密后的 `api_key`）由 service 层解析后传入。

由 `scripts/check-deps.sh` 规则 5 强制 + `nexus-llm/tests/architecture.rs` 双重保证。

### 2.3 不引入 `Box<dyn>`

供应商多态用 **enum + match**（架构文档 §2.3.2）。理由：穷尽匹配、无虚调用开销、错误类型精确。

由 `nexus-llm/tests/architecture.rs` 扫描源码强制。

---

## 3. 核心契约

### 3.1 网关管道（顺序**有讲究**）

```
Chat / ChatStream
  → 预算裁剪（**每次尝试都重跑**，含 fallback）
  → 熔断 allow?  → 拒绝 → 走 fallback
  → 供应商并发信号量
  → 重试（指数退避 + 抖动）
  → OpenAI 兼容供应商
  → 失败 → fallback（且 provider 与 primary 不同才切）
```

**熔断在最外层** —— 避免已知故障的供应商消耗配额。

### 3.2 回退边界

| 场景 | 能否回退 | 依据 |
|---|---|---|
| 非流式调用失败 | ✅ 可回退 | 参考实现 `gateway.go:133-142` |
| 流式：**首事件到达前**失败 | ✅ 可回退 | 参考实现 `gateway.go:180-190` |
| 流式：**首事件之后**失败 | ❌ **不可** | 已吐出的 token 无法撤回 |

架构文档 §3.3：**流式响应一旦开始向客户端发送，就不能回退到另一个模型**。

**Rust 特有的陷阱**：`Stream` 是**惰性**的 —— 「已经发给客户端了」不是「调用了 `send`」，
而是「下游 poll 到了这个 item」。所以**锁定点必须在实际 yield 之前**。

参考实现：`examples/demo-stream-fallback`（**回归基准**，生产实现把形状搬进 `nexus-llm`）。

**fallback 的另一个条件**：只在 **fallback 的 provider 与 primary 不同**时才切换。
两边都失败 → 错误信息**同时含** primary 与 fallback 的信息。

### 3.3 预算裁剪（纯函数）

调用**前**拒绝，不等上游返回 400 —— 这让预算错误可以在**重试之前短路**。

```
framing    = 64 + 16 * len(messages) + 32 * len(tools)
body_bytes = 序列化后的请求体字节数（token 数的上界）
limit      = desc.max_output_tokens（0 则回落 desc.max_tokens）
requested  = req.max_tokens（0 则默认 1024）
requested  = min(requested, limit, context_window / 4)
```

**两个不同的拒绝条件**：

1. `requested >= context_window` → 拒绝（预留输出本身吃掉整个窗口）
2. `body_bytes + framing > context_window - requested` → 拒绝

`context_window == 0` 表示**未知** → 不裁剪。

**保守上界，不是精确 token 数** —— 保证「不会超」，不保证「充分利用」。

### 3.4 失败分类

判据一律取 `nexus-core` 的 `RetryPolicy`（架构文档 §2.6.3）：

| 策略 | 含义 | P3 行为 |
|---|---|---|
| `Retryable` | 明确失败，重试安全 | 自动重试（带退避） |
| `UnknownOutcome` | 结果未知，重试可能重复副作用 | **不重试**（如 `ModelTimeout`） |
| `Terminal` | 明确失败，重试无意义 | 不重试（如 `BudgetExceeded`） |

> 这是本设计相对参考实现的**关键改进**：参考实现的 `IsRetryable` 把 504（超时，可能已执行）
> 与 502（明确拒绝）一视同仁 —— 504 下重试支付类工具会重复扣费。

### 3.5 熔断

| 属性 | 值 | 依据 |
|---|---|---|
| 状态位置 | **进程内**（`std::sync::Mutex` + 环形缓冲） | 决策 D3 |
| 窗口大小 | 20 | 参考实现 `fallback.go` |
| 失败率阈值 | 0.5 | 超过一半失败才熔断 |
| 打开时长 | 60s | 参考实现 `fallback.go` |
| 预热期 | 样本数 < 窗口大小 → **不评估**错误率 | 样本不足不评估是**更安全**的规则 |

**取消不计入熔断样本** —— 用户取消不是供应商的过错。
**熔断拒绝也不计入** —— 那是我们主动拒绝的。

| 结果 | 进窗口？ |
|---|---|
| 成功 | ✅ 进（拉低错误率） |
| 失败 | ✅ 进 |
| 取消 | ❌ 不进 |
| 熔断拒绝 | ❌ 不进 |

---

## 4. 安全契约

### 4.1 凭据不外泄

`ModelDescriptor.api_key` 用 `secrecy::SecretString` 承载，字段**私有**，
`Debug` **手写**实现保证输出 `[REDACTED]`（不依赖 `SecretString` 的 `Debug` 行为）。

`ToolDef` 的控制面字段（`dangerous` / `replay_policy` / `source_identity` /
`binding_version` / `capability`）**不参与序列化** —— 那是控制面的东西，不是给模型看的。

### 4.2 内部原因不外泄

`StreamEvent.err` **不参与序列化**，且 `StreamEvent` **不实现 `Deserialize`**
—— 「从 wire 上塞一个错误进来」在**编译期**就不可能。

供应商响应体解析失败 → `ErrorCode::Internal`（**不是** `ModelUnavailable`，
后者是 `Retryable`，会让解析失败被自动重试 —— 重试同样的坏响应没有意义）。

`base_url` 细节不回显到用户可见消息。

---

## 5. 完成判据与「防伪」要求

架构文档 §6.1：**「预算裁剪与回退边界有测试」**

拆成两个可验证的子目标：

1. **预算裁剪有测试** —— 上下文/输出预算在**调用前**被拒绝，不等上游返回 400。
2. **回退边界有测试** —— 非流式可回退、流式**首事件前**可回退、**首事件后**不可回退。

### 5.1 「有测试」的**防伪**要求

沿用 NEX-14 的教训：测试「存在」不等于「真的跑了」。

| 假象形态 | 防线 |
|---|---|
| 预算裁剪只在 happy path 测了「不超预算」 | 必须同时测**超预算被拒**与**边界值恰好通过** |
| 回退只测了「流开始前可回退」 | 必须测「流开始后回退被拒」—— 只测前者等于没测边界 |
| 预算裁剪测试没覆盖 fallback 路径 | 每次尝试（含 fallback）都要**重跑**裁剪 —— 参考实现的明确不变量 |
| 熔断测试只测「打开」不测「预热期」 | 预热期样本不足时**不得**评估错误率 —— 否则窗口零值会被误判为真实失败 |

---

## 6. 已定稿的技术决策

| 编号 | 决策 | 选择 | 依据 | 类别 |
|---|---|---|---|---|
| D1 | 供应商多态形态 | **enum + match**，不是 `Box<dyn>` | §2.3.2 | 已定稿 |
| D2 | 供应商协议范围 | **只做 OpenAI 兼容**（ollama 走 `/v1`） | 本次决策 | (a) decide-and-proceed |
| D3 | 熔断状态位置 | **进程内**（`std::sync::Mutex` + 环形缓冲） | 本次决策 + 参考架构 §3.1.3 | (a) decide-and-proceed |

### 6.1 D2 的理由

deepseek / openai / moonshot / qwen / zhipu / doubao 都是**同一套** OpenAI 兼容协议；
本地 ollama 是同一协议的 `base_url` 变体（`/v1` 路径）。

引入第二套协议形状（ollama 原生 `/api/chat`）在只有一个供应商时**没有收益**。

### 6.2 D3 的**显式表态**

参考实现的熔断是进程内（`internal/llm/fallback.go:4-5`），多副本部署时会出现
「A 副本熔断、B 副本继续打」的不一致（参考架构文档 §5.3 局限 #3）。

**本阶段决策：进程内实现 + 文档显式声明「熔断是单副本粒度」。**

这不是「照抄局限」，而是**明确表态** —— 外置到 Redis/PG 属后续特性，不在 P3。

> 决策类别 (a)：可低成本回滚、影响面小、不触碰信任链与治理控制。

---

## 7. 验证方式

```bash
# 门禁
bash scripts/check-lock.sh      # 依赖锁定纪律
bash scripts/check-deps.sh      # crate 依赖方向铁律
bash scripts/check-naming.sh    # 标识符不得含中文
bash scripts/check-backlog.sh   # backlog 组织纪律
bash scripts/check-specs.sh     # Spec 层组织纪律
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是**独立 workspace**，验证命令为 `cd examples && cargo test --workspace`。
