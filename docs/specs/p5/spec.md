# Spec · P5 协议层

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 权威顺序：架构文档 > 本 Spec > `backlog/tasks/*`（Task 层）。
> 本文件是**契约**，不是进度记录 —— 进度看 `backlog/tasks/` 的任务状态。
> 分层判定记录见 [`../p5-tiering.md`](../p5-tiering.md)。

**特性**：`nexus-api` —— REST 路由骨架 + SSE 帧编解码与桥接 + 协议层中间件。
**架构文档阶段**：§6.1 的 P5 行。
**完成判据**：**SSE 半帧处理有测试**（拆成可验证子目标，见 §5）。

---

## 1. 范围

### 1.1 在范围内

| 项 | 说明 |
|---|---|
| **SSE 帧编解码器** | `FrameEncoder`（事件 → 字节帧）+ `FrameDecoder`（字节流 → 帧，**半帧缓冲**） |
| **SSE 桥接** | `nexus_core::EventPayload` 流 → `Body::from_stream`；**手写**，不用 axum 的 `Sse`（决策 A） |
| **事件名映射** | `EventPayload` 的 12 个变体 → SSE `event:` 名（架构文档 §2.2.4.1 **已定稿**） |
| **协议层中间件** | 鉴权上下文的**提取机制**（trait 接缝）；错误响应的**兜底**；请求 ID 透传 |
| **路由骨架** | 可增量注册路由的 `Router` 构建器 + 装配机制 |
| **心跳** | `: ping\n\n` 注释帧（保活） |
| **取消传播** | 客户端断开 → `CancellationToken` 触发（连接断开 = 取消） |
| **REST 响应包** | 成功/失败的**统一形状**（`{code, message, data?}`，参考实现 `internal/pkg/response`） |

### 1.2 不在范围内

| 项 | 理由 |
|---|---|
| **业务编排**（会话保障 / 归属校验 / 上下文组装） | 决策 B：P5 是**传输层**；业务编排属 `nexus-service`，且 `nexus-service` 是 7 行空壳 |
| **具体业务端点**（KB / 工作流 / 记忆 / 模型管理） | 属 P6/P7/P8；P5 只交付**注册机制** |
| **鉴权的具体策略**（JWT 校验 / 配额判定） | `nexus-security` 是 7 行空壳；P5 交付**机制**（trait 接缝），策略留给上层 |
| **A2A 入站端点** | 属 P8（`nexus-a2a`），且架构文档 §4.3.1 已定稿「MCP 纯出站，A2A 双向」 |
| **MCP 端点** | 属 P8（`nexus-mcp`） |
| **WebSocket / 长轮询** | 架构文档 §1.2 的 `nexus serve` 只有 REST + SSE + A2A |
| **`FinishReason::Canceled` 变体** | 事件契约已定稿（§6.3）；P5 只**登记**（OQ-1 / F3），不在实现里顺手决定 |
| **优雅关停** | 属 `nexus-cli`（进程生命周期） |
| **诊断端点独立端口** | 参考架构 §1.1 标注为「可选，独立端口」；P5 只交付 `/metrics` |

---

## 2. 不可让步的红线

### 2.1 契约不可改

`nexus-core` 的 **42 个业务码、12 个事件名、18 个 ID 类型已定稿**（架构文档 §6.3）。

P5 **复用**下列码值，**不得新增**：

| 码值 | 枚举 | HTTP | P5 用途 |
|---|---|---|---|
| `10001` | `InvalidParam` | 400 | 请求体非法（缺字段 / 类型错 / SSE 帧畸形） |
| `20101` | `Unauthorized` | 401 | 缺少身份凭证 |
| `20201` | `Forbidden` | 403 | 身份无权访问该资源 |
| `40001` | `RateLimited` | 429 | 上游/共享额度已满 |
| `90001` | `Internal` | 500 | 协议层内部状态不一致 |
| `90002` | `NotImplemented` | 501 | 端点未实现（骨架占位） |
| `90003` | `ServiceUnavailable` | 503 | 服务不可用 / 运行已取消 |

