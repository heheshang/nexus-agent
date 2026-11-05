# P3 运行说明 · LLM 网关

架构文档 §2.4 / §3.8 的落地说明。**这是运行说明**，不是计划 ——
计划见 [`p3-plan.md`](p3-plan.md)，配置全表见 [`configuration.md`](configuration.md)。

## 网关做什么

`nexus-llm` 是**模型网关层**：它把「一次业务上的模型调用」变成
「一次带预算、熔断、重试与失败转移的供应商调用」。

它**不**做模型选择（那是上层的事），**不**做业务语义，**不**查库。

## 配置模型供应商

### 三层来源

配置的优先级是 `base < file < env`（见 [`configuration.md`](configuration.md)）。
模型供应商相关的配置项：

| 配置路径 | 环境变量 | 敏感 | 说明 |
|---|---|---|---|
| `llm.default_provider` | `NX_LLM__DEFAULT_PROVIDER` | | 默认供应商标识 |
| `llm.request_timeout_secs` | `NX_LLM__REQUEST_TIMEOUT_SECS` | | 单次请求超时（秒） |
| `llm.context_budget` | `NX_LLM__CONTEXT_BUDGET` | | 上下文预算（token） |
| `llm.deepseek.base_url` | `NX_LLM__DEEPSEEK__BASE_URL` | | 供应商基地址 |
| `llm.deepseek.api_key` | `NX_LLM__DEEPSEEK__API_KEY` | ✅ | 供应商凭据 |
| `llm.deepseek.model` | `NX_LLM__DEEPSEEK__MODEL` | | 模型名 |

### `base_url` 的 `/v1` 后缀

`base_url` **带不带 `/v1` 都可以**：

| 配置值 | 实际请求地址 |
|---|---|
| `https://api.deepseek.com/v1` | `https://api.deepseek.com/v1/chat/completions` |
| `https://api.deepseek.com` | `https://api.deepseek.com/v1/chat/completions` |

已带 `/v1` 的**不会**重复追加。**不会**出现 `/v1/v1/chat/completions`。

### 协议范围

**只做 OpenAI 兼容协议**。deepseek / openai / qwen / zhipu / moonshot / doubao
都是同一套协议；本地 ollama 走 `/v1` 兼容层。

**不做** ollama 原生协议（`/api/chat`）—— 那是另一套字段形状。
契约依据：`docs/specs/p3/contracts/invariants.json` 决策 **D2**。

## 凭据

### 承载

`api_key` 由调用方（`nexus-service`）从配置解析后传入
`ModelDescriptor`，类型是 `secrecy::SecretString`。

**网关不查库** —— 它拿不到数据库连接，只能拿到已解密好的描述符。
这条由 **Cargo 依赖图**保证（`nexus-llm` 的 `Cargo.toml` 不出现 `nexus-repo`）。

### 不外泄

| 防线 | 机制 |
|---|---|
| `Debug` 输出不含明文 | `SecretString` 的 `Debug` 是 `[REDACTED]` |
| 只在发请求时取用 | 必须显式调 `expose_api_key()` |
| 不进错误消息 | 错误信息只带供应商代号与状态码 |
| **不进埋点** | 结构层测试 `the_credential_never_reaches_the_instrumentation` 扫描埋点调用点 |

> 最后一条是**结构层**防线 —— 埋点字段会进日志、进 trace 后端，
> 那是**不可撤回**的扩散。所以它靠**扫描源码**保证，而不是靠调用方自觉。

### 日志与指标里的敏感信息

| 位置 | 会出现的 | 不会出现的 |
|---|---|---|
| span 字段 | 供应商标识、模型名、finish_reason、token 数 | `api_key` |
| `llm_requests_total` 标签 | `provider`、`status` | `api_key` |
| `llm_tokens_total` 标签 | `provider`、`kind` | `api_key` |

## ⚠️ 熔断是**单副本粒度**

**这是本阶段显式接受的局限**，不是疏漏。

熔断状态是**进程内**的（`nexus-llm` 的 `BreakerRegistry` 用 `std::sync::Mutex`
保护一个 `HashMap`），状态**只在本进程内存在**。

### 部署含义

| 部署形态 | 熔断行为 |
|---|---|
| 单副本 | 完全符合预期 |
| **多副本** | 每个副本各有一套 —— 会出现「A 副本判定供应商已熔断并拒绝，B 副本却继续打」 |

从外部看，同一个供应商在**同一时刻**有的副本放行、有的副本拒绝。

### 为什么本阶段接受

1. 外置熔断状态（Redis / PG）需要**跨副本一致性与 fencing** ——
   那属于容量治理（`nexus-capacity`）的范畴。
2. 熔断的语义是「**减少**对已知故障供应商的调用」——
   单副本粒度下它仍然**有效**（A 副本确实少打了），只是**不是全局一致**。
3. **沉默比不一致更危险** —— 如果读者以为多副本下也一致，
   排障时会得出完全错误的结论。

### 什么时候必须外置

- 需要**全局一致**的熔断决策（比如「全集群同时停止打一个供应商」）；
- 供应商配额是**集群级**的（每副本各熔断会让总流量超配额）；
- 副本数会**弹性伸缩**（新副本没有历史熔断状态，会立刻打满）。

契约依据：架构文档 §4.3「熔断是单副本粒度 —— 显式接受的局限」。

## 可观测

### span

| span | 覆盖范围 | 生命周期 |
|---|---|---|
| `llm.chat` | 一次业务调用（含 fallback） | 调用结束 |
| `llm.attempt` | 一次供应商尝试 | 该次尝试结束（重试会产生多个） |
| `llm.stream` | 一次流式调用 | **流关闭时** |

