---
id: NEX-10
title: "feat(infra): 落地 Redis / S3 / Milvus 客户端"
status: Done
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

- [x] #1 `nexus-infra` 有 `redis` / `s3` / `milvus` 三个模块，各自导出客户端构造入口
- [x] #2 三个模块都从 `nexus-config` 的 `ConfigStack` 取配置，**不自己读环境变量**
- [x] #3 Redis 用 `ConnectionManager`（**异步**构造 —— `ConnectionManager::new(client).await`），且开了 `connection-manager` feature
- [x] #4 Redis 客户端在连接失败时返回 `Result` —— **不 panic**，且**不阻断** PG 侧工作
- [x] #5 S3 的 `access_key` / `secret_key` 与 Redis URL **绝不**出现在 `Debug` / `Display` / 日志里
- [x] #6 Milvus / S3 的 URL / bucket 缺失时返回**明确的错误**（不是默认空串）
- [x] #7 单元测试覆盖：配置缺失 → 明确错误；配置完整 → 构造成功
- [x] #8 `nexus-infra` 的内部依赖仍只限 `nexus-core` / `nexus-config` / `nexus-observability`
- [x] #9 `cargo test --workspace --locked` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [x] #10 CI 六道门禁全绿

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

**1. redis 2.0 没有 `ConnectionManager::new_lazy` —— 只有 `new_lazy_with_config`。**
§0.3.2 记的是 `ConnectionManager::new(client)` 改成 async，
但**惰性**构造的入口名没记。实测 `redis-2.0.0-rc.2/src/aio/connection_manager.rs:471`
只有 `new_lazy_with_config(client, config)`。
所以惰性构造必须传一个 `ConnectionManagerConfig`。

**2. redis 的 `ConnectionManager::new` 最坏要挂 30 秒 —— 必须自己套总预算。**
这是本次最重要的发现。实测：连一个不可达地址，`ConnectionManager::new` 耗时 **30 秒**。

原因（实测 `redis-2.0.0-rc.2/src/aio/connection_manager.rs:481-489`）：
`ConnectionManager` 内部用 `ExponentialBuilder` 重试，
缺省 `number_of_retries = 6` + 指数退避：

```
单次尝试 1s × 6 次 + 退避 0.1+0.2+0.4+0.8+1.6+3.2 ≈ 6.3s
→ 实测 30 秒
```

对一个「纯派生 · 可丢弃」的依赖，30 秒的启动阻塞不可接受 ——
§1.3.1 说 Redis 不可用时系统必须照常工作。

**关键取舍**：用 `set_number_of_retries` 限流是**错的** ——
那个参数同时管着**断线后的自动重连**，调到 0 会让
「Redis 短暂抖动后自动恢复」这个能力消失，而那是 `ConnectionManager` 存在的理由。

所以：**不动 redis 的重试策略**，用 `tokio::time::timeout` 给
**我们自己的急切构造调用**套一个总预算（3 秒）。
超时了就返回错误让调用方降级 —— 管理器反正会被丢弃。

**3. 本机网络的出站 TCP 是全部接受的 —— 「黑洞 IP」不能当不可达地址用。**
最初用 `192.0.2.1`（RFC 5737 的 TEST-NET-1）做「不可达」测试地址。
实测 `nc -z 192.0.2.1 19530` **成功** —— 这台机器的网络接受了所有出站 TCP 连接。

那意味着这类测试的结果**依赖运行环境**（换个网络环境行为就不同）——
不可接受。

改用**本机临时监听**：绑 `127.0.0.1:0`（内核分配端口，不会撞），
后台线程只 `accept` 不读写 —— 得到一个**确定的**「接受 TCP 但永不回应」的对端。

**4. milvus 的构造**只做 TCP 连接**，不做 HTTP/2 握手。**
实测（`tonic-0.13.1/src/transport/channel/service/connection.rs:90`）：
`Connection::connect` 走的是 `ready_oneshot()` ——
那只等**连接器**就绪（TCP 建连），协议握手是**延迟**的。

后果：对端「接受 TCP 但不回应」时，`milvus::connect()` **会成功**，
错误要到**第一次真正的 RPC** 才暴露。

**这不是 bug，是 gRPC 客户端的常态**。但调用方必须知道 ——
**不能**把「`connect()` 返回 `Ok`」当成「Milvus 已就绪」。
已写进模块文档并有测试断言这个行为。

**5. milvus 的 `timeout()` 是「请求超时」，不是「建连超时」。**
实测 `milvus-sdk-rust-3.0.3/src/v1/client.rs:180-183`：
`ClientBuilder::timeout()` 只设 tonic 的 `Endpoint::timeout`。
建连超时是 tonic 的 `connect_timeout`（`endpoint.rs:431`），**未被设置**。

后果：遇到「SYN 被丢弃」的主机，建连会挂到 OS 的 TCP 超时（**数分钟**）。
同样用 `tokio::time::timeout` 套总预算解决。

