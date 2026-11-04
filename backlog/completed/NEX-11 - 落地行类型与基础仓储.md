---
id: NEX-11
title: "feat(repo): 落地行类型与基础仓储"
status: To Do
assignee: []
labels: [repo, postgres, p2]
priority: high
dependencies:
  - NEX-8
  - NEX-9
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.4"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.9.3"
  - "docs/operations/p2-plan.md"
---

## Description

`nexus-repo` 的第一层：**行类型（`FromRow`）+ 基础仓储**。

### 职责边界（§4.2 差异 2）

| crate | 内容 | 禁止 |
|---|---|---|
| `nexus-model` | 领域类型（纯 DTO） | 任何数据库痕迹（`FromRow` / `sqlx::`） |
| `nexus-repo` | **行类型**（`#[derive(FromRow)]`）+ 仓储 | 业务语义 |

行类型定义在 `nexus-repo`（`#[derive(FromRow)]`），领域类型在 `nexus-model`。
两者**分开** —— 这是 §4.2 差异 2 的核心。

### SQL 校验方式：`query_as` + `FromRow`（§3.9.3）

**不用** `sqlx::query!` 编译期宏 —— 理由：

1. 项目处于重建期，schema 会频繁变（参考实现 13 个迁移说明迭代很快）
2. `FOR UPDATE SKIP LOCKED` / `ON CONFLICT` / `FILTER(WHERE ...)` 这类关键 SQL
   本来就要写原生语句，编译期宏对它们支持有限
3. Go 版的 GORM 本来也是**运行期**校验，语义一致，风险更低

**代价**：字段名写错运行时才炸。缓解手段是**启动期 smoke check**（NEX-8 的职责）。

### `nexus-repo` 的依赖方向

`nexus-repo` 依赖能力层（`nexus-rag` / `nexus-memory` / `nexus-vector` / `nexus-ingestion`）
**只为实现它们的 trait**。方向是 `repo → 能力层`，**绝不能反向**（§4.1.2）。

`scripts/check-deps.sh` 规则 5 强制「能力层不依赖 `nexus-repo`」。

### 本任务的范围

本任务只做**最基础**的那部分，为后续的租约（NEX-12）与 outbox（NEX-13）铺路：

- Phase 0 核心表的**行类型**
- 一个能证明形状正确的**仓储示例**（建议 `users` 或 `sessions` —— 简单、无租约语义）
- CRUD 的基本形状

**不做**：租约、fencing、outbox、幂等键 —— 那是 NEX-12 / NEX-13。

## Acceptance Criteria

- [ ] #1 `nexus-repo` 有行类型模块，Phase 0 核心表各有对应的 `#[derive(FromRow)]` 结构
- [ ] #2 行类型**只在** `nexus-repo` 定义 —— `nexus-model` 仍**零**数据库痕迹（`scripts/check-deps.sh` 规则 6）
- [ ] #3 SQL 一律用 `query_as` + `FromRow`，**不用** `query!` 编译期宏（无 `.sqlx` offline 目录）
- [ ] #4 至少一个仓储实现可用（`users` 或 `sessions`），带真实的集成测试
- [ ] #5 仓储错误**不 panic** —— 用 `Result` + `AppError`（NEX-6 的三层错误模型）
- [ ] #6 `nexus-repo` 的 `Cargo.toml` 内部依赖与 `scripts/check-deps.sh` 的 `deps_of` 声明**一致**
- [ ] #7 集成测试连真实 PostgreSQL —— 建表 → 插入 → 读回 → 断言
- [ ] #8 集成测试在**无数据库**时**跳过**（不 panic、不让 CI 红）—— 用 `DATABASE_URL` 存在性判断
- [ ] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #10 CI 六道门禁全绿

## 备注

- **集成测试必须有「无库则跳过」的机制** —— 否则每个没有 PG 的开发者
  （包括 CI 的非集成 job）都会看到红灯。
  `sqlx` 生态常用 `#[ignore]` + 环境变量判断，本项目沿用。
- 本地测试库用 `nexus_test`（独立于 `nexus_design`），且**每次重建** ——
  因为测试会插入数据。用 `nexus-test-pg` 容器 + `docker exec ... DROP DATABASE`。
- `AppError` 与 `ErrorCode` 已在 NEX-6 落地 ——
  `nexus-repo` 的错误**必须**用它们，不要自定义错误类型。
  `ErrorCode` 里与存储相关的：`StorageUnavailable`（503，可重试）、
  `RunNotFound` / `SessionNotFound`（404）、`RunConflict`（409）、`Internal`（500）。
- 行类型与领域类型的转换关系要在代码里显式 —— 不要让行类型**直接**流到能力层。
- `nexus-repo` 可以依赖 `nexus-capacity`（共享容量）—— 但那是 P6 的事，本任务不用。

## 实现过程中的发现

<!-- 实现时填写 -->