> ⚠️ **P5 不得新增任何 `ErrorCode` 变体** —— 由
> `scripts/check-specs.sh` 规则 4/5 机器校验。

### 2.2 SSE 字节格式**逐字节**对齐

参考架构 §1.5 **已定稿**的帧格式：

```text
event: <event>\n
data: <json>\n
\n
```

心跳是 `: ping\n\n`（注释行）。

**为什么逐字节**：前端 `lib/sse.ts` 是**按字节解析**的，
任何格式差异都会表现为「前端收不到事件」这类**极难排查**的问题。

| 规则 | 说明 |
|---|---|
| 帧分隔符是 `\n\n` | **不是** `\r\n\r\n`（虽然解码器容忍 CRLF 变体） |
| `data:` 后**恰好一个**空格 | `data: {"a":1}`，不是 `data:{"a":1}` |
| `event:` 后**恰好一个**空格 | 同上 |
| 帧**以** `\n\n` **结尾** | 最后一帧也不能少分隔符 |
| 心跳是**注释行** | `: ping\n\n` —— 前端靠 `:` 开头识别并跳过 |
| **不加** `Cache-Control` 等额外头 | 保持与 Go 版一致 |

> **决策 A**：**手写** `Body::from_stream`，**不用** axum 的 `Sse`。
> axum 的 `Sse` 会用 `Event::default().data()` **自动转义** data ——
> 与上面的字节输出不一致。

### 2.3 事件名**原样**使用

SSE 的 `event:` 字段的值 = `nexus_core::EventPayload` 的 12 个事件名：

`meta` / `delta` / `reasoning` / `plan` / `tool_call` / `tool_result` /
`agent_event` / `refs` / `usage` / `approval` / `done` / `error`

**不得**加前缀、不得改大小写、不得用枚举的 Rust 变体名（`ToolCall` ≠ `tool_call`）。

> ⚠️ 这是**前后端契约**。改一个名字，前端那个分支就**静默失效**了。

### 2.4 依赖方向

**不新增任何 crate 依赖方向。**

`nexus-api` 的既有依赖（`nexus-core` / `nexus-config` / `nexus-model` /
`nexus-observability` / `nexus-security` / `nexus-service` / `nexus-agent` /
`nexus-execution` / `nexus-llm`）**全部保留**。

由 `scripts/check-deps.sh` 规则 7（`deps_of` ↔ `Cargo.toml` 一致性）强制。

> ⚠️ **但**：P5 **不依赖** `nexus-service` 的**具体实现** ——
> 它依赖的是**trait**（消费方定义 trait 模式）。
> 理由见决策 B 的说明。

### 2.5 内部原因不外泄

`AppError.cause` **绝不**进入对外响应（架构文档 §2.6.5）。

**类型级**保证：只序列化 `UserFacing`（`code` + `message`）——
`AppError` **根本不实现 `Serialize`**。

### 2.6 不引入 `unsafe`

workspace 已 `forbid`。

### 2.7 不引入 `Box<dyn>` 于热路径

SSE 桥接是**逐事件**的热点路径。用**泛型 + 静态分发**。

只有**路由注册**这种「异构集合」的场景才允许 `dyn`。

---

## 3. 核心契约

### 3.1 SSE 帧编解码器

```
EventPayload ──encode──> Vec<u8>          （单帧，无状态）
字节流 ──decode──> Vec<Frame>             （增量式，缓冲半帧）
```

#### 3.1.1 编码器

```
"event: " + <event_name> + "\n" + "data: " + <json> + "\n\n"
```

**纯函数**：同样的输入 → 同样的字节。

#### 3.1.2 解码器（**半帧缓冲**）

这是 P5 完成判据的**核心**。

```
push(chunk) → 完整帧（0..n 个）
```