**6. milvus 的 `ClientBuilder::new` 要求 `D: 'static`。**
传 `&str` 借用会报 `borrowed value does not live long enough` ——
`ClientBuilder<D>` 的 `D: TryInto<Endpoint> + Clone`，
而 `Endpoint` 要求 `'static`。必须传**拥有所有权**的 `String`。

**7. milvus 的 `AuthInterceptor` 派生 `Debug`，会打印认证 token。**
实测 `milvus-sdk-rust-3.0.3/src/v1/client.rs:147`：
`Client` 派生 `Debug` → 内部持有 `AuthInterceptor { token: Option<String> }`，
而那个 token 是 `base64(username:password)`（`src/v1/client.rs:187-192`）——
**派生 `Debug` 会把它打印出来**。

这比 redis / object_store 都严重（那两个上游自己手写了脱敏的 `Debug`）。
所以 `MilvusHandle` 的 `Debug` **必须**手写。

**8. 三个上游的 `Debug` 脱敏都「碰巧」是对的 —— 但不能依赖。**
逐个查过：

| 类型 | 上游行为 |
|---|---|
| `redis::ConnectionManager` | 派生 `Debug` → `RedisConnectionInfo` **手写** `Debug`，password 显示 `<redacted>`（`connection.rs:455-475`） |
| `object_store::AmazonS3` | 派生 `Debug` → `AwsCredential` **手写** `Debug`，secret 显示 `******`（`aws/credential.rs:83-92`） |
| `milvus_sdk::client::Client` | 派生 `Debug` → `AuthInterceptor` 派生 `Debug` → **token 明文** |

三个上游的脱敏都依赖**它们当前的实现细节**。
本仓库**手写** `Debug`（三个 handle 都不打印任何字段），
并且**额外**加了一条测试断言「上游原始类型的 `Debug` 也不泄露」——
上游哪天改回派生，那条测试会立刻红，提醒我们「自己的手写 `Debug` 不是唯一防线」。

**9. `object_store` 默认 `allow_http = false` —— 只允许 https。**
实测 `object_store-0.14.2/src/client/mod.rs:973`：
`.https_only(!self.allow_http.get()?)`。
MinIO / 本地 S3 兼容端点走 http，不开这个开关**根本连不上**。

但**没有**无条件打开 —— 而是**按 endpoint 的 scheme 决定**：
配了 `https://` 端点时顺手把 http 也放行，等于留了一条**降级通道**。

**10. `object_store` 的 `build()` 会校验 bucket 与凭据 —— 但报错是英文的第三方错误。**
实测 `object_store-0.14.2/src/aws/builder.rs:1160-1186`：
缺 bucket → `Error::MissingBucketName`；
只配了 access key 没配 secret key → `Error::MissingSecretAccessKey`。

本模块**提前**校验并返回**自己的**错误（`InvalidParam` + 中文原因），
好过让调用方看到一个来自第三方 crate 的英文错误。

**11. `object_store` 的客户端构造是纯本地的 —— 不发网络请求。**
`build()` 只组装配置与 HTTP 客户端。所以 S3 客户端的构造**没有超时问题** ——
真正的连通性在第一次上传/下载时才暴露。

**12. 本任务**不新增**任何配置项 —— 严格消费 `nexus-config` 的映射清单。**
初稿里我加了三个配置项：`redis.connect_timeout_secs` / `s3.region` /
`milvus.timeout_secs`。但任务书明确写「本任务**消费**不新增」，
且 `nexus-config` 的映射清单里没有它们。

已全部改回**代码内常量**：

| 值 | 处理 |
|---|---|
| Redis 单次连接超时 | 常量 `ATTEMPT_TIMEOUT_SECS = 1`（与上游缺省一致，但显式设上） |
| Redis 整体连接预算 | 常量 `DEFAULT_CONNECT_TIMEOUT_SECS = 3` |
| Milvus RPC 超时 | 常量 `DEFAULT_TIMEOUT_SECS = 10`（与上游 `RPC_TIMEOUT` 一致） |
| Milvus 整体连接预算 | 常量 `DEFAULT_CONNECT_BUDGET_SECS = 3` |
| S3 region | 常量 `DEFAULT_REGION = "us-east-1"`（与上游缺省一致） |

理由：**Redis / Milvus 都是「可丢弃」的派生存储**，
为它们开「必须配准」的旋钮只会让部署多一个失败点。

**13. 新增了 `tests/degradation.rs` —— 把 AC #4 变成可执行断言。**
AC #4 说「Redis 客户端在连接失败时返回 `Result` —— **不 panic**，且**不阻断** PG 侧工作」。

「不阻断 PG 侧」必须有测试，否则只是断言了一句口号。
6 条测试断言：

- Redis 失败时 PG 侧**不受牵连**（PG 的错误信息里**不含** "redis" 字样）
- 完全不配 Redis 时 PG 侧依然能走到「连接」这一步
- 格式非法的 Redis URL **不 panic**（急切与惰性两条路径都断言）
- Redis 构造对挂起对端**有界**（预算来自代码常量，不写死）
- Milvus 构造**只做 TCP 层**（断言实测行为）
- Milvus 构造在 TCP 被拒时**有界且失败**

