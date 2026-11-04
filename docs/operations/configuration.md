# 配置参考

架构文档 §2.7 的落地说明。

## 三层来源与优先级

```
base（打包进二进制的默认值）  <  file（configs/*.yaml）  <  env（NX_* 环境变量）
```

优先级从低到高。高优先级层**只覆盖叶子**，不覆盖整棵子树：

```yaml
# file 层
llm:
  deepseek:
    base_url: http://from-file
    model: from-file
```

```bash
export NX_LLM__DEEPSEEK__BASE_URL=http://from-env
```

结果：`llm.deepseek.base_url` = `http://from-env`（env 覆盖），`llm.deepseek.model` = `from-file`（env 未涉及，保留 file）。

### base 层

打包在二进制里（`crates/nexus-config/resources/default.yaml`），**不需要任何外部文件**即可工作。

只放合理默认值 —— 不放任何与部署环境相关的具体值（连接串、密钥、主机名一律走 file 层或 env 层）。

### file 层

来自 `configs/` 目录下的 `*.yaml` / `*.yml`，按**文件名排序**依次深合并：

| 文件 | 入库 | 作用 |
|---|---|---|
| `configs/config.yaml` | ✅ | 各部署环境共有的结构性配置 |
| `configs/local.yaml` | ❌ | 本地覆盖（`.gitignore` 已排除） |
| `configs/secrets.yaml` | ❌ | 密钥（`.gitignore` 已排除） |

排序保证 `config.yaml` → `local.yaml` → `secrets.yaml`，即后者覆盖前者。

> 本地开发：`cp configs/local.yaml.example configs/local.yaml`

### env 层

来自 `NX_` 前缀的环境变量，**由声明式映射表驱动**（见下）。

## 声明式环境变量映射表

映射表位置：`crates/nexus-config/src/mapping.rs` 的 `table()`。

**为什么要映射表**：参考实现（viper）的「自动匹配」让「哪些环境变量能覆盖什么」成为运行时的副作用 —— 不可枚举、不可文档化、不可测试。映射表把它变成一份**静态清单**。

### 命名规则

```
NX_<段>__<段>__<键>    →    配置路径  段.段.键（全小写）
```

- 双下划线 `__` 是**层级分隔符**
- 单下划线是键名的一部分（如 `NX_DATABASE__CONNECT_TIMEOUT_SECS`）
- 段内允许单下划线，但不允许段首/段尾下划线或连续三个下划线（会造成切分歧义）

### 完整映射表

| 环境变量 | 配置路径 | 类型 | 敏感 |
|---|---|---|---|
| `NX_LOG__LEVEL` | `log.level` | string | |
| `NX_LOG__FORMAT` | `log.format` | string | |
| `NX_SERVER__HOST` | `server.host` | string | |
| `NX_SERVER__PORT` | `server.port` | int | |
| `NX_SERVER__HEARTBEAT_SECS` | `server.heartbeat_secs` | int | |
| `NX_SERVER__MAX_BODY_BYTES` | `server.max_body_bytes` | int | |
| `NX_DATABASE__URL` | `database.url` | string | ✅ |
| `NX_DATABASE__MAX_CONNECTIONS` | `database.max_connections` | int | |
| `NX_DATABASE__CONNECT_TIMEOUT_SECS` | `database.connect_timeout_secs` | int | |
| `NX_REDIS__URL` | `redis.url` | string | ✅ |
| `NX_MILVUS__URL` | `milvus.url` | string | |
| `NX_S3__BUCKET` | `s3.bucket` | string | |
| `NX_S3__ENDPOINT` | `s3.endpoint` | string | |
| `NX_S3__ACCESS_KEY` | `s3.access_key` | string | ✅ |
| `NX_S3__SECRET_KEY` | `s3.secret_key` | string | ✅ |
| `NX_WORKER__LEASE_TTL_SECS` | `worker.lease_ttl_secs` | int | |
| `NX_WORKER__POLL_INTERVAL_MS` | `worker.poll_interval_ms` | int | |
| `NX_LLM__DEFAULT_PROVIDER` | `llm.default_provider` | string | |
| `NX_LLM__REQUEST_TIMEOUT_SECS` | `llm.request_timeout_secs` | int | |
| `NX_LLM__CONTEXT_BUDGET` | `llm.context_budget` | int | |
| `NX_LLM__DEEPSEEK__BASE_URL` | `llm.deepseek.base_url` | string | |
| `NX_LLM__DEEPSEEK__API_KEY` | `llm.deepseek.api_key` | string | ✅ |
| `NX_LLM__DEEPSEEK__MODEL` | `llm.deepseek.model` | string | |
| `NX_SANDBOX__TIMEOUT_SECS` | `sandbox.timeout_secs` | int | |
| `NX_SANDBOX__MAX_OUTPUT_BYTES` | `sandbox.max_output_bytes` | int | |
| `NX_OBSERVABILITY__OTLP_ENABLED` | `observability.otlp_enabled` | bool | |
| `NX_OBSERVABILITY__OTLP_ENDPOINT` | `observability.otlp_endpoint` | string | |

### 值类型

| 类型 | 接受的值 |
|---|---|
| `string` | 原样字符串 |
| `int` | `i64` 十进制整数 |
| `float` | `f64` 浮点数 |
| `bool` | `true` / `false`（大小写不敏感）/ `1` / `0` |
| `list` | 逗号分隔，元素去空白 |

**类型不匹配 → 报错**，不静默跳过，也不猜默认值。

### 未声明的环境变量

`NX_` 前缀但**不在映射表里**的变量：**被忽略**（不生效、不报错），但会出现在 `EnvApplyReport.unmapped` 里 —— 这是一个可观测信号，说明运维以为配了但其实没生效。

## 排查配置问题

`ConfigStack::origin(path)` 回答「这个值从哪来」：

```rust
match stack.origin("log.level") {
    Some(ConfigLayer::Env)  => "来自环境变量",
    Some(ConfigLayer::File) => "来自配置文件",
    Some(ConfigLayer::Base) => "来自打包默认值",
    None                    => "未配置",
}
```

## 相关

- 架构文档：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` §2.7
- 代码规范：`.cursor/rules/rust-code-standards.mdc` §8
- 流程规范：`.cursor/rules/ai-sdlc-workflow.mdc`
