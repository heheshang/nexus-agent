---
id: NEX-23
title: "feat(llm): 网关可观测与配置接线 —— S8 部署运维收口"
status: Done
assignee: []
labels: [llm, observability, config, s8]
priority: medium
dependencies:
  - NEX-19
  - NEX-20
references:
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.4"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.8"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.6"
  - "docs/operations/configuration.md"
  - "docs/ai-sdlc/adapter-inventory.md"
  - ".cursor/rules/ai-sdlc-observability.mdc"
---

## Description

P3 的**能力**已完成（NEX-16..20），但 **S8（部署与运维）核对发现三个真实缺口**。

收口不等于「代码写完了」——
`ai-sdlc-observability.mdc` §6 要求 S8 的最低标准包含
「**可观测**：关键路径有指标与结构化日志；错误可按 `trace_id` 定位」。

### 缺口 1 —— 网关**零可观测**（架构文档 §2.4 关键不变量第 2 条）

参考架构 §2.4 的「关键不变量」明确列了 **3 条**，第 2 条是：

> **span 生命周期绑定流**：`spanOnClose` 把 span 的结束推迟到事件流关闭
> （`internal/llm/gateway.go:197`）。
> 注释原文：「若放进 `runWithGuard`，拿到通道就返回，span 只有几毫秒，等于没测」。

并且「Rust 版落点」表把 `spanOnClose` 列为**必须实现**的机制：

> `spanOnClose` → `tracing` span 无法「延迟结束」→ 用 `Span::record` + 显式 `drop(span)`；
> **或在流结束时用 `span.in_scope(...)` 记录结束时间**。
> 这是 OTel/tracing 的固有差异，**需要专门设计**。

**当前实现的实际状态**：

| 项 | 状态 |
|---|---|
| `nexus-llm` 里的 `tracing::` 调用 | **0 处** |
| `nexus-observability` 的内容 | **7 行**，只有 `CRATE_NAME` 常量 |
| 全 workspace 调用 `nexus_observability::` 的地方 | **0 处** |

也就是说：`nexus-observability` 是个**空壳**，
`nexus-llm` 声明了对它的依赖但**从不使用**。

这不是「优化项」——**架构文档 §2.4 把 span 绑定流列为关键不变量**，
而 §3.8 明确「可观测性是结构的一部分，不是事后贴的」（原则 5）。

### 缺口 2 —— 配置**未接线**（运维改了环境变量不生效）

`nexus-config` 的映射表里**已经有**这些条目：

| 环境变量 | 配置路径 |
|---|---|
| `NX_LLM__DEFAULT_PROVIDER` | `llm.default_provider` |
| `NX_LLM__REQUEST_TIMEOUT_SECS` | `llm.request_timeout_secs` |
| `NX_LLM__CONTEXT_BUDGET` | `llm.context_budget` |
| `NX_LLM__DEEPSEEK__BASE_URL` | `llm.deepseek.base_url` |
| `NX_LLM__DEEPSEEK__API_KEY` | `llm.deepseek.api_key`（**敏感**） |
| `NX_LLM__DEEPSEEK__MODEL` | `llm.deepseek.model` |

`resources/default.yaml` 里也有 `llm:` 段（`request_timeout_secs: 120` 等）。

**但 `nexus-llm` 从不读这些**：

- `ProviderConfig::timeout` 是**硬编码** `Duration::from_secs(120)`；
- `ProviderConfig::default()` 不接受任何外部输入；
- `nexus-llm` 虽依赖 `nexus-config`，但**没有任何一处**调用它的读取 API。

后果：运维设 `NX_LLM__REQUEST_TIMEOUT_SECS=300`，**不生效**。
而 `nexus-cli` 的 `config show` 会**显示**这个值已生效 —— 那是**假象**。

> 这是本仓库第三次踩「**看起来覆盖了，实际没有**」的坑
> （前两次：NEX-14 的 `check-deps.sh` 规则 7、NEX-22 的文档层缺口）。

### 缺口 3 —— 没有 P3 的**运行说明**

`docs/operations/` 下有 `p1-plan.md` / `p2-plan.md` / `p3-plan.md`，
但那是**计划**文档，不是**运行说明**。

`ai-sdlc-observability.mdc` §6 要求「`docs/operations/` 下有对应运行说明」。
运维需要知道：

- 模型供应商怎么配（`base_url` / `api_key` / `model`）；
- 凭据从哪来、怎么保证不进日志；
- **熔断是单副本粒度**对部署意味着什么（多副本要不要外置）；
- 哪些指标能看、哪些**看不到**（成本未核算）。

### 附带缺口 —— 成本**恒为 `None`**（参考架构 §5.3 局限 #9）

参考架构 §5.3 局限 #9 记录：

