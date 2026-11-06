# P5 运行说明 · 协议层（REST + SSE）

架构文档 §6.1 / §5.4 的落地说明。**这是运行说明**，不是计划 ——
计划见 [`../specs/p5/spec.md`](../specs/p5/spec.md)，
契约见 [`../specs/p5/contracts/invariants.json`](../specs/p5/contracts/invariants.json)。

## 协议层做什么

`nexus-api` 是**协议层**：它把「一次业务上的事件流」变成
「一次带鉴权、请求 ID、心跳与统一响应包的 HTTP / SSE 响应」。

它**不**做业务编排（那是 `nexus-service` 的事），
**不**判断权限策略（那是 `nexus-security` 的事），
**不**查库。

| 能力 | 模块 | 决策 |
|---|---|---|
| SSE 帧编解码（逐字节对齐 + 半帧缓冲 + 帧大小上限） | `sse` | D8 |
| 事件名映射 + SSE 桥接（事件流 → 响应体） | `bridge` | D9 |
| 鉴权**机制**（trait 接缝）+ 请求 ID 透传 | `middleware` | D10 |
| 可增量注册的路由构建器 + 路由层心跳 | `router` | — |
| 统一响应包 + 协议层错误兜底 | `response` / `error` | D11 |
| `GET /metrics` | `metrics` | — |

## 统一响应包

**所有** REST 端点的响应体都是同一个形状：

```json
{"code": 0, "message": "ok", "data": {...}}
```

| 情况 | `code` | `message` | `data` |
|---|---|---|---|
| 成功 | **0** | `"ok"` | 有则带上，无则**省略** |
| 失败 | **业务码**（如 `90001`） | 面向用户的文案 | **省略** |

失败响应的 **HTTP 状态码**与业务码**一致**
（`Unauthorized` → 401，`Forbidden` → 403，`RateLimited` → 429，…）。

> ⚠️ `code` 是**数字**，不是字符串 —— 前端按**码段**分支
> （`1xxxx` 参数 / `2xxxx` 鉴权 / `9xxxx` 内部）。
> `data` 缺省时**整个字段被省略**，不输出 `"data": null` ——
> 否则前端的类型会从 `data?: T` 变成 `data?: T | null`。

## SSE 端点

### 端点在哪

示范性 SSE 端点的默认路径是 `nexus_api::router::SSE_DEMO_PATH`
（当前为 `/api/v1/chat/stream`）。它是**注入式**的：

```rust
use nexus_api::{EventStreamProvider, RouterBuilder};
use std::sync::Arc;

let app = RouterBuilder::new()
    // 可选：注入鉴权提取器。不注入则**不挂**鉴权中间件。
    .with_identity_extractor(Arc::new(MyExtractor))
    // 可选：心跳间隔。None = 不发心跳。
    .with_heartbeat(Some(std::time::Duration::from_secs(15)))
    // 注入数据源 —— 没有它，SSE 端点**不会注册**。
    .with_sse_provider(Arc::new(MyProvider))
    // 注册**公开**的 SSE 端点（不挂鉴权中间件）。
    .register_sse_demo(nexus_api::router::SSE_DEMO_PATH)
    .build();
```

> ⚠️ **不注入 `EventStreamProvider` 就没有 SSE 端点** ——
> `register_sse_demo` 会**静默跳过**注册，返回 404。
> 这是刻意的：路由骨架不该注册一个**永远发不出数据**的端点。

需要鉴权的端点用**另一个**方法：

```rust
// 注册**受保护**的 SSE 端点（挂上鉴权中间件）。
// 数据源拿到的 identity 是**已认证**的身份。
let app = RouterBuilder::new()
    .with_identity_extractor(Arc::new(MyExtractor))
    .with_sse_provider(Arc::new(MyProvider))
    .register_sse_demo_protected(nexus_api::router::SSE_DEMO_PATH)
    .build();
```

> ⚠️ 「哪些端点需要鉴权」是**上层**的决定 ——
> 所以这里提供**两个**方法，而不是一个 `protected: bool` 参数：
> 让调用方在**调用点**就能看出意图。

### 怎么访问

```bash
curl -N http://127.0.0.1:8080/api/v1/chat/stream
```

`-N`（`--no-buffer`）**必须加** —— 否则 `curl` 会缓冲输出，
你看不到流式的效果。

### 输出长什么样

```
event: delta
data: {"type":"delta","content":"第一段"}

event: done
data: {"type":"done","finish_reason":"stop"}

```

**逐字节**格式是 `event: {名字}\ndata: {JSON}\n\n` ——
**空行**是帧边界。心跳是 `: ping\n\n`（**注释行**，不产生事件）。

### 怎么解析

**前端必须按字节解析**（与后端的 `encode_frame` 逐字节对齐）：

```ts
// 前端伪代码
const buf = "";  // 累积的半帧
for await (const chunk of response.body) {
  buf += decoder.decode(chunk, { stream: true });
  let idx;
  // ⚠️ 必须找**连续两个**换行 —— 单个 \n 是字段分隔，不是帧边界
  while ((idx = buf.indexOf("\n\n")) !== -1) {
    const frame = buf.slice(0, idx);
    buf = buf.slice(idx + 2);   // ⚠️ 分隔符要**一起**切走
    // frame 现在是 "event: X\ndata: Y"
    ...
  }
}
```

