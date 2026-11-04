---
id: NEX-11
title: "feat(repo): 落地行类型与基础仓储"
status: Done
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

- [x] #1 `nexus-repo` 有行类型模块，Phase 0 核心表各有对应的 `#[derive(FromRow)]` 结构
- [x] #2 行类型**只在** `nexus-repo` 定义 —— `nexus-model` 仍**零**数据库痕迹（`scripts/check-deps.sh` 规则 6）
- [x] #3 SQL 一律用 `query_as` + `FromRow`，**不用** `query!` 编译期宏（无 `.sqlx` offline 目录）
- [x] #4 至少一个仓储实现可用（`users` 或 `sessions`），带真实的集成测试
- [x] #5 仓储错误**不 panic** —— 用 `Result` + `AppError`（NEX-6 的三层错误模型）
- [x] #6 `nexus-repo` 的 `Cargo.toml` 内部依赖与 `scripts/check-deps.sh` 的 `deps_of` 声明**一致**
- [x] #7 集成测试连真实 PostgreSQL —— 建表 → 插入 → 读回 → 断言
- [x] #8 集成测试在**无数据库**时**跳过**（不 panic、不让 CI 红）—— 用 `DATABASE_URL` 存在性判断
- [x] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #10 CI 六道门禁全绿

## 交付内容

| 文件 | 内容 |
|---|---|
| `crates/nexus-repo/src/rows.rs` | 17 个行类型（`#[derive(FromRow)]`）+ `NullableId<T>` + `Numeric` |
| `crates/nexus-repo/src/convert.rs` | 行类型 → 领域类型的**显式**转换（`user` / `user_credential` / `session` / `message` / `trim_bpchar`） |
| `crates/nexus-repo/src/error.rs` | `sqlx::Error` → `AppError` 的**唯一**转换点 |
| `crates/nexus-repo/src/users.rs` | 用户仓储（9 个方法） |
| `crates/nexus-repo/src/sessions.rs` | 会话仓储（7 个方法） |
| `crates/nexus-model/src/user.rs` / `session.rs` | 领域类型（纯 DTO） |
| `crates/nexus-repo/tests/common/mod.rs` | 集成测试基座（有库则跑、无库则跳过） |
| `crates/nexus-repo/tests/schema_conformance.rs` | 行类型 ↔ 真实 schema 的一致性核对 |
| `crates/nexus-repo/tests/architecture.rs` | 结构层架构测试 |

## 交付内容

| 文件 | 内容 |
|---|---|
| `crates/nexus-repo/src/rows.rs` | 17 个行类型（`#[derive(FromRow)]`）+ `NullableId<T>` + `Numeric` |
| `crates/nexus-repo/src/convert.rs` | 行类型 → 领域类型的**显式**转换（`user` / `user_credential` / `session` / `message` / `trim_bpchar`） |
| `crates/nexus-repo/src/error.rs` | `sqlx::Error` → `AppError` 的**唯一**转换点 |
| `crates/nexus-repo/src/users.rs` | 用户仓储（9 个方法） |
| `crates/nexus-repo/src/sessions.rs` | 会话仓储（7 个方法） |
| `crates/nexus-model/src/user.rs` / `session.rs` | 领域类型（纯 DTO） |
| `crates/nexus-repo/tests/common/mod.rs` | 集成测试基座（有库则跑、无库则跳过） |
| `crates/nexus-repo/tests/schema_conformance.rs` | 行类型 ↔ 真实 schema 的一致性核对 |
| `crates/nexus-repo/tests/architecture.rs` | 结构层架构测试 |

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

### 1. `query_as` 的代价必须有人兜底 —— 任务书低估了这一点

任务书说「字段名写错运行时才炸。缓解手段是**启动期 smoke check**」。

但 `nexus-infra` 的 `assert_phase0_tables` 只断言**表存在**，
**不校验列名** —— 表存在检查对列名对不对**毫无帮助**。

所以本任务补了 `tests/schema_conformance.rs`：把 17 个行类型的列名集合
与真实 PG 的 `information_schema.columns` **逐表比对**，要求**完全相等**。

两点设计取舍：

- **期望值是手写的**，不是从 `rows.rs` 自动推导的。
  自动推导的话，行类型写错列名时期望值也跟着错，测试永远不会失败。
  代价是改表结构要改两处 —— 第二处忘了改 → 测试变红 → 正是想要的。