**不完整的尾部被缓冲**，等待后续数据。

| 规则 | 理由 |
|---|---|
| 分隔符**连同帧内容一起切走** | 只切内容会把 `\n\n` 留在缓冲区头部 → `position()` 永远返回 0 → **死循环** |
| 支持 **CRLF 变体** | 代理/网关可能改写换行；解码器要容忍 |
| 缓冲字节数**可观测** | 测试要能断言「半帧被缓冲」而不是「半帧被丢弃」 |
| 空帧 / 无 `event` 无 `data` → **错误** | 畸形帧不能静默通过 |
| 帧内容有**大小上限**（默认 1 MiB） | 上游不送分隔符时缓冲区会**无界增长**；上限把它变成**有界**（NEX-35） |

**上限的语义**（NEX-35 定稿）：

| 项 | 取值 |
|---|---|
| 默认上限 | 1 MiB（`MAX_FRAME_BYTES`） |
| 约束对象 | **单帧内容**（**不含**分隔符）—— **不是**整个 chunk |
| 超限行为 | `push` 返回 `FrameError::FrameTooLarge` —— **不 panic、不静默截断** |
| 超限后 | 解码器**不可恢复**（无法重新同步帧边界）→ 释放缓冲，后续 `push` 直接返回同一错误 |
| 错误内容 | 只带 `limit` / `actual` 两个**长度** —— **绝不**带上游负载 |

> **回归基准**：`examples/demo-sse-frames` 已有**逐字节喂入**与
> **任意切分点**的测试 —— P5 的解码器**必须**通过同形态的测试。

### 3.2 SSE 桥接

```
EventStream（nexus_agent::events::EventStream）
  → 逐事件编码为帧
  → Body::from_stream
```

| 规则 | 说明 |
|---|---|
| **手写** `Body::from_stream` | 决策 A |
| 事件流**结束** → 流结束 | 不补额外的帧 |
| 事件流**出错** → 发 `error` 帧 | 用 `ErrorCode` 的码值，**不含** cause |
| **客户端断开** → 取消令牌触发 | 连接断开 = 取消（参考架构 §2.1 的取消语义） |
| **心跳**由调用方注入 | 桥接**不自带**心跳定时器 —— 那是**路由**的职责 |

> **为什么心跳不内置在桥接里**：心跳是**传输层保活**，
> 频率与路由的读超时配置相关。把它做成桥接的**可配置项**会让桥接承担两条职责。
> 桥接只做「事件 → 帧」这一件事。

### 3.3 中间件：鉴权上下文的**机制**

P5 交付**机制**，不交付**策略**。

```
trait IdentityExtractor: Send + Sync {
    fn extract(&self, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError>;
}
```

**由上层实现**（`nexus-security` / `nexus-service`），`nexus-api` 只负责：

1. 在请求进入时调用提取器；
2. 提取失败 → `401` / `403`（**统一响应包**）；
3. 成功 → 把身份放进请求的 `Extension`，供 handler 取用。

> **为什么用 trait 而不是直接调 `nexus-security`**：
> `nexus-security` 是 7 行空壳，且 JWT 校验的具体形态还没定。
> trait 让 `nexus-api` **不阻塞**于那个决策。

### 3.4 统一响应包

参考实现 `internal/pkg/response` **已定稿**：

```json
{"code": 0, "message": "ok", "data": {...}}
```

| 规则 | 说明 |
|---|---|
| 成功 `code` 是 **0** | 业务码从 `10001` 起 —— 0 不与任何业务码冲突 |
| 失败 `code` 是**业务码** | 与 `ErrorCode` 的码值一致 |
| `message` **不含**内部原因 | 架构文档 §2.6.5 |
| `data` 缺省时**省略** | 不输出 `"data": null` |

