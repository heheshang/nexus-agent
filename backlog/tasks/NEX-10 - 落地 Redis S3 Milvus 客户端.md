---
id: NEX-10
title: "feat(infra): 落地 Redis / S3 / Milvus 客户端"
status: To Do
assignee: []
labels: [infra, redis, s3, milvus, p2]
priority: medium
dependencies:
  - NEX-8
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.3.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §0.3.2"
  - "docs/operations/p2-plan.md"
---

## Description

`nexus-infra` 的其余三个后端客户端：**Redis / S3 / Milvus**。

`nexus-infra` 的职责边界（§4.3.3）：**客户端构造，不含任何业务语义**。
「Redis 存什么」「Milvus 建什么 collection」是业务决策，不属于本 crate。

### 三条已实测的 API 事实（§0.3.2）

| crate | 事实 |
|---|---|
| `redis 2.0.0-rc.2` | **`ConnectionManager::new(client)` 已改为 `async`**（1.x 是同步构造）。必须 `features = ["tokio-comp", "connection-manager", "script"]`。Lua 脚本用 `Script::new(...)` + `prepare_invoke()` |
| `milvus-sdk-rust 3.0.3` | 编译通过（运行时行为待 spike 验证） |
| `object_store 0.14.2` | 编译通过，`features = ["aws"]` |

### Redis 的定位

§1.3.1：**PostgreSQL 是唯一权威，Redis 只做唤醒提示**。
Redis 不是数据源 —— 丢一个通知只是延迟唤醒，不影响正确性。
这决定了 Redis 客户端的**降级策略**：Redis 不可用时，PG 侧仍应能工作。

### 配置

`nexus-config` 已提供全部映射，本任务**消费**不新增：

| 环境变量 | 配置路径 | 备注 |
|---|---|---|
| `REDIS__URL` | `redis.url` | secret |
| `MILVUS__URL` | `milvus.url` | |
| `S3__BUCKET` | `s3.bucket` | |
| `S3__ENDPOINT` | `s3.endpoint` | |
| `S3__ACCESS_KEY` | `s3.access_key` | secret |
| `S3__SECRET_KEY` | `s3.secret_key` | secret |

## Acceptance Criteria

- [ ] #1 `nexus-infra` 有 `redis` / `s3` / `milvus` 三个模块，各自导出客户端构造入口
- [ ] #2 三个模块都从 `nexus-config` 的 `ConfigStack` 取配置，**不自己读环境变量**
- [ ] #3 Redis 用 `ConnectionManager`（**异步**构造 —— `ConnectionManager::new(client).await`），且开了 `connection-manager` feature
- [ ] #4 Redis 客户端在连接失败时返回 `Result` —— **不 panic**，且**不阻断** PG 侧工作
- [ ] #5 S3 的 `access_key` / `secret_key` 与 Redis URL **绝不**出现在 `Debug` / `Display` / 日志里
- [ ] #6 Milvus / S3 的 URL / bucket 缺失时返回**明确的错误**（不是默认空串）
- [ ] #7 单元测试覆盖：配置缺失 → 明确错误；配置完整 → 构造成功
- [ ] #8 `nexus-infra` 的内部依赖仍只限 `nexus-core` / `nexus-config` / `nexus-observability`
- [ ] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #10 CI 六道门禁全绿

## 备注

- **本任务不需要真实后端** —— 只验证「配置解析 + 构造不 panic」。
  真实连接行为（Redis 读写、S3 上传、Milvus 建 collection）留给
  P7（知识与记忆）、P8（工具与协议）、P9（集成与容量）。
- Milvus 依赖 `milvus-sdk-rust 3.0.3`（根 manifest 里包名为 `milvus-sdk`）。
  注意 `nexus-vector`（能力层）**也**依赖 `milvus-sdk` ——
  两者不是重复依赖：`nexus-infra` 负责**连接**，`nexus-vector` 负责**schema / index 构造**（§4.3.3）。
- S3 用 `object_store`（抽象层）而非直接用 `aws-sdk-s3` ——
  这样本地 MinIO / 未来其他 S3 兼容后端都能用。
- Redis 的 `redis 2.0.0-rc.2` 是 **RC 版本** —— API 可能变。
  这条已在 §0.3.2 实测记录，用 `ConnectionManager::new` 的异步签名确认过。

## 实现过程中的发现

<!-- 实现时填写 -->