> **成本指标无写入点** —— `llm_cost_total` 定义在 `metrics.go:43`，
> 但 `recordUsage` 只写 token。
> 决定成本是估算还是上游返回。

本实现 `Usage.cost` 恒为 `None`（`OpenAiChatResponse::into_chat_response`
里写死 `cost: None`），而 `ModelDescriptor` 有 `input_price` / `output_price`
两个 `Decimal` 字段 —— **能算但没算**。

这不是 bug（`docs/specs/p3/spec.md` §1.2 明确「成本核算的准确性」不在 P3 范围），
但**指标层必须如实反映** —— 不能让 `llm_cost_total` 恒为 0 而没人知道。

---

## 范围边界

### 做

1. `nexus-observability` 从空壳变成**真能用**的埋点设施
   （span 生命周期绑定流 + 指标计数器）。
2. `nexus-llm` 接入埋点：非流式与流式都有 span，流的 span **绑到流关闭**。
3. `nexus-llm` 的 `ProviderConfig` / `GatewayConfig` 能从 `ConfigStack` 读取。
4. P3 运行说明文档。

### 不做（显式排除）

| 项 | 理由 |
|---|---|
| **成本核算的实现** | 精度问题属 P4；但**指标层要如实标记「未核算」** |
| OTel 导出端点 / Prometheus HTTP 暴露 | 属 `nexus-observability` 的完整形态（P9）；本任务只保证**埋点产生正确的数据** |
| 熔断状态外置 | 决策 D3 明确**单副本粒度**；外置属容量治理 |
| 分布式追踪的跨服务传播 | 需 P5 的 HTTP 中间件 |
| 修改 `nexus-core` 的 42 个业务码 | §6.3 已定稿 |

---

## Acceptance Criteria

- [x] #1 `nexus-observability` 提供**流绑定 span** 的构造器 —— span 的生命周期与**流**绑定，而非与「拿到流」绑定
- [x] #2 `nexus-observability` 提供 `llm_requests_total` 计数器，标签含 `status`（`ok` / `error` / `canceled`）与 `provider`
- [x] #3 `nexus-llm` 的**非流式**调用有 span，span 记录 `provider` / `model` / `finish_reason` / `usage`
- [x] #4 `nexus-llm` 的**流式**调用的 span 在**流关闭时**才结束 —— 有测试断言「拿到流后 span 未结束，消费完才结束」
- [x] #5 取消（`Canceled`）**单独计数**，不与 `error` 混在一起 —— 有测试断言
- [x] #6 `ProviderConfig` / `GatewayConfig` 可从 `nexus_config::ConfigStack` 构造 —— 有测试断言
- [x] #7 `llm.request_timeout_secs` 走配置路径，`ProviderConfig::default()` 不再硬编码超时 —— 有测试断言改配置能改超时
- [x] #8 成本**未核算**这件事在**指标与文档**里如实标记 —— 不出现「成本为 0」的静默假象
- [x] #9 `docs/operations/p3-runbook.md` 存在：供应商配置、凭据来源、熔断单副本粒度的部署含义、能看到与看不到的指标
- [x] #10 `docs/README.md` 索引覆盖新增文档（`check-docs.sh` 校验）
- [x] #11 `docs/ai-sdlc/adapter-inventory.md` 的模型供应商行状态从「🚧 P3 进行中」更新为实际状态
- [x] #12 结构层测试：埋点**不得**把 `api_key` 写进 span 字段或日志
- [x] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [x] #14 CI 七道门禁全绿

---

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

13 条 AC 全部可二元判定 —— 每条都指明「有测试断言」或「文件存在」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。
「成本核算」是**显式排除**（范围边界表），不是未决问题。

### #3 引用可解析

- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.4` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.8` —— 存在
- `docs/operations/configuration.md` —— 存在
- `docs/ai-sdlc/adapter-inventory.md` —— 存在
- `.cursor/rules/ai-sdlc-observability.mdc` —— 存在

### #4 范围有界

「做」4 项 / 「不做」5 项，均已列明。**不触及** 42 个业务码、ID 类型、事件清单、crate 依赖方向。

### #5 受影响面被指名

| crate | 影响 |
|---|---|
| `nexus-observability` | 从空壳变为有实现（**新增** `tracing` / `prometheus` 使用） |
| `nexus-llm` | 接入埋点 + 配置接线（`ProviderConfig` / `GatewayConfig` 构造器） |
| `nexus-config` | **只读**，不改映射表（条目已存在） |
| `docs/` | 新增 `p3-runbook.md` + 索引更新 |

**不新增 crate**，**不新增依赖**（`tracing` / `prometheus` 已在 workspace 依赖里）。

### #6 完成态可描述

- `nexus-observability` 有**公开 API**（span 构造器 + 指标计数器）且被 `nexus-llm` 真实调用；
- `ProviderConfig::from_config(&ConfigStack)` 可用；
- `docs/operations/p3-runbook.md` 存在且被 `docs/README.md` 索引；
- 全部测试绿。

