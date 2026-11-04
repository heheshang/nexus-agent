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

- [x] #1 `nexus-infra` 有 `pg` 模块，导出 `PgPool` 的构造入口
- [x] #2 连接池从 `nexus-config` 的 `ConfigStack` 取配置（`database.url` / `max_connections` / `connect_timeout_secs`），**不自己读环境变量**
- [x] #3 `max_connections` 与 `connect_timeout_secs` 缺失时用**默认值**（10 / 5 秒），不报错
- [x] #4 连接 URL **绝不**出现在 `Debug` / `Display` / 日志里 —— 有测试断言
- [x] #5 `nexus-infra` 的 `Cargo.toml` 只依赖 `nexus-core` / `nexus-config` / `nexus-observability`（内部依赖）
- [x] #6 单元测试覆盖：配置缺失走默认值、配置完整走给定值
- [x] #7 构造连接池**不 panic** —— 连接失败返回 `Result`，不 unwrap
- [x] #8 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #9 CI 六道门禁全绿

## 备注

- **本任务不需要数据库** —— 只构造池对象，不建连。建连是 NEX-9 的事。
- `sqlx` 依赖已在根 manifest 声明（`sqlx 0.9.0`，features 含 `postgres` / `runtime-tokio` / `tls-rustls-ring` / `migrate`）。
  按纪律，成员 crate 写 `sqlx.workspace = true`。
- `PgPool` 本身是 `Clone` 的 —— 但本任务**不导出**任何可多写者的池句柄，
  「单写者」纪律在事件流那层（`nexus-core::event::EventSink`）已经确立，这里不重复发明。
- 不引入 `deadpool-postgres` 之类的第三方池 —— `sqlx::postgres::PgPoolOptions` 自带连接池，
  多一层抽象只会增加依赖面。

## 实现过程中的发现

**1. `sqlx 0.9` 移除了 `PgConnectOptions::connect_timeout` —— 配置项语义被迫改变。**
原计划把 `database.connect_timeout_secs` 映射到 `PgConnectOptions::connect_timeout`。
实测 `sqlx-postgres-0.9.0/src/options/mod.rs` —— `PgConnectOptions` 的字段里
**没有** `connect_timeout` 了（只有 `host` / `port` / `password` / `ssl_mode` 等）。
全 crate 搜索 `connect_timeout` 只在 `sqlx-core` 的 `PoolOptions` 里出现一次，
且那里的 `Debug` 实现把 `acquire_timeout` **显示成** `connect_timeout` 字段名
（`pool/options.rs:590`）—— 容易误导。

**sqlx 0.9 的实际做法**：连接建立超时统一由池的 `acquire_timeout` 兜底
（`pool/inner.rs:252` 以 `acquire_timeout` 为截止时间）。

所以本实现把 `database.connect_timeout_secs` 映射到 `PoolOptions::acquire_timeout`。
这与配置项的名字**不完全对齐** —— 但这是 sqlx 0.9 唯一可用的旋钮。
已在代码注释里写明这个偏离，避免后人以为配错了。

**2. `PgConnectOptions` 的派生 `Debug` 会**明文打印口令** —— 实测确认。**
`PgConnectOptions` 派生 `Debug`，内部持有 `password: Option<String>`，于是：

```text
PgConnectOptions { host: "127.0.0.1", ..., password: Some("supersecret"), ... }
```

只要这个类型被 `{:?}` 打进日志，口令就泄露了。
`secrecy::SecretString` 挡不住这个问题 —— 它保护的是**我们自己**的类型，
而 sqlx 的类型不受我们控制。

**已落地 `SafeConnectOptions` 包装**：手写 `Debug`，只输出 `host` / `port` / `database`，
口令一律 `[REDACTED]`。
「手写」而非「派生」是关键 —— 派生只能原样打印字段。

同时把 `connect_options`（返回裸类型）标了 ⚠️ 警告，
并让 `connect_lazy` 走安全包装 —— 裸类型**只在建连那一刻**出现。

**3. `connect_lazy_with` 必须在 Tokio 上下文里调用。**
初稿的惰性构造测试用普通 `#[test]`，结果 panic：
「this functionality requires a Tokio context」——
`connect_lazy_with` 会 spawn 一个后台连接维护任务
（`sqlx-core-0.9.0/src/pool/inner.rs:533`）。
已改成 `#[tokio::test]`，并在函数文档里写明这个前置条件。

**4. `SecretBox<str>` 没有实现 `PartialEq` / `Eq`。**
`PgSettings` 一开始派生 `PartialEq, Eq`，编译失败。
已去掉 —— 而且**这本身是对的**：让 secret 参与相等性比较可疑
（比较可能泄漏长度信息）。

**5. 配了 `0` 是明确错误，不能悄悄退回默认值。**
`max_connections = 0` / `connect_timeout_secs = 0` → 返回 `ErrorCode::InvalidParam`。
**缺失**才用默认值。
两者区别很重要：缺失是「没配」，退回默认合理；
配了 0 是「配错了」，退回默认会让运维以为配的值生效了。

**6. `serde_json` 只作为 dev-dependency 引入。**
测试要用 `json!` 宏构造 `ConfigStack`，但 `serde_json` **不是**运行时依赖 ——
放进 `[dependencies]` 会平白扩大依赖面。
`nexus-infra` 的运行时依赖只增加了 `secrecy`（根 manifest 早已声明）。