> **为什么 `code` 是数字而不是字符串**：架构文档 §2.6.3 的验证记录明确指出
> `ErrorCode` **派生** `Serialize` 会输出 `{"code":"InvalidParam"}`，
> 手工实现后输出 `{"code":10001}` —— 前端按**码段**分支。
> 所以成功响应的 `code` 也必须是**数字**（0）。

### 3.5 路由骨架

```
Router 构建器
  ├── /metrics（已交付，NEX-25）
  └── SSE 端点（示范性）
```

**可增量注册** —— P6/P7/P8 的端点随对应阶段加入。

### 3.6 取消传播

```
客户端断开
  → axum 的 Body 被 drop
  → 桥接的流被 drop
  → CancellationToken 触发
  → 上层（service / agent）感知取消
```

> **这是 P4 的取消语义在协议层的落点** ——
> P4 的 `EventSink::cancel_token()` 与 P5 的断开检测**接在一起**。

---

## 4. 安全契约

### 4.1 凭据不外泄

P5 **不接触**任何凭据 —— 鉴权由**上层**的 `IdentityExtractor` 完成。

但 P5 必须保证：**请求头里的凭据绝不出现在日志或响应里**。

### 4.2 内部原因不外泄

已在 §2.5 与 §3.4 声明。**类型级**保证。

### 4.3 错误信息不泄露内部结构

`message` 面向**用户**，不是面向**开发者**。

---

## 5. 完成判据与「防伪」要求

架构文档 §6.1：**「SSE 半帧处理有测试」**

拆成**七个**可验证子目标：

1. **半帧缓冲有测试** —— 喂入不完整的帧，**不产出**帧，缓冲字节数 > 0。
2. **逐字节喂入有测试** —— 一帧被切成 N 片喂入，最终得到**完整**的一帧。
3. **任意切分点有测试** —— 在帧的**每一个**字节位置切断，两次喂入后结果相同。
4. **分隔符不残留有测试** —— 连续多帧解码后，缓冲区**清空**（无 `\n\n` 残留）。
5. **字节格式有测试** —— 编码结果**逐字节**等于期望字符串。
6. **事件名映射有测试** —— 12 个事件名**全部**映射正确（**不是**抽查几个）。
7. **桥接有测试** —— 事件流 → 响应体，帧序列正确；流出错时发 `error` 帧且**不含** cause。

### 5.1 「有测试」的**防伪**要求

沿用 NEX-14 / NEX-22 / NEX-24 / NEX-25 的教训：

| 假象形态 | 防线 |
|---|---|
| 半帧只测了「短一字节」 | 必须测**逐字节喂入**与**任意切分点** |
| 帧格式只测了「能解析回来」 | 必须断言**逐字节**等于期望字符串（**不是** round-trip） |
| 事件名只测了 2、3 个 | 必须测**全部 12 个** —— 漏一个就是前端**静默失效** |
| 「解析成功」但分隔符残留 | 必须断言**缓冲区清空**（防死循环回归） |
| 断言引用了被测代码的**常量** | 必须断言**字面量**（NEX-25 的教训 —— 同义反复） |
| 桥接只测了「正常结束」 | 必须测「**流出错**」—— 发 `error` 帧 |
| 测试「存在」但没跑 | `scripts/check-specs.sh` 规则 3 机器校验 |
| 门禁/断言**没验证过会红** | **必须做故障注入**（NEX-24 的教训） |

---

## 6. 已定稿的技术决策

| 编号 | 决策 | 选择 | 依据 | 类别 |
|---|---|---|---|---|
| D8 | SSE 输出的实现方式 | **手写** `Body::from_stream` | 见 §6.1 | (a) decide-and-proceed |
| D9 | P5 的交付边界 | **传输层能力**（trait 接缝，不做业务编排） | 见 §6.2 | (a) decide-and-proceed |
| D10 | 鉴权的归属 | **机制在 P5，策略在上层**（trait 接缝） | 见 §6.3 | (a) decide-and-proceed |
| D11 | 成功响应的 `code` | **数字 0** | 见 §6.4 | (a) decide-and-proceed |

