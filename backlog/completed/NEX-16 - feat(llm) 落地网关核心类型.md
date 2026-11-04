---
id: NEX-16
title: "feat(llm): 落地网关核心类型"
status: Done
assignee: []
labels: [llm, types, p3]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.3.2"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.6.2"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.4"
  - "docs/operations/p3-plan.md"
---

## Description

P3 的第一个任务：落地 `nexus-llm` 的**核心类型** —— 消息、请求、响应、流事件、`ModelDescriptor`。

后续所有 P3 任务（预算裁剪、重试、熔断、网关主体、HTTP 客户端）都建立在这套类型上。

### 要落地的类型

| 类型 | 来源 | 说明 |
|---|---|---|
| `Message` | `internal/llm/types.go:14` | 对话消息（system / user / assistant / tool） |
| `Role` | `internal/llm/types.go:17` | 四个角色 |
| `ToolCall` | `internal/llm/types.go:26` | 模型发起的一次工具调用 |
| `ToolDef` | `internal/llm/types.go:33` | 工具定义（JSON Schema） |
| `ChatRequest` | `internal/llm/types.go:44` | 单次对话请求 |
| `Usage` | `internal/llm/types.go:53` | Token 用量 |
| `ChatResponse` | `internal/llm/types.go:60` | 非流式响应 |
| `StreamEvent` | `internal/llm/types.go:70` | 流式事件 |
| `ModelDescriptor` | `internal/llm/types.go:81` | 模型描述（网关按它实例化请求） |

### 类型放在哪个 crate

**`nexus-llm`** —— 这些是**网关的协议类型**，不是跨 crate 共享的领域 DTO。

- `nexus-model` 是「跨 crate 共享的纯 DTO」（§4.2 差异 2），不带任何序列化痕迹；
- `ChatRequest` / `ChatResponse` 只在 `nexus-llm` 与其调用方之间流转，
  不需要 `nexus-model` 承载。

**调用方（`nexus-agent` / `nexus-service`）将来直接依赖 `nexus-llm`** ——
依赖方向是 `service → llm`，与 §4.4 规则 1（能力层不得反向依赖上层）一致。

### `ModelDescriptor` 的 secret 处理

`ModelDescriptor` 含 `api_key`（解密后的明文）。

**必须**用 `secrecy::SecretString` 承载 —— 与 `nexus-infra` 的 PG URL 同一纪律：
`Debug` 输出自动脱敏，需要真正使用时显式 `expose_secret()`。

`nexus-security` 已经在依赖里 —— `nexus-llm` 依赖它是**合法的**（能力层内部依赖）。

## Acceptance Criteria

- [ ] #1 `crates/nexus-llm/src/types.rs` 落地 `Message` / `Role` / `ToolCall` / `ToolDef` / `ChatRequest` / `Usage` / `ChatResponse` / `StreamEvent` / `ModelDescriptor`
- [ ] #2 所有公共类型实现 `Debug`（workspace lint `missing_debug_implementations` 通过）
- [ ] #3 `ModelDescriptor` 的 `api_key` 用 `secrecy::SecretString` 承载；`Debug` 输出**不含**明文口令
- [ ] #4 `StreamEvent` 的错误字段**不参与序列化**（`json = "-"`）—— 内部错误绝不外泄给客户端
- [ ] #5 `Role` 是 `enum`（不是 `String`）—— 角色取值在类型上封闭
- [ ] #6 `ToolDef` 的**权威执行元数据**（`dangerous` / `replay_policy` / `source_identity` / `binding_version`）**不序列化** —— 那是控制面的东西，不是给模型看的
- [ ] #7 `nexus-llm` 的 `Cargo.toml` **不依赖** `nexus-repo`（§4.2 差异 1 + 参考架构 §2.4）
- [ ] #8 `cargo test -p nexus-llm` 通过；`cargo clippy -p nexus-llm --all-targets --locked -- -D warnings` 零告警；`cargo fmt --all --check` 通过
- [ ] #9 有**契约测试**：类型字段与参考实现的 `internal/llm/types.go` 逐字段对齐（字段名与 JSON tag）
- [ ] #10 有**负向测试**：试图给 `StreamEvent` 序列化错误、或给 `ToolDef` 序列化 `dangerous` 时**编译不过**