**三条**必须遵守的规则（错了就是「事件莫名其妙少了几条」）：

1. **帧边界是 `\n\n`** —— 单个 `\n` 是字段分隔。
2. **分隔符必须和帧内容一起切走** —— 只切内容会让 `\n\n` 留在缓冲区头部，
   下一轮 `indexOf` 又返回 0 → **死循环**。
3. **半帧必须缓冲** —— TCP 分包可能把一帧劈成两半，
   收到半帧就解析会得到**截断的 JSON**。

> 后端有**同名**的 `FrameDecoder`（`nexus_api::sse::FrameDecoder`）
> 实现了同样的规则，可作参考。

### 心跳

心跳由**路由层**注入（不是桥接）—— 频率与路由的读超时配置相关。

| 配置 | 行为 |
|---|---|
| `with_heartbeat(Some(15s))` | 空闲 15 秒写 `: ping\n\n` |
| `with_heartbeat(None)` | **不发**心跳 |

前端**必须跳过**以 `:` 开头的注释行 —— 它们是保活，不是事件。

## 请求 ID

每个响应都带 `x-request-id` 响应头：

- 请求**带**合法的 `x-request-id` → **原样透传**；
- 否则**生成**一个 UUID v4。

「合法」的判据是**长度 ≤128 且只含 ASCII 字母 / 数字 / `-` / `_` / `.`**。

> ⚠️ 为什么透传的值要**校验** —— 上游传来的头是**不可信输入**，
> 而它会被**回显**到响应头里。无校验直接回显等于让攻击者控制的内容
> **注入**响应头；含 CR / LF 的值还会造成**响应拆分**（response splitting）。

用途：把前端看到的 ID 拿去查日志。服务端日志里的同一个 ID
就能串起整条请求链路。

## 鉴权

P5 交付**机制**，不交付**策略**（决策 D10）。

```rust
pub trait IdentityExtractor: Send + Sync {
    fn extract(&self, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError>;
}
```

由上层（`nexus-security` / `nexus-service`）实现，`nexus-api` 只负责：

1. 在请求进入时调用提取器；
2. 提取失败 → `401` / `403`（**统一响应包**）；
3. 成功 → 把身份放进请求的 `Extension`，供 handler 取用。

> ⚠️ **失败响应的 body 里没有内部原因** ——
> 只序列化 `UserFacing`（`code` + `message`），
> `cause` 在**类型上**就没有外泄的路径。

## 帧大小上限

SSE 解码器对**单帧内容**有上限（默认 1 MiB，`MAX_FRAME_BYTES`）。

超限时 `push` 返回 `FrameError::FrameTooLarge` —— **不 panic、不静默截断**。
解码器随即进入**不可恢复**状态：释放缓冲，后续 `push` 直接返回同一错误。

为什么：**上游永远不送分隔符**时（流损坏、代理截断、恶意输入），
缓冲区会**无界增长**，直到进程内存耗尽。

错误消息只带**长度**（`limit` / `actual`），**绝不**带上游负载 ——
上游字节是不可信输入。

## 指标

`GET /metrics` 返回 Prometheus 文本格式。

```bash
curl http://127.0.0.1:8080/metrics
```

指标**产生**在 `nexus-observability`，本 crate 只**暴露**。
见 [`p3-runbook.md`](p3-runbook.md) 的网关指标说明。

## 排障

| 症状 | 排查方向 |
|---|---|
| SSE 端点返回 404 | 没有注入 `EventStreamProvider` —— 端点**未注册** |
| 收不到任何事件 | 检查数据源是否真的产出事件；检查心跳是否被误当事件 |
| 事件「莫名其妙少了几条」 | 前端解析问题 —— 逐条核对上面的**三条**规则 |
| 收到 `data: null` | 前端在读 `data` 字段而该帧**没有** `data`（只有 `event`） |
| 连接被服务端断开 | 帧超过上限（`FrameTooLarge`）或上游流出错（`error` 帧） |
| 鉴权总是 401 | 上层的 `IdentityExtractor` 没实现，或请求头名不对 |

## 相关文件

| 文件 | 作用 |
|---|---|
| [`../specs/p5/spec.md`](../specs/p5/spec.md) | P5 的设计条款（§3 是协议细节，§5 是完成判据） |
| [`../specs/p5/contracts/invariants.json`](../specs/p5/contracts/invariants.json) | 37 条不变量，逐条指向真实测试名 |
| [`../specs/p5/tasks.md`](../specs/p5/tasks.md) | P5 的任务拆解（T-001..T-006） |
| [`../../crates/nexus-api/src/sse.rs`](../../crates/nexus-api/src/sse.rs) | 帧编解码 + 半帧缓冲 + 帧大小上限 |
| [`../../crates/nexus-api/src/bridge.rs`](../../crates/nexus-api/src/bridge.rs) | 事件名映射 + 桥接 |
| [`../../crates/nexus-api/src/router.rs`](../../crates/nexus-api/src/router.rs) | 路由构建器 + 心跳 |
| [`../../crates/nexus-api/src/middleware.rs`](../../crates/nexus-api/src/middleware.rs) | 鉴权接缝 + 请求 ID |
