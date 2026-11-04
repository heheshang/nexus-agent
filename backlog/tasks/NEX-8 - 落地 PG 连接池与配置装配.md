---
id: NEX-8
title: "feat(infra): 落地 PG 连接池与配置装配"
status: To Do
assignee: []
labels: [infra, postgres, p2]
priority: high
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.3.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §0.3.2"
  - "docs/operations/p2-plan.md"
---

## Description

P2 的地基：**`nexus-infra` 负责 PG / Redis / S3 / Milvus 的客户端构造，不含任何业务语义**（§4.3.3）。

参考实现对应 `internal/infra/milvus.go` + `internal/infra/pg.go` + `internal/infra/redis.go`。

本任务只做 **PG** 部分 —— 它是「迁移可应用」与「集成测试绿」两项判据的共同前提。

### 要点

**1. 配置来自 `nexus-config`，不自己读环境变量。**
`nexus-config` 已提供 `DATABASE__URL` / `DATABASE__MAX_CONNECTIONS` / `DATABASE__CONNECT_TIMEOUT_SECS`
到 `database.url` / `database.max_connections` / `database.connect_timeout_secs` 的映射。
本任务**消费** `ConfigStack`，不新增环境变量名。

**2. 连接 URL 是 secret。**
`database.url` 在 `nexus-config` 的映射表里已标 `.secret()`。
本任务不得把它打进日志、不得放进 `Debug` 输出。

**3. 连接池参数有默认值。**
`max_connections` 默认 10、`connect_timeout` 默认 5 秒 —— 缺失时不报错，用默认值。

**4. `nexus-infra` 不依赖任何上层 crate。**
§4.3.3 + `scripts/check-deps.sh` 规则 4：只能依赖 `nexus-core` / `nexus-config` / `nexus-observability`。

## Acceptance Criteria

- [ ] #1 `nexus-infra` 有 `pg` 模块，导出 `PgPool` 的构造入口
- [ ] #2 连接池从 `nexus-config` 的 `ConfigStack` 取配置（`database.url` / `max_connections` / `connect_timeout_secs`），**不自己读环境变量**
- [ ] #3 `max_connections` 与 `connect_timeout_secs` 缺失时用**默认值**（10 / 5 秒），不报错
- [ ] #4 连接 URL **绝不**出现在 `Debug` / `Display` / 日志里 —— 有测试断言
- [ ] #5 `nexus-infra` 的 `Cargo.toml` 只依赖 `nexus-core` / `nexus-config` / `nexus-observability`（内部依赖）
- [ ] #6 单元测试覆盖：配置缺失走默认值、配置完整走给定值
- [ ] #7 构造连接池**不 panic** —— 连接失败返回 `Result`，不 unwrap
- [ ] #8 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #9 CI 六道门禁全绿

## 备注

- **本任务不需要数据库** —— 只构造池对象，不建连。建连是 NEX-9 的事。
- `sqlx` 依赖已在根 manifest 声明（`sqlx 0.9.0`，features 含 `postgres` / `runtime-tokio` / `tls-rustls-ring` / `migrate`）。
  按纪律，成员 crate 写 `sqlx.workspace = true`。
- `PgPool` 本身是 `Clone` 的 —— 但本任务**不导出**任何可多写者的池句柄，
  「单写者」纪律在事件流那层（`nexus-core::event::EventSink`）已经确立，这里不重复发明。
- 不引入 `deadpool-postgres` 之类的第三方池 —— `sqlx::postgres::PgPoolOptions` 自带连接池，
  多一层抽象只会增加依赖面。

## 实现过程中的发现

<!-- 实现时填写 -->