### #7 无隐形依赖

- `prometheus` 0.14 与 `tracing` 0.1 **已在 workspace 根 manifest** —— 不需要改根 manifest；
- `nexus-config` **已是** `nexus-llm` 的依赖 —— 不需要改依赖方向；
- `nexus-observability` **已是** `nexus-llm` 的依赖。

### 结论

**pass** —— 七项全过，可进入实现（S4）。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：本任务的范围定为「完整 P3 收口」（补齐网关可观测 + 配置接线 + 运行说明），而非「只写文档声明延后」。
- **依据**：
  - 架构文档 §2.4 把 span 绑定流列为**关键不变量**，不是可选优化；
  - 缺口 2 是**静默失效**（运维改了配置不生效，而 `config show` 显示已生效）——
    这正是本仓库反复记录的那类「看起来覆盖了，实际没有」的坑；
  - `ai-sdlc-observability.mdc` §6 把「可观测」列为 S8 的**最低要求**。
- **可逆性**：全部是**新增**代码与文档，无既有行为变更；回滚 = `git revert`。
- **影响面**：`nexus-observability`、`nexus-llm`、`docs/` 三个位置。
  **不触及** 42 个业务码、ID 类型、事件清单、crate 依赖方向。

### 类别：decide-and-proceed

- **决策**：成本核算**不实现**，但指标层**必须如实标记「未核算」**。
- **依据**：`docs/specs/p3/spec.md` §1.2 已把「成本核算的准确性」列为 P3 范围外；
  但参考架构 §5.3 局限 #9 的教训是「指标有定义却没有写入点」会让读者误读。
- **可逆性**：纯增量，回滚无风险。

---

## 实现过程中的发现

### 1. `tracing` 的 span 是**引用计数**的 —— `wrap` 必须**消耗** `self`

**发现**：最初的 `StreamSpan::wrap(&self, ...)` 内部 `clone()` 一份放进 wrapper。
按直觉这样「wrapper 持有 span，流结束 span 就结束」。

**实际**：`tracing` 的 `on_close` 只在**最后一个** `Span` 句柄被 drop 时触发。
只要调用方还留着 `self`，引用计数降不到 0 —— **span 永远不会关闭**。

**修正**：`wrap` 改成**消耗** `self`（`pub fn wrap<S>(self, ...)`）。
wrapper 持有**唯一**句柄，它被 drop 时 span 立刻关闭。

> 这是**类型级**的保证 —— 之后想留克隆的人必须**显式** `clone()`，
> 而不是不小心多留一份。

### 2. `nexus-observability` 的单元测试**抓不到**网关多留克隆

`wrap` 消耗 `self` 之后，单元测试全绿。
但如果网关**另外**留一份 span 克隆（比如塞进 `StreamHandle`），
引用计数还是降不到 0 —— 而 `nexus-observability` 的测试**照样绿**。

**原因**：单元测试验证的是「`wrap` 自己的行为」，不是「**调用方**怎么用」。

**修正**：增加了**网关层**测试 `the_gateway_span_closes_when_the_stream_is_consumed` ——
从网关的公开 API 出发（建流 → 消费流 → 断言 span 已关闭）。
**变异测试验证**：把克隆塞进 `StreamHandle` → 该测试**失败**。

### 3. `tracing` 的 callsite **interest 缓存**是**全局**的 —— 导致测试**随机失败**

**症状**：`the_gateway_span_closes_when_the_stream_is_consumed` **随机失败**
（8 次里挂 1~2 次），单独跑必过，全量跑时挂。

**排查**：`on_new_span` **根本没被调用** —— span **压根没被创建**。

**原因**：`tracing` 把「这个 callsite 要不要记录」**全局缓存**在 callsite 注册表里，
在 callsite **第一次**注册时算一次，之后不再重算。

同一个测试二进制里**另一个测试**先跑过 `chat_stream` ——
那时**还没有** subscriber —— callsite 的 interest 被缓存成 `never`。
之后本测试装的 subscriber **看不到**任何 span。

`rebuild_interest_cache` **救不了**：它只对**已注册的全局 dispatcher** 生效，
而线程局部的 `set_default` 不在其中。

**修正**：把这条测试**单独**放进一个测试二进制 `stream_span_lifecycle.rs`，
并在**任何**业务调用**之前**装上**全局** subscriber。

> **「先注册 callsite 的那个测试赢了」** —— 这是 `tracing` 的一个真实陷阱，
> 值得记住。

### 4. `set_default` 是**线程局部**的 —— span 的 `on_close` 可能根本不触发

即使把 subscriber 装对了，`set_default` 也只对**当前线程**可见。

