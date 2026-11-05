---
id: NEX-25
title: "feat(api): 暴露网关与观测指标的 /metrics 端点"
status: Done
assignee: []
labels: [observability, api, metrics]
priority: medium
dependencies:
  - NEX-23
references:
  - "docs/ai-sdlc/metrics-snapshot-2026-11.md §六"
  - "docs/operations/p3-runbook.md"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.8"
  - "crates/nexus-observability/src/lib.rs"
---

## Description

**S9（度量与校准）** 快照发现的第二条流程缺口。

### 观察到的（事实）

NEX-23 已经在网关里产生了真实的 Prometheus 指标：

| 指标 | 标签 | 产生位置 |
|---|---|---|
| `llm_requests_total` | `provider`、`status` | `nexus-llm` 的 `Gateway` |
| `llm_tokens_total` | `provider`、`kind` | 同上 |

但**外部看不到它们** —— 因为：

1. `nexus-observability::LlmMetrics::render()` 能渲染 Prometheus 文本格式，
   但**没有任何调用方**；
2. `nexus serve` **尚未实现**（Phase 5，`nexus-cli/src/main.rs` 里是 `bail!`）；
3. `nexus-api` 还没有 `/metrics` 端点。

### 为什么这是**缺口**

指标产生但不可见，等于**没产生**。

`ai-sdlc-observability.mdc` §6 要求「**可观测**：关键路径有指标与结构化日志」。
「有指标」指的是**能被采集的指标**，不是「代码里有个计数器」。

更实际的问题：**S9 的「重试次数」指标现在取不到** ——
它本该来自 `llm_requests_total` 的 `error` 计数，
但端点不存在，所以快照里那一格只能写「⚠️ 未计量」。

### 本任务要做什么

1. `nexus-api` 增加 `GET /metrics` 端点，返回 Prometheus 文本格式；
2. 端点**不要求鉴权**（指标不含凭据，但**只应在内网暴露**）——
   该决策的契约依据写进文档；
3. 端点**不被**「凭据不得进入埋点」的红线违反 ——
   指标标签只有 `provider` / `status` / `kind`，没有 `api_key`；
4. 端点有**行为层测试**。

## 范围边界

### 做

- `nexus-api` 的 `/metrics` 端点
- 端点的行为层测试
- `docs/operations/p3-runbook.md` 补充「怎么看到这些指标」

### 不做

| 项 | 理由 |
|---|---|
| `nexus serve` 的实现 | 属 Phase 5，**独立**任务 |
| OTel 导出端点 | `nexus-observability` 已依赖 `opentelemetry-otlp`，但导出配置与后端属 P9 |
| 把指标做成**全局单例**供多个 crate 共享 | `LlmMetrics` 已是全局共享计数器（`LazyLock`），端点只需持有句柄 |
| 给 `/metrics` 加鉴权 | 需要 `nexus-security` 的能力评估 —— 但**必须**在文档里写明「只应内网暴露」 |

## Acceptance Criteria

- [x] #1 `nexus-api` 暴露 `GET /metrics`，返回 **Prometheus 文本格式**（`text/plain; version=0.0.4`）
- [x] #2 响应体**包含** `llm_requests_total` 与 `llm_tokens_total` 的指标定义
- [x] #3 端点**不要求**任何凭据即可访问
- [x] #4 响应体中**不包含** `api_key` 或任何凭据字段 —— 有测试断言
- [x] #5 端点有**行为层测试**：请求后能拿到可解析的 Prometheus 文本
- [x] #6 `docs/operations/p3-runbook.md` 写明「怎么访问 `/metrics`」与「它只应内网暴露」
- [x] #7 `docs/operations/p3-runbook.md` 的「可观测」章节补上「指标怎么被采集」这一环
- [x] #8 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [x] #9 CI 全部门禁全绿

## DoR 判定

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

9 条 AC 全部可二元判定 —— 每条都指明「请求后响应体含什么」或「文件含某段」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。
「是否给端点加鉴权」是**显式排除**（范围边界表），不是未决问题 ——
但排除的**代价**（只应内网暴露）已写进 AC #6。

