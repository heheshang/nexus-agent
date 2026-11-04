# P2 · 基础设施

架构文档 §6.1 的 P2 阶段：**`nexus-infra`（PG/Redis/S3/Milvus）、`nexus-repo`**。

**状态：✅ 完成（NEX-8 ~ NEX-14）**

## 完成判据

架构文档 §6.1：**「迁移可应用、集成测试绿」**

拆成两个可验证的子目标：

1. **迁移可应用** —— `sqlx::migrate!` 能在空库上完整建表，且可重复执行（幂等）。
2. **集成测试绿** —— 真实 PostgreSQL 上的行为层不变量测试通过。

### 「绿」的**防伪**要求（NEX-14 确立）

「集成测试绿」这个词本身是个**陷阱** —— 有三种方式可以让它**假绿**：

| 假象形态 | 防线 |
|---|---|
| 没有 `DATABASE_URL` → 测试全部「跳过」 → 摘要显示 `passed` | 集成测试基座在 CI 环境**直接 panic** |
| CI 的 PG 服务是**空库** → 表不存在 → 测试报错 | CI 增加「应用数据库迁移」步骤（`cargo run -p nexus-infra --bin apply-migrations`） |
| 测试套件**根本没被编进来** | CI 解析 `cargo test` 输出，断言各集成套件的测试数达阈值 |

三条防线都用**注入故障**的方式验证过会红。

### 验证命令

```bash
# 应用迁移到空库（CI 里就是这么做的）
DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_test \
  cargo run -p nexus-infra --bin apply-migrations

# 跑集成测试
DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_test cargo test -p nexus-repo
```

## 任务拆解

| 任务 | 内容 | 依赖 | 对应设计条款 |
|---|---|---|---|
| **NEX-8** | `nexus-infra` 的 PG 连接池与配置装配 | — | §4.3.3、§5.5 |
| **NEX-9** | 迁移基线 —— 核心表 schema（sqlx 迁移） | NEX-8 | §3.9、§3.9.4 |
| **NEX-10** | `nexus-infra` 的 Redis / S3 / Milvus 客户端 | NEX-8 | §4.3.3、§0.3.2 |
| **NEX-11** | `nexus-repo` 的行类型与基础仓储 | NEX-8、NEX-9 | §4.2 差异 2、§5.4 |
| **NEX-12** | 租约与 generation fencing（`RunLease`） | NEX-11 | §2.5.4、§3.4.2 |
| **NEX-13** | outbox 与幂等键（事务一致性） | NEX-11、NEX-12 | §4.1.1、§4.1.2 |
| **NEX-14** | P2 收口：集成测试基座 + 架构文档同步 | NEX-8..13 | §5.3、§5.4、§4.2 |

依赖图：

```
NEX-8 (PG 连接池 + 配置)
  ├── NEX-9 (迁移基线)  ──┐
  └── NEX-10 (Redis/S3/Milvus)
                          │
                          └── NEX-11 (行类型 + 基础仓储)
                                ├── NEX-12 (租约 + fencing)
                                │     └── NEX-13 (outbox + 幂等)
                                └── NEX-14 (收口)
```

## 三条不可让步的红线

### 1. 契约不可改

迁移文件定义了表结构 —— 那就是**持久化契约**。改列名、类型、约束都是破坏性变更。

实现与参考实现的 schema 不符时，**是实现错了，不是契约错了**（§6.3 纪律）。
唯一允许的偏离是架构文档**明文规定**的两处（见下「刻意偏离」）。

### 2. `sqlx` 只出现在三个 crate

`nexus-infra` / `nexus-repo` / `nexus-execution`。
`scripts/check-deps.sh` 规则 3 强制。**能力层出现 `sqlx` 就是违规** ——
那意味着业务语义渗进了数据访问。

### 3. `nexus-repo` 不得引用 `workflow_runs` 的类型

