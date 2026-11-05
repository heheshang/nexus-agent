---
id: NEX-33
title: "feat(api): 落地统一响应包与错误兜底"
status: To Do
assignee: []
labels: [api, contract, p5]
priority: high
dependencies: []
references:
  - "docs/specs/p5/spec.md §3.4"
  - "docs/specs/p5/tasks.md T-003"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.5"
---

## Description

决策 D11 的落地：统一响应包的形状。

成功 `{code: 0, message: "ok", data?}`；失败 `{code: <业务码>, message: "..."}`。

### 为什么 `code` 必须是**数字**

架构文档 §2.6.3 的**验证记录**（2026-11-04）：

> 派生 `Serialize` 会输出 `{"code":"InvalidParam"}`；手工实现后输出 `{"code":10001}`。
> 这个差异看似细节，实际决定了前端能否按码段分支。

所以 `code` 是 `i32` / `u32`，**不是**枚举的字符串变体名。

### 为什么成功用 `0`

业务码从 `10001` 起 —— `0` **不与任何业务码冲突**，
且前端可以按**同一个**分支处理「成功」与「失败」。

参考实现 `internal/pkg/response` **已经这么用** —— 前端**已经**按 `code == 0` 判成功。
改掉是**协议破坏性变更**。

### 类型级的「不外泄」

`AppError` **根本不实现 `Serialize`** —— 所以「不小心把 `cause` 发出去」
在**类型上**就不可能。响应体只序列化 `UserFacing`（`code` + `message`）。

## 范围边界

### 做

- `crates/nexus-api/src/response.rs`：统一响应包类型 + 构造器
- `crates/nexus-api/src/error.rs`：**修改** —— 对齐新响应包
- `crates/nexus-api/tests/response.rs`：行为层测试

### 不做

| 项 | 理由 |
|---|---|
| 分页包络 | 属具体业务端点（P6/P7/P8） |
| 错误码表本身 | 架构文档 §6.3 **已定稿**，P5 只复用 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/src/response.rs` 定义统一响应包 —— 成功 `{code: 0, message: "ok"}`，失败 `{code: <业务码>, message: "..."}`
- [ ] #2 `data` 缺省时**省略**该字段 —— 不输出 `"data": null`
- [ ] #3 `code` 是**数字** —— 不是字符串变体名
- [ ] #4 响应体**不含**内部原因（`cause`）
- [ ] #5 失败响应的 HTTP 状态码与业务码**一致**（`ErrorCode::http_status()`）
- [ ] #6 `crates/nexus-api/src/error.rs` 的 `ApiError` 的响应体**只有** `code` 与 `message` 两个字段
- [ ] #7 兜底路径：序列化失败时仍有**合法 JSON** body（不是空 body）
- [ ] #8 断言用的是**字面量** —— 不是引用被测代码里的常量（NEX-25 的教训）
- [ ] #9 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

9 条 AC 全部可二元判定 —— 每条都指明「函数存在 / 响应体形状可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.4` —— 存在
- `docs/specs/p5/tasks.md T-003` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.5` —— 存在
- `crates/nexus-core/src/error.rs`（`UserFacing` / `ErrorCode::http_status`）—— 存在

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增一个模块 + 修改一个已有模块。

**不触及**：42 个业务码的**值**、12 个事件名、18 个 ID 类型、crate 依赖方向。

> ⚠️ **但**：统一响应包的形状是**对外响应形状** ——
> 这是本任务**唯一**触及「对外响应形状」的地方。
> 变更性质：**新增**统一包络（NEX-25 的 `/metrics` 不受影响，它返回的是 Prometheus 文本）。
> **不改动**任何**既有**响应。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/response.rs` | **新增** |
| `crates/nexus-api/src/error.rs` | **修改** —— 对齐新响应包 |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/tests/response.rs` | **新增** |

### #6 完成态可描述

`nexus-api` 有一个统一响应包类型，成功与失败各有一条**确定的**序列化形状，
且响应体**在类型上**不可能包含内部原因。

### #7 无隐形依赖

- 无前置任务
- `serde` / `serde_json` 已在 `nexus-api` 的依赖里
- `nexus-core` 已在依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，可以开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：`/metrics` 端点**不**改用统一响应包 —— 它返回 Prometheus 文本格式。
- **依据**：Prometheus 抓取器要求 `text/plain` 文本格式，
  包上 JSON 包络会让抓取器**解析失败**。
  这是**格式契约**优先于**包络一致性**的正当例外。
- **可逆性**：高 —— 局部改动。
- **影响面**：`crates/nexus-api/src/metrics.rs`。