而流式测试里：
- `tokio::spawn` 起的 mock server 在**别的线程**；
- span 的最后一个句柄（流被 drop 时）可能在**别的线程**释放 ——
  而 `on_close` 是在**释放的那个线程**上触发的。

线程局部 subscriber 在那些线程上**看不见**。

**修正**：用 `set_global_default`（全局）。全局 subscriber 只能设一次，
所以用 `OnceLock` 保证重复运行不报错。

### 5. `prometheus` 的 `CounterVec` **必须**全局共享

**症状**：`llm_tokens_total` 指标**渲染不出来**。

**原因**：`LlmMetrics::new()` 每次都新建 `CounterVec` 并注册。
第二次注册返回 `Error::AlreadyReg`（被 `let _ =` 吞掉），
而**全局注册表里留下的仍是第一次那个空计数器** ——
写进新计数器的值**根本不会被导出**。

**修正**：计数器改用 `LazyLock` 全局共享，`LlmMetrics` 只是它的**句柄**。

> 「注册失败被忽略」看起来无害，实际上让指标**静默失效**。

### 6. `CounterVec::with_label_values` 在标签不匹配时 **panic**

`prometheus` 0.14 的 `with_label_values` 内部是 `get_metric_with_label_values(...).unwrap()` ——
标签数量不符就**崩溃**。

而供应商代号来自**配置**（外部输入）—— 崩溃的代价远大于漏记一次。

**修正**：改用 `get_metric_with_label_values`（返回 `Result`），
错误用 `tracing::warn!` 记录。测试 `a_mismatched_label_does_not_panic` 覆盖。

### 7. 源码扫描断言必须**剥离注释**且**限定作用域**

架构测试 `the_gateway_pipeline_order_matches_the_contract` 原本扫 `self.provider.chat`。
插桩后 `cargo fmt` 把它拆成多行 → 断言**静默失效**（找不到字符串时 panic，
但用 `find(".provider")` 又会命中 `desc.provider_code` 这个**字段访问**）。

**修正**：
- **剥离注释**（模块级文档里就写着「预算裁剪 → 熔断 → ...」，不剥离会命中**文档**）；
- 限定到 `attempt` 函数体；
- 找 **`.chat(`**（方法调用）而不是 `.provider`（字段访问）。

**变异测试验证**：把预算裁剪移到熔断之后 → 断言**失败**。

### 8. `nexus-config` 的 `get_u64` 存在，但 `ProviderConfig` 之前**完全没读过配置**

这是本任务的核心缺口。映射表里**早就有** `NX_LLM__REQUEST_TIMEOUT_SECS`，
但 `nexus-llm` 从不读 —— 运维设了环境变量**不生效**，
而 `nexus config show` 会**显示**它已生效。

**修正**：`ProviderConfig::from_config` / `GatewayConfig::from_config`。
新增 `llm.max_conns_per_provider` 映射（并发上限也该可配）。

**配置缺失时回退到缺省值**而不是报错 —— 让「忘了配一项」变成「服务不可用」
是不合理的。**但 0 值必须回退**：0 超时会让每个请求立即失败，
0 并发会让所有请求卡在 `acquire` 上。

### 9. 成本恒为 `None` —— 指标层如实反映

`Usage.cost` 恒为 `None`，且**没有** `llm_cost_total` 指标。

参考架构 §5.3 局限 #9 记录了参考实现的同类问题
（「成本指标有定义却没有写入点」）。

**选择**：宁可**没有**这个指标，也不要给一个恒为 0 的假指标。
已在 `docs/operations/p3-runbook.md` 的「看不到的东西」表里如实列出。

---

## 验证记录

| 门禁 | 结果 |
|---|---|
| `bash scripts/check-lock.sh` | ✓ |
| `bash scripts/check-deps.sh` | ✓ |
| `bash scripts/check-naming.sh` | ✓ |
| `bash scripts/check-backlog.sh` | ✓ |
| `bash scripts/check-specs.sh` | ✓ |
| `bash scripts/check-rules.sh` | ✓ |
| `bash scripts/check-docs.sh` | ✓ |
| `cargo fmt --all --check` | ✓ |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ✓ 零告警 |
| `cargo test --workspace --locked` | ✓ 383 passed, 0 failed |

**新增测试**：14 个（`nexus-observability` 5 + `nexus-llm` 9）。

**变异测试**（验证断言真的有效）：

| 变异 | 期望 | 实际 |
|---|---|---|
| 网关在 `StreamHandle` 里多留 span 克隆 | `stream_span_lifecycle` 失败 | ✓ 失败 |
| `wrap` 里 `mem::forget` 一份克隆 | `observability_contract` 失败 | ✓ 失败（2 个用例） |
| 预算裁剪移到熔断判断之后 | `architecture` 失败 | ✓ 失败 |
