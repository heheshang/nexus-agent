# 适配器清单（外部系统边界）

> **判据在 `.cursor/rules/ai-sdlc-adapters.mdc`**。本清单回答：
> **本仓库实际接了哪些外部系统**、凭据怎么走、失败怎么翻译、现在是什么状态。
>
> 上游的适配器**注册与发现机制**（`metadata.yaml`、registry、git ref）**未落地** ——
> 本仓库的适配器是**仓库内的 Rust crate**，靠 `scripts/check-deps.sh` 静态校验。
> 理由见 `docs/ai-sdlc/coverage-matrix.md` §2 `spec/adapters.md` 行。

---

## 1. 三条纪律

外部系统会变、会挂、会被限流。把它们直接写进业务逻辑，
**可替换性**和**失败语义**都无从谈起。所以：

1. 外部系统的**形状**不得渗入领域模型（DTO 里不许出现供应商的字段名）。
2. 外部系统的**失败**必须被翻译成领域语义（`Retryable` / `UnknownOutcome` / `Terminal`）。
3. 外部系统的**凭据**不得进入日志、错误消息或代码库。

---

## 2. 清单

| 外部系统 | 契约类型 | 实现位置 | 凭据来源 | 当前状态 |
|---|---|---|---|---|
| **PostgreSQL** | 基础设施（数据访问） | `crates/nexus-infra/src/pg.rs`（连接池）<br>`crates/nexus-repo/`（行类型与仓储） | `NX_DATABASE__URL` | ✅ 已落地 |
| **Redis** | 基础设施（缓存 / 租户状态） | `crates/nexus-infra/src/redis.rs` | `NX_REDIS__URL` | ✅ 已落地 |
| **S3** | 基础设施（对象存储） | `crates/nexus-infra/src/s3.rs` | `NX_S3__ACCESS_KEY` / `NX_S3__SECRET_KEY` | ✅ 已落地 |
| **Milvus** | 基础设施（向量库） | `crates/nexus-infra/src/milvus.rs` | `NX_MILVUS__URL` | ✅ 已落地 |
| **模型供应商**（OpenAI 兼容） | 供应商（LLM 网关） | `crates/nexus-llm/`（客户端 + 网关） | `NX_LLM__DEEPSEEK__API_KEY` | 🚧 P3 进行中（NEX-19） |
| **GitHub** | 源码控制（IssueTracker / SourceControl） | `git` CLI + `gh` CLI（**仓库外**） | git / gh 自身凭据 | ⏳ 未接入 |

### 契约类型与上游的对应

| 上游契约 | 本仓库的实现 |
|---|---|
| `IssueTracker` | `backlog/`（任务载体）+ GitHub（**待接**） |
| `SourceControl` | `git` CLI + `gh` CLI |
| `CIPipeline` | `.github/workflows/ci.yml` |
| `CodeAnalysis` | `cargo clippy` / `cargo test` |
| `Messenger` | PR 评论 |
| `DeploymentTarget` | ⏳ 待接 |
| `SecretStore` | 环境变量 + `NX_` 映射表 |
| `AuditSink` | CI 日志 + 任务文件 |

---

## 3. 凭据管理

### 硬性规则

1. **凭据不得写入仓库** —— 含 `.env`、示例配置、测试夹具。
2. **凭据不得出现在日志 / 错误消息 / 提交信息**里。
3. 凭据**只在确实需要时才获取**，用完立刻吊销。
4. 需要凭据的动作 → **operator-only** 类别（`ai-sdlc-decision-authority.mdc` §2），
   代理**不得自行获取或使用**账号凭据。

### 本仓库的三层配置

```
base（打包进二进制的默认值）  <  file（configs/*.yaml）  <  env（NX_* 环境变量）
```

| 层 | 文件 | 入库 | 内容 |
|---|---|---|---|
| base | `crates/nexus-config/resources/default.yaml` | ✅ | 合理默认值，**不含**环境相关值 |
| file | `configs/config.yaml` | ✅ | 各环境共有的结构性配置 |
| file | `configs/local.yaml` | ❌ | 本地覆盖（由 `local.yaml.example` 复制） |
| file | `configs/secrets.yaml` | ❌ | 密钥（由 `secrets.yaml.example` 复制） |
| env | `NX_<段>__<段>__<键>` | — | 最高优先级，由**声明式映射表**驱动 |