### 6.1 D8 —— 手写 `Body::from_stream`

**决策**：SSE 输出**手写** `Body::from_stream`，**不用** axum 的 `Sse` 类型。

**依据**：参考架构 §1.5 **已定稿**：

> axum 的 `Sse` 会用 `Event::default().data()` 自动转义，与 Go 版手写
> `event: / data:` 的字节输出不完全一致。**建议 Rust 版手写 `Body::from_stream`**，
> 保持字节级一致，避免前端兼容问题。

**反方论点与回应**（决策 rubric §4）：

> 「axum 的 `Sse` 是**框架原生**的 —— 它自动处理 keep-alive、
> 自动处理 data 转义、自动处理 `Cache-Control`。
> 手写这些是**重复造轮子**，而且 axum 未来修了 bug 我们不会跟上。」

**回应**：这条论点**部分成立**，但结论**不成立**。

1. axum 的 `Sse` **自动转义 data** —— 那是**行为差异**，不是「额外功能」。
   前端 `lib/sse.ts` 按字节解析，转义后的 `data:` 内容**前端不会**原样还原。
2. axum 的 `Sse` **自动加 `Cache-Control`** —— 与 Go 版不一致。
   这类差异在**联调时**才会暴露，代价是**跨团队排查**。
3. keep-alive **不需要** axum 的实现 —— 我们要的是 `: ping\n\n` **注释帧**，
   而 axum 的 keep-alive 是**另一个机制**（发送 `: ping\n\n` 但格式可能不同）。

**可逆性**：中 —— 换回 axum 的 `Sse` 是局部改动，但会**破坏**前端兼容性。
**影响面**：`crates/nexus-api/src/sse.rs` + `src/bridge.rs`。

### 6.2 D9 —— 传输层边界

**决策**：P5 交付**传输层能力**（SSE 编解码 + 桥接 + 中间件 + 路由骨架），
用 **trait 接缝**连接上层；**不做**业务编排。

**依据**：

1. `nexus-service` 是 **7 行空壳** —— 做完整 chat 端点就等于**同时**填
   `nexus-service`（会话保障、归属校验、上下文组装、RAG 检索、记忆召回），
   那是**另一个阶段**的工作量。
2. 架构文档 §1.4 的能力分层：**协议层能感知 HTTP / SSE 语义与已认证的身份，
   但不能感知业务规则、事务边界** —— 业务编排**本来就不该**在 `nexus-api`。
3. 参考架构 §0.3 的 crate 职责表：`nexus-api` 的职责是
   「HTTP/SSE 协议适配、鉴权与门禁中间件」，**不该做**的事是「查库拼业务」。

**反方论点与回应**：

> 「只交付传输层而没有端点，P5 交付的东西**谁来用**？
> 没有完整端点，trait 接缝就是**空壳** ——
> 而『空壳比没有更糟』，因为它看起来完成了。」

**回应**：这条论点**成立**，所以 P5 **必须交付一个真实的端点** ——
不是只有骨架。

具体地，P5 交付：
- `/metrics`（NEX-25 已交付，**真实可用**）；
- **一个**示范性的 SSE 端点 —— 它的数据源是一个**注入的**事件流提供器
  （trait），所以它**真实可跑**（用本地 mock 就能看到完整 SSE 帧流）。

骨架部分（可增量注册的 `Router` 构建器）是**基础设施**，不是交付物本身。

**可逆性**：高 —— trait 接缝是**接口**，后续填 `nexus-service` 实现**不改** `nexus-api`。
**影响面**：`crates/nexus-api/src/*`。

### 6.3 D10 —— 鉴权：机制在 P5，策略在上层

**决策**：`nexus-api` 定义 `IdentityExtractor` trait；
**上层**（`nexus-security` / `nexus-service`）提供实现。
P5 **不实现**任何具体的 JWT 校验。

**依据**：