> ⚠️ `llm.stream` 的生命周期**绑定流**，不是绑定「拿到流」。
>
> 参考实现的 `spanOnClose` 注释原文：
> 「若放进 `runWithGuard`，拿到通道就返回，span 只有几毫秒，等于没测」。
>
> 本实现的 `StreamSpan::wrap` **消耗** span（而不是克隆）——
> 因为 `tracing` 的 span 是**引用计数**的，
> 只有**最后一个**句柄被 drop 时才触发 `on_close`。
> 留一份克隆在别处，span 就永远不会关闭。

### 指标

| 指标 | 类型 | 标签 | 含义 |
|---|---|---|---|
| `llm_requests_total` | Counter | `provider`、`status` | 请求总数 |
| `llm_tokens_total` | Counter | `provider`、`kind` | token 总数（`kind` = `prompt` / `completion`） |

`status` 有三个取值：`ok` / `error` / `canceled`。

> **`canceled` 单独计数** —— 便于区分「上游故障」与「用户取消」。
> 把取消混进 `error` 会让错误率虚高。

#### 怎么看到这些指标

指标**产生**在 `nexus-observability::LlmMetrics`，**暴露**在 `nexus-api` 的
`GET /metrics` 端点（NEX-25）。

```text
GET /metrics
→ 200 OK
→ Content-Type: text/plain; version=0.0.4; charset=utf-8
```

响应体是 Prometheus 文本格式：

```text
# HELP llm_requests_total LLM 请求总数
# TYPE llm_requests_total counter
llm_requests_total{provider="deepseek",status="ok"} 12
# HELP llm_tokens_total LLM token 总数
# TYPE llm_tokens_total counter
llm_tokens_total{provider="deepseek",kind="prompt"} 1024
```

**采集**：Prometheus 抓取器直接指向该端点即可，无需 exporter 中转。

```yaml
scrape_configs:
  - job_name: nexusagent
    static_configs:
      - targets: ["nexus-api:8080"]
```

#### ⚠️ 该端点**只应在内网暴露**

`/metrics` **不要求鉴权** —— 这是有意的决策：

- Prometheus 抓取器**通常**没有业务凭据 —— 加鉴权会让标准抓取配置复杂化；
- 指标**不含**凭据（标签只有 `provider` / `status` / `kind`）——
  所以未鉴权暴露的信息是**低敏**的。

**代价**：任何能访问该端点的人都能看到供应商代号与请求量。
所以它**必须**部署在内网 / 有网络策略限制的位置，
**不要**直接暴露到公网。

#### ⚠️ 指标里**不会有**凭据

架构文档 §2.6.5 与「凭据不得进入埋点」的红线在这里是**类型级**保证：
`LlmMetrics` 的写入方法（`record_request` / `record_tokens`）
**签名上就不接受凭据类型** —— 标签只有 `provider` / `status` / `kind` 三个。

#### ⚠️ 没有流量时，指标**不出现**

`prometheus` 的 `Registry::gather()` 会**剪掉空 family**
（没有样本的指标定义不输出）。

所以刚启动、还没有任何请求时，`/metrics` 返回的是**空体** —— 不是 404，
是 200 + 空内容。

这不是缺陷，是标签口径的必然结果：标签值来自**运行期配置**
（供应商代号），启动时无从得知。

### ⚠️ 成本**没有**核算

`Usage.cost` **恒为 `None`**，且**没有** `llm_cost_total` 指标。

参考架构 §5.3 局限 #9 记录了参考实现的同类问题
（「成本指标无写入点」—— 指标有定义却没有写入点，会让读者误读）。

本实现的选择是：**宁可没有这个指标，也不要给一个恒为 0 的假指标**。
`ModelDescriptor` 的 `input_price` / `output_price` 字段已就位，
精确核算属 P4（用量对账）。

### 看不到的东西

| 想知道的 | 为什么看不到 |
|---|---|
| 「这个请求花了多少钱」 | 成本未核算（P4） |
| 「请求是否已到达上游」 | **网关层不可观测** —— 只有上游侧能回答 |
| 「全局熔断状态」 | 熔断是单副本粒度 |
| 「跨副本的配额使用」 | 属容量治理 |
| 「OTel trace 的导出」 | `opentelemetry-otlp` 已在依赖里，但导出配置与后端属 P9 |

## 回滚

| 变更 | 回滚方式 |
|---|---|
| 熔断状态 | 无持久化 —— 重启即清空 |
| 配置变更 | 改回环境变量 / file 层，**无需重启**（配置在启动时读入） |
| 供应商切换 | 改 `llm.default_provider` 与对应的 `base_url` / `model` |
| 代码变更 | `git revert` —— 网关**不写库**，无数据迁移需要回滚 |

> 网关**不持久化任何东西** —— 所以它的回滚路径是**纯代码/配置**的，
> 不涉及数据迁移。这不是巧合，是「网关不查库」的直接后果。

## 验证

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

网关的行为层测试用**本地 mock HTTP server**，不依赖真实模型服务：

```bash
cargo test -p nexus-llm --locked --test gateway_contract
cargo test -p nexus-llm --locked --test config_wiring
cargo test -p nexus-observability --locked --test observability_contract
```

## 相关文件

| 文件 | 作用 |
|---|---|
| [`p3-plan.md`](p3-plan.md) | P3 的计划、红线与实际交付 |
| [`configuration.md`](configuration.md) | 配置三层来源与 `NX_` 映射全表 |
| `crates/nexus-llm/src/lib.rs` | crate 级文档（职责边界与关键约束） |
| `docs/specs/p3/contracts/invariants.json` | 19 条不变量，逐条指向真实测试 |
| 架构文档 §4.3 | 「熔断是单副本粒度」的声明 |
