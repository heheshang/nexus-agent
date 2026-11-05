---
id: NEX-34
title: "feat(api): 落地协议层中间件与路由骨架"
status: To Do
assignee: []
labels: [api, middleware, p5]
priority: high
dependencies:
  - NEX-32
  - NEX-33
references:
  - "docs/specs/p5/spec.md §1.1、§3.2、§3.3、§3.5、§3.6"
  - "docs/specs/p5/tasks.md T-004"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §1.4"
---

## Description

决策 D10 的落地：鉴权的**机制**在 P5，**策略**在上层（trait 接缝）。

路由可**增量注册**；交付**一个**真实可跑的 SSE 端点（数据源是注入的 trait）。

### 为什么要 trait 而不是直接调 `nexus-security`

`nexus-security` 是 **7 行空壳** —— JWT 校验的具体形态**还没定**。

直接调它会让 P5 **阻塞于**一个还没做的 crate，而 P5 与 P8
（`nexus-security` 落地）**没有顺序依赖**。trait 让两者**并行**。

参考架构 §0.4 的 Go 版反例 → Rust 落点明确写着
「**权限由 service 注入**的 `SetAccessCheck` 回调完成」。

这与 P4 的 D4（`nexus-agent` 自定义 trait）是**同一条纪律**。

### 必须交付一个**真实可跑**的端点

决策 D9 的反方论点成立：「只交付骨架，端点就是**空壳**」。

所以本任务交付**一个** SSE 端点 —— 它的数据源是一个**注入的**事件流提供器
（trait），所以它**真实可跑**（用本地 mock 就能看到完整 SSE 帧流）。

## 范围边界

### 做

- `crates/nexus-api/src/middleware.rs`：`IdentityExtractor` trait + 中间件
- `crates/nexus-api/src/router.rs`：可增量注册的路由构建器 + 一个 SSE 端点
- `crates/nexus-api/tests/middleware.rs` + `tests/architecture.rs`

### 不做

| 项 | 理由 |
|---|---|
| 具体的 JWT 校验 | `nexus-security` 是空壳；属 P8 |
| 业务端点（KB / 工作流 / 记忆） | 属 P6/P7/P8 |
| `nexus-service` 的实现 | 决策 D9 —— P5 是**传输层** |

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/src/middleware.rs` 定义 `IdentityExtractor` trait —— `nexus-api` **不实现**具体的 JWT 校验
- [ ] #2 提取失败 → `401` / `403` + 统一响应包
- [ ] #3 提取成功 → 身份放进请求 `Extension`，供 handler 取用
- [ ] #4 `crates/nexus-api/src/router.rs` 提供**可增量注册**的路由构建器
- [ ] #5 交付**一个**真实可跑的 SSE 端点 —— 数据源是**注入的** trait（不依赖 `nexus-service` 的具体实现）
- [ ] #6 `nexus-api` **不引用** `nexus-service` 的**具体类型**（同 P4 的 D4 纪律）
- [ ] #7 `crates/nexus-api/tests/architecture.rs` 断言 `nexus-api` 不依赖 `nexus-repo` / `nexus-infra`
- [ ] #8 `crates/nexus-api/Cargo.toml` **不新增**任何内部 crate 依赖
- [ ] #9 路由层注入**心跳** —— 空闲连接按配置间隔写出 `: ping\n\n` 注释帧；桥接**不自带**定时器（spec §3.2）
- [ ] #10 **请求 ID 透传** —— 中间件为每个请求分配或透传请求 ID，放进请求 `Extension` 与响应头（spec §1.1）
- [ ] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

11 条 AC 全部可二元判定 —— 每条都指明「trait / 函数存在 / 行为可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

「哪些端点挂鉴权」是**显式排除**（范围边界表），不是未决问题 ——
P5 交付**机制**，逐端点决定由上层做。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.3、§3.5、§3.6` —— 存在
- `docs/specs/p5/tasks.md T-004` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §1.4` —— 存在
- `scripts/check-deps.sh`（依赖方向表）—— 存在

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增两个模块 + 测试。

**不触及**：42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/middleware.rs` | **新增** |
| `crates/nexus-api/src/router.rs` | **新增** |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/tests/middleware.rs` | **新增** |
| `crates/nexus-api/tests/architecture.rs` | **新增/修改** |
| `crates/nexus-api/Cargo.toml` | **不修改** |

### #6 完成态可描述

`nexus-api` 有一个可增量注册的路由构建器、一个**真实可跑**的 SSE 端点，
以及一套**可替换**的鉴权机制（trait 接缝）。

### #7 无隐形依赖

- 前置任务 NEX-32 / NEX-33 均已登记在 `backlog/tasks/`
- `axum` / `tower` / `tokio` 已在 `nexus-api` 的依赖里
- `nexus-agent` / `nexus-core` 已在依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-32 / NEX-33 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：SSE 端点的数据源用 **trait**（`EventStreamProvider` 之类）注入，
  而不是直接调 `nexus-service`。
- **依据**：决策 D9；架构文档 §1.4「协议层不能感知业务规则」；
  参考架构 §0.4「权限由 service 注入」的同构做法。
  同时这让 T-004 的测试能**不依赖任何业务实现**就跑起来。
- **可逆性**：高 —— trait 是接口，后续填实现**不改** `nexus-api`。
- **影响面**：`crates/nexus-api/src/router.rs`。