1. `nexus-security` 是 **7 行空壳** —— JWT 校验的具体形态**还没定**。
2. 参考架构 §0.4 的 Go 版反例 → Rust 落点明确写着
   「**权限由 service 注入**的 `SetAccessCheck` 回调完成」——
   Rust 用 trait 对象表达「可替换的策略」。
3. 这与 P4 的 D4（`nexus-agent` 自定义 trait、复用 `nexus-llm` 数据类型）
   是**同一条纪律**。

**反方论点与回应**：

> 「多一层 trait 意味着多一层间接。而且 P5 明明可以直接调 `nexus-security` ——
> 反正它早晚会实现 JWT 校验。」

**回应**：这条论点的**前半段**成立（确实多一层），
但**结论**不成立。

直接调 `nexus-security` 会让 P5 **阻塞于**一个**还没做的** crate ——
而 P5 与 P8（`nexus-security` 落地）**没有顺序依赖**。
trait 让两者**并行**。

**可逆性**：高 —— 等 `nexus-security` 有具体实现后，加一个 `impl IdentityExtractor`
**不改动** `nexus-api` 的任何已有代码。
**影响面**：`crates/nexus-api/src/middleware.rs`。

### 6.4 D11 —— 成功响应的 `code` 是数字 0

**决策**：统一响应包的成功 `code` 是**数字 `0`**。

**依据**：架构文档 §2.6.3 的**验证记录**（2026-11-04）：

> 派生 `Serialize` 会输出 `{"code":"InvalidParam"}`；手工实现后输出 `{"code":10001}`。
> 这个差异看似细节，实际决定了前端能否按码段分支。

业务码从 `10001` 起 —— `0` **不与任何业务码冲突**，
且让前端可以用**同一个**分支处理「成功」与「失败」。

**反方论点与回应**：

> 「`0` 作为成功码是 Go 生态的习惯，但本项目的业务码是 `10001` 起 ——
> 成功用 `0` 会让码段体系出现一个『不在码段里的值』，
> 破坏『码段决定分类』的一致性。」

**回应**：这条论点**有道理**。但两条理由压过它：

1. 参考实现**已经这么用**（`internal/pkg/response`）——
   前端**已经**按 `code == 0` 判成功。改掉是**协议破坏性变更**。
2. 码段分类（`1xxxx` 参数 / `2xxxx` 鉴权 / …）描述的是**错误**的分类，
   成功**不属于**任何错误码段。用 `0` 是「不属于任何错误码段」的**自然**表达。

**可逆性**：中 —— 改 `0` 会**破坏前端**。
**影响面**：`crates/nexus-api/src/response.rs`。

### 6.5 后续任务（带触发条件）

| # | 后续工作 | 触发条件 |
|---|---|---|
| F3 | `FinishReason` 增加 `Canceled` 变体 | 当**前端**需要区分「用户取消」与「正常结束」时。P4 的 F2 在 P5 触发条件已到，但 P5 当前交付边界**不需要**区分 —— 取消由 `RequestStatus::Canceled` 指标承载。**承接 P4 的 F2** |

> **F3 不在本阶段解决**：改 `FinishReason` = **协议破坏性变更**，
> 需要前后端同步。按「不扩大范围」+「不静默偏离」，
> P5 只**登记**，**不在实现里顺手决定**（OQ-1）。

---

## 7. 验证方式

```bash
# 门禁
bash scripts/check-lock.sh      # 依赖锁定纪律
bash scripts/check-deps.sh      # crate 依赖方向铁律
bash scripts/check-naming.sh    # 标识符不得含中文
bash scripts/check-backlog.sh   # backlog 组织纪律
bash scripts/check-specs.sh     # Spec 层组织纪律
bash scripts/check-rules.sh     # 规则文件一致性
bash scripts/check-docs.sh      # 文档层一致性
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是**独立 workspace**，验证命令为 `cd examples && cargo test --workspace`。