## 备注

- **不实现任何网络调用** —— HTTP 客户端是 NEX-19。本任务只落类型。
- **不实现预算裁剪** —— 那是 NEX-17。本任务不碰 `ContextWindow` 的语义。
- **`ToolDef` 的 `dangerous` 与 `replay_policy` 不序列化** 是**安全要求**，不是偷懒：
  参考实现的 `internal/llm/types.go:36-37` 注释写明「authoritative execution metadata, never sent to the model」。
  把「这个工具危险」告诉模型，模型可能据此改变行为 —— 那是把控制面泄漏给不可信的推理方。
- **`StreamEvent.err` 不序列化** 同理 —— 参考实现注释「never serialized to clients」。
  内部错误原因绝不出现在对外响应（§2.6.5）。
- **本任务不改任何契约** —— 这些类型还没有对外契约（它们在 `nexus-llm` 内部流转）。
  未来 `nexus-api` 暴露时会有**另一层** DTO，那时再定对外契约。

## 实现过程中的发现

### 1. `StreamEvent` **不能**实现 `Deserialize`

原计划用 `#[serde(skip)]` 让 `err` 字段「不参与序列化」就够了。

但 `#[serde(skip)]` 的副作用是：反序列化时该字段取默认值（`None`）。
那意味着「从 JSON 里塞一个错误进来」在**运行期静默失效** ——

而**静默失效比编译失败更危险**（你以为它失败了，其实没有）。

所以 `StreamEvent` 只 derive `Serialize`，**不** derive `Deserialize`。
「从 wire 上还原一个带错误的流事件」在**编译期就不可能**。

> 通用教训：`#[serde(skip)]` 用来**排除敏感字段**是对的，
> 但如果那个字段的存在与否**有语义**（这里是「这个事件是不是错误事件」），
> 就该用「不实现 `Deserialize`」来表达，而不是靠 `skip` 的副作用。

### 2. `temperature` 是 `f32` —— 契约测试不能用 `==` 精确比较

`0.7f32` 序列化成 JSON 是 `0.699999988079071`，与 `json!(0.7)`（f64）**不相等**。

参考实现的 `internal/llm/types.go:48` 也是 `float32`，
所以这不是我们的选择，是**协议的约束**。

契约测试改成**近似比较**（误差 < 1e-6）。
这个坑值得记下来 —— 否则下一个人会以为「序列化有 bug」。

### 3. `api_key` 私有字段的错误码是 `E0616` 而不是 `E0609`

「字段不存在」是 `E0609`，「字段私有」是 `E0616`。
负向测试的注释里写错了错误码 —— 已被 `.stderr` 快照纠正。

> 这正是**快照入库**的价值：注释写错，测试会立刻报出来。

### 4. `ToolDef` 保留 `Deserialize`，`StreamEvent` 不保留

两者看似不对称，实则理由不同：

- `ToolDef` 的控制面字段（`dangerous` 等）从 wire 上恢复时取默认值 —— 这是**正确**的：
  wire 上本来就没有这些信息，「不知道」是准确的表达。
- `StreamEvent` 的 `err` 从 wire 上恢复时取 `None` —— 这是**错误**的：
  一个「错误事件」恢复成「正常事件」，语义被**静默篡改**。

区别在于：缺失的信息是「本来就无关」还是「本身承载语义」。

## 交付物

| 项 | 数量 | 位置 |
|---|---|---|
| 网关协议类型 | 9 | `crates/nexus-llm/src/types.rs` |
| 契约测试 | 16 | `crates/nexus-llm/tests/types_contract.rs` |
| 负向测试 | 3 | `crates/nexus-llm/tests/compile_fail/` |

## 后续

- **NEX-17** 上下文预算裁剪 —— 用 `ModelDescriptor.context_window` 与 `max_output_tokens`
- **NEX-18** 重试 —— 用 `nexus-core` 的 `RetryPolicy` 判定，**不**自己另立一套
- **NEX-19** OpenAI 兼容客户端 —— 用 `ChatRequest` / `ChatResponse` / `StreamEvent`