### #3 引用可解析

- `docs/ai-sdlc/metrics-snapshot-2026-11.md §六` —— 存在
- `docs/operations/p3-runbook.md` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §3.8` —— 存在
- `crates/nexus-observability/src/lib.rs` —— 存在

### #4 范围有界

「做」4 项 / 「不做」4 项。
**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `nexus-api` | **新增**一个端点 |
| `docs/operations/p3-runbook.md` | 补两处说明 |
| `nexus-observability` | **只读** —— `render()` 已存在，不需要改 |

**不新增依赖**（`prometheus` 已在 workspace 依赖里）。

### #6 完成态可描述

- `GET /metrics` 返回可解析的 Prometheus 文本；
- 响应体含两个指标定义且不含凭据；
- 有行为层测试覆盖；
- 全部测试绿。

### #7 无隐形依赖

- `prometheus` 已在 workspace 根 manifest；
- `nexus-observability` **已被** `nexus-api` 依赖（`Cargo.toml` 里有 `nexus-observability.workspace = true`）；
- `axum` 已在 `nexus-api` 的依赖里。

### 结论

**pass** —— 七项全过，可进入实现（S4）。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：`/metrics` **不要求鉴权**，但文档必须写明「只应内网暴露」。
- **依据**：
  - Prometheus 抓取器**通常**没有业务凭据 —— 加鉴权会让标准抓取配置复杂化；
  - 指标**不含**凭据（标签只有 `provider` / `status` / `kind`）——
    所以未鉴权暴露的信息是**低敏**的；
  - 参考实现的 `metrics.go` 也是**独立**端点。
- **可逆性**：加鉴权是纯增量改动，回滚成本极低。
- **影响面**：`nexus-api` 一个端点 + 文档一处说明。

### 类别：decide-and-proceed

- **决策**：本任务**不**顺带实现 `nexus serve`。
- **依据**：`nexus serve` 属 Phase 5，是**独立**的、可独立评审的交付物。
  按 `ai-sdlc-decision-authority.mdc` §5「不扩大范围」——
  在 HTTP 端点里塞进整个服务启动是**范围蔓延**。
- **可逆性**：不适用。

---

## 实现过程中的发现

### 交付内容

| 文件 | 内容 |
|---|---|
| `crates/nexus-api/src/metrics.rs` | `/metrics` 端点（新增） |
| `crates/nexus-api/src/error.rs` | 协议层错误响应（新增） |
| `crates/nexus-api/src/lib.rs` | crate 级文档（替换原来的 TODO 占位） |
| `crates/nexus-api/tests/metrics_endpoint.rs` | 7 个行为层测试（新增） |
| `docs/operations/p3-runbook.md` | 「怎么看到这些指标」+ 采集配置 + 内网暴露约束 |

**不新增任何依赖** —— `axum` / `tower` / `serde_json` / `nexus-observability` 都已在
`nexus-api` 的依赖里，`nexus-observability` 也是**只读**使用（只调 `render()`）。

### 发现 1 —— 两条断言是**同义反复**，故障注入当场拆穿

第一次写完，注入验证时发现：

| 注入 | 期望 | 实际 |
|---|---|---|
| `METRICS_CONTENT_TYPE` 改成 `text/html` | 测试红 | **测试全绿 ✗** |
| `METRICS_PATH` 改成 `/metrics2` | 测试红 | **测试全绿 ✗** |

根因是同一个：**断言引用了被测代码里的常量**。

```rust
// ❌ 同义反复：改常量 → 两边同时变 → 永远绿
assert_eq!(content_type, METRICS_CONTENT_TYPE);