**映射表**：`crates/nexus-config/src/mapping.rs` 的 `table()`。
它是一份**可枚举的静态清单** —— `nexus config env` 可打印，运维手册可引用，测试可断言。

> 为什么不用「自动匹配」：自动匹配让「哪些环境变量能覆盖什么」成为运行时的副作用 ——
> 不可枚举、不可文档化、不可测试。映射表把它变成静态清单。

### 密钥引用的写法

配置里**只写引用**，不写真值：

```yaml
llm:
  deepseek:
    api_key: ${env:NX_LLM__DEEPSEEK__API_KEY}
```
- **真值只从环境变量 / SecretStore 来**。
- 密钥**不得**出现在日志、错误消息、**快照测试的期望输出**里。

### `secrecy::SecretString`

模型供应商的 API key 用 `secrecy::SecretString` 承载 ——
`Debug` 输出**不含明文口令**。这是「凭据不得进入日志」的**类型层保障**，
不是靠调用方自觉。

---

## 4. 失败翻译

**供应商的错误码不得直接进入领域。**

| 供应商语义 | 领域语义 | 说明 |
|---|---|---|
| 明确拒绝（限流、参数错误） | `Retryable` | 退避后可重试 |
| 上游不可用（连接失败、5xx） | `Retryable` | 退避后可重试 |
| **超时** | **`UnknownOutcome`** | 请求**可能已到达上游并产生副作用** |
| 认证 / 授权失败 | `Terminal` | 重试无意义 |
| 请求非法 | `Terminal` | 重试无意义 |

**禁止**把超时与「明确拒绝」一视同仁 —— 这是参考实现的已知缺陷，
本项目在 `nexus-core` 的 `RetryPolicy` 里已修正。

### 其他规则

- 重试判据**只在 `nexus-core` 定义一次**，其他 crate **不得另写一套**。
- 熔断 / 重试 / 限流是**基础设施语义**，属于**被调用方**的责任，不是调用方的。
- **不得把 `UnknownOutcome` 的错误当可重试处理** —— 这条在本仓库是 `hard-mandatory` 红线。

---

## 5. 数据库与存储

| 规则 | 说明 |
|---|---|
| `sqlx` **只允许**出现在 `nexus-repo` / `nexus-infra` / `nexus-execution` | 其他 crate 出现即 `hard-mandatory` 违规，由 `scripts/check-deps.sh` 强制 |
| `nexus-model` 保持**纯 DTO** | 不得出现 `FromRow` / `sqlx::Type` / 任何 `sqlx::` 痕迹 |
| 迁移**经由 sqlx 记账** | 不得手工 `psql` 应用迁移（会绕过 `_sqlx_migrations` 记账，导致重复应用） |
| 迁移**必须可回滚** | 生产部署前确认回滚路径 |
| 行类型 `#[derive(FromRow)]` 定义在 `nexus-repo` | 保持数据访问细节收敛在一处 |

---

## 6. 未接入的外部系统

| 系统 | 缺失的契约 | 接入的前置条件 |
|---|---|---|
| **DeploymentTarget** | 部署与回滚 | 需要确定部署形态（容器编排？裸机？）与回滚策略 |
| **GitHub IssueTracker** | issue 双向同步 | 需要决定 backlog 与 issue 哪个是权威源，以及漂移如何判定 |

> **注意**：这两项**不是**「暂不需要」——
> **S7 合并与交付**与 **S8 部署与运维**这两个阶段，在它们接入前只能靠人工闭环。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `.cursor/rules/ai-sdlc-adapters.mdc` | **判据**（接口契约、凭据、配置、失败翻译） |
| `docs/operations/configuration.md` | 配置三层与 `NX_` 映射表全表 |
| `scripts/check-deps.sh` | `sqlx` 收敛与依赖方向的静态校验 |
| `crates/nexus-config/src/mapping.rs` | 声明式环境变量映射表 |
| `docs/ai-sdlc/coverage-matrix.md` | 上游适配器条款的适配状态 |