§4.2 差异 1：`workflow_runs` 由 `nexus-execution` 独占。
`nexus-repo` 依赖 `nexus-rag` / `nexus-memory` 等能力层只是为了**实现它们的 trait** ——
依赖方向是 `repo → 能力层`，绝不能反向。

## 两处刻意的偏离（架构文档明文规定）

| 偏离 | 参考实现 | Rust 版 | 依据 |
|---|---|---|---|
| 执行表 | 迁移 004 给 `workflow_runs` **加 11 列** | **新建 `execution_runs`**，与 `workflow_runs` 分开 | §3.9.4 —— 避免历史数据迁移的复杂度 |
| 建表节奏 | 13 组迁移 43 表一次建完 | **按 Phase 渐进建表**，核心表 Phase 0 一次建齐 | §3.9.4 —— 避免后期反复改表 |

**Phase 0 核心表**（§3.9.4 点名）：
`users` / `agents` / `tools` / `sessions` / `messages` / `workflow_runs` / `execution_*`

## 三条已定稿的技术决策

| 决策 | 选择 | 依据 |
|---|---|---|
| SQL 校验方式 | **`query_as` + `FromRow`**（运行期），**不用** `query!` 编译期宏 | §3.9.3 —— schema 频繁变 + `SKIP LOCKED` / `ON CONFLICT` 本来就要写原生 SQL |
| 迁移工具 | **`sqlx::migrate!`**（可逆格式 `.up.sql` / `.down.sql`） | §3.9.3 + **实测源码** —— sqlx 0.9 原生支持可逆迁移（详见 NEX-9） |
| 启动期健康检查 | 启动时跑一次关键表 smoke check | §3.9.3 —— 缓解 `query_as` 的「字段名写错运行时才炸」 |
| 连接超时旋钮 | `database.connect_timeout_secs` → `PoolOptions::acquire_timeout` | **实测**：sqlx 0.9 移除了 `PgConnectOptions::connect_timeout`（详见 NEX-8） |
| 凭据承载 | `secrecy::SecretString` + **手写** `Debug` 的包装 | sqlx 的 `PgConnectOptions` 派生 `Debug`，**明文打印口令**（详见 NEX-8） |

### 实施时实测更正的两处文档偏差

| 文档原文 | 实测结论 | 证据 |
|---|---|---|
| 「`sqlx::migrate!` **只支持 up 迁移**」 | **已过时** —— sqlx 0.9 原生支持 `.up.sql` / `.down.sql`，且 `Migrator::undo()` 是 API 能力 | `sqlx-core-0.9.0/src/migrate/migration_type.rs:19`、`migrator.rs:314` |
| 「13 组迁移、**43 表**」 | 实为 **42 表 + 1 视图**（`execution_active_capacity` 是视图） | `information_schema` 实测：`BASE TABLE 42` + `VIEW 1` |

两处已在参考架构文档中更正，并记入附录。

## 已知风险与前置条件

| 项 | 说明 | 应对 |
|---|---|---|
| **需要 PostgreSQL** | P2 判据含「集成测试绿」，必须有真实 PG | 本机已有 `nexus-test-pg`（PG 17，42 表 + 1 视图已应用）。CI 需 `services:` 拉起 PG |
| **Redis / S3 / Milvus** | 三个后端不一定都有 | NEX-10 的客户端构造可**离线测**（配置解析 + 构造不 panic）；连接行为留 P7/P9 |
| **`redis 2.0.0-rc.2`** | `ConnectionManager::new` 已改为 **async** | §0.3.2 实测记录 —— 连接管理器必须 `await` |

## 验证方式

```bash
# 门禁（七道）
bash scripts/check-lock.sh && bash scripts/check-deps.sh \
  && bash scripts/check-naming.sh && bash scripts/check-backlog.sh \
  && cargo fmt --all --check \
  && cargo clippy --workspace --all-targets --locked -- -D warnings \
  && cargo test --workspace --locked

# 集成测试（需数据库）
DATABASE_URL=postgres://nexus:nexus@127.0.0.1:5432/nexus_test cargo test -p nexus-repo
```