// ✅ 断言字面量
assert_eq!(content_type, "text/plain; version=0.0.4; charset=utf-8");
```

这是本仓库的**第三次**同义反复（P4 的「测试存在 ≠ 测试断言了那条不变量」
是同一个病根的另一个形态）。

**教训**：「引用被测常量」看着 DRY，实际是**断言的自我实现** ——
它验证的是「常量等于它自己」。

**代价**：这个缺陷在故障注入前**不会**暴露。门禁全绿、测试全绿，
但 AC #1 与端点路径**实际上没有被验证**。

### 发现 2 —— 「没有流量时指标不出现」是**必然**，不是缺陷

`prometheus` 的 `Registry::gather()` 会**剪掉空 family**：

```rust
// prometheus-0.14.0/src/registry.rs
if mf.get_metric().is_empty() {
    continue;
}
```

所以刚启动、还没有任何请求时，`/metrics` 返回的是**空体**（200，不是 404）。

根因是**标签口径**：标签值来自运行期配置（供应商代号），
启动时无从得知，所以没有样本就没有 family。

这**不是**可以「修掉」的问题 —— 除非预先创建一批标签值为占位符的样本，
那会**污染**指标（凭空多出 `provider=""` 的 series）。

所以处理方式是**如实记录**：runbook 里写明「没有流量时是空体」，
测试里也**先产生流量再断言**（而不是断言空体）。

### 发现 3 —— 「渲染另一个注册表」是最坏的静默失效形态

注入验证时专门试了它：

```rust
// 注入：新建一个独立注册表并渲染它
let registry = prometheus::Registry::new();
let body = TextEncoder::new().encode_to_string(&registry.gather())?;
```

症状是**端点返回 200、Content-Type 正确、响应体可解析** ——
**看起来完全正常**，但指标**永远是空的**。

3 个测试同时红（`the_response_contains_both_metric_definitions` /
`the_endpoint_requires_no_credentials` /
`the_cancelled_status_is_counted_separately`）。

这正是「指标产生却不可见」的**镜像**形态：
NEX-25 要解决的正是「指标产生但没人看」，
而这个注入展示了「没人看」的另一种成因 —— **看的是空的那本账**。

所以 `MetricsState` 的文档里明确写：
**不新建注册表**，因为那会造成两份互不相干的指标。

### 门禁自身的防伪验证（7 条注入全部验证过会红）

| 注入 | 结果 |
|---|---|
| `Content-Type` 改成 `text/html` | FAILED ✓（修正断言后） |
| 端点返回**空体** | 3 个测试同时红 ✓ |
| 丢掉 `llm_tokens_total` 的全部内容 | FAILED ✓ |
| 把 `canceled` 混进 `error` | FAILED ✓ |
| 端点路径改成 `/metrics2` | 5 个测试同时红 ✓（修正断言后） |
| 响应体里塞 `api_key` | FAILED ✓ |
| 渲染**另一个注册表** | 3 个测试同时红 ✓ |

### 一条方法论教训

**前两次注入失败，不是因为注入做得不对，而是因为断言本身没有断言力。**

如果当时因为「注入没红」就认为「这个断言没问题」，
那么 AC #1 与端点路径就会**带着一个失效的测试**进入 `main` ——
而门禁全绿、测试全绿，**没有任何信号**表明那里漏了。

这正是 NEX-22 教训 #4 的强化版：
**「没验证过会红的门禁，等于没有门禁」** ——
而「没验证过会红的**断言**」比那更隐蔽，
因为断言不是门禁，它看起来只是测试的一部分。

### 契约影响五问

1. 改动**错误码值**吗？ → **否**（只用 `Internal` 90001，已在 `nexus-core` 定稿的 42 个之内）
2. 改动**事件名或负载形状**吗？ → **否**
3. 改动 **ID 底层类型**吗？ → **否**
4. 改动 **crate 依赖方向**吗？ → **否**（`nexus-api` 的依赖表**未变**，不新增任何依赖）
5. 改动**对外响应形状**吗？ → **否**（`/metrics` 是**新增**端点，不改动任何既有响应）

### 验证结果

- `cargo test -p nexus-api --locked`：7 个测试全绿
- `cargo test --workspace --locked`：455 通过 / 0 失败（原 448）
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- 7 个组织纪律门禁全绿
