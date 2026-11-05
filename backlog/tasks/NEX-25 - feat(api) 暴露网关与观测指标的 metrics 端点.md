---
id: NEX-25
title: "feat(api): 暴露网关与观测指标的 /metrics 端点"
status: To Do
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

- [ ] #1 `nexus-api` 暴露 `GET /metrics`，返回 **Prometheus 文本格式**（`text/plain; version=0.0.4`）
- [ ] #2 响应体**包含** `llm_requests_total` 与 `llm_tokens_total` 的指标定义
- [ ] #3 端点**不要求**任何凭据即可访问
- [ ] #4 响应体中**不包含** `api_key` 或任何凭据字段 —— 有测试断言
- [ ] #5 端点有**行为层测试**：请求后能拿到可解析的 Prometheus 文本
- [ ] #6 `docs/operations/p3-runbook.md` 写明「怎么访问 `/metrics`」与「它只应内网暴露」
- [ ] #7 `docs/operations/p3-runbook.md` 的「可观测」章节补上「指标怎么被采集」这一环
- [ ] #8 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [ ] #9 CI 全部门禁全绿

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

（待补）