- **用集合相等而不是子集**：子集检查会放过「行类型少定义了一个字段」，
  那种情况下 `FromRow` 报的是 `ColumnNotFound`，错误信息指向解码器
  而不是「你少定义了一个字段」。

负向验证：把 `session_id` 改成 `session_id_typo` → 测试精确报出
「`MessageRow` ↔ `messages`：行类型多出/写错的列 `["session_id_typo"]`」。

### 2. `sqlx` 的 `NUMERIC` 需要 `rust_decimal` feature

`model_configs.input_price` / `output_price` 是 `NUMERIC(12,6)`。
PG 的 `NUMERIC` 在**二进制协议**下有独立编码（不是 IEEE754），
只有 `rust_decimal` 能解码 —— 用 `f64` 会在 sqlx 的类型兼容性检查处
直接失败（`FLOAT8 ≠ NUMERIC`）。

顺带确认了价格字段用精确十进制而非浮点是**对的**：浮点的 15 位有效数字
不足以保证金额精度。已在根 `Cargo.toml` 的 `sqlx` features 里加上 `rust_decimal`。

### 3. `try_from` 不接受 `Option<i64> → Option<UserId>`

std 不提供 `Option<T>: From<Option<U>>`（只在 `U == T` 时成立），
而 `Option<UserId>` 与 `Option` 都不是 `#[fundamental]` ——
孤儿规则会拒绝直接补 impl。

所以可空外键用**本 crate 自己的** `NullableId<T>`：
它是本 crate 的类型，孤儿规则允许为它实现 `TryFrom`。

### 4. `CHAR(64)` 的 `bpchar` 会补尾随空格，但比较时忽略空格

`workflow_versions.definition_hash` / `execution_runs.definition_hash`
是 `CHAR(64)`。写入不足 64 字符的值会被**补空格**；
但 PG 的 `bpchar` 比较语义**忽略尾随空格** ——
所以**数据库层面看不出差异**，Rust 侧拿到的却是带空格的字符串。

这是「看着像细节、实际会炸」的坑。归一化放在 `convert::trim_bpchar`，
**不放在 SQL 侧** —— 加 `TRIM` 会让行类型只能配特定 SQL 用，失去通用性。

### 5. `messages.session_id` 没有 `ON DELETE CASCADE`

迁移 6 里 `messages.session_id → sessions.id` 是普通外键，
**没有级联删除**。所以「有消息的会话删不掉」——
`delete` 会因外键约束冲突返回 `InvalidParam`。

这不是 bug，是**当前 schema 的真实行为**。要改就得补一条迁移，
那是**破坏性契约变更**，超出本任务范围。
集成测试 `delete_fails_while_messages_exist` 把这个行为**记录**下来，
避免后人误以为是 bug 而「顺手修掉」。

### 6. `users.status` 的测试隔离维度选错了（踩过一次坑）

初版测试用固定的 `status = 7` 做隔离，结果在有历史残留数据的测试库上**假失败** ——
残留的 8 条 `status=7` 数据把刚创建的用户挤出了分页首页。

**根因不是测试太严格，而是隔离维度选错了**：
列表接口的分页语义天然受同状态的历史行影响，
隔离维度必须落在「被查询的那个键」上。

改法：用**随机**的 `status` 值。`status` 是 `SMALLINT`（取值空间有限），
所以从**负半区**（`[-32768, -1]`）随机取 —— 正半区留给真实业务语义
（`0` = 禁用、`1` = 有效），负半区在业务上无意义，不会与真实数据混淆。

### 7. 删掉了 `error::not_found` —— 未被使用的公共 API 是负债

初版有个 `not_found(message)` 辅助函数，映射到 `ErrorCode::Internal`。
但**没有任何调用点**：具体仓储都需要更精确的码
（`users` → `Unauthorized`，`sessions` → `SessionNotFound`）。

一个未被使用、且默认值会误导调用方（把「不存在」标成 `Internal`）
的公共函数，不如没有。已删除。

### 8. `check-naming.sh` 漏检「英文前缀 + 中文」

本任务附带修掉的**独立缺陷**：命名检查脚本的 Rust 正则只匹配
「中文**开头**」的函数名，导致 `fn sqlx_只出现在数据访问层()`
这类「英文前缀 + 中文在后」的写法长期漏检。

已加固正则（匹配标识符**任意位置**含 Han 字符），
并改名了仓库里 11 个含中文的函数。详见 `.cursor/rules/rust-code-standards.mdc` §11。
