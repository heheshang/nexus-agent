# P5 协议层 — Tasks

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 契约见 `spec.md`；机器可读的不变量见 `contracts/`。
>
> 布局遵循 spec-kit `v0.8-headings`（每个任务一个 `### T-NNN — <title>` 标题）。
> `taskId` 形如 `T-NNN`，与 `backlog/tasks/` 下的 `NEX-nn` 一一对应。

## Tasks

### T-001 — 落地 SSE 帧编解码器

**P5 的地基**：所有后续任务都建立在这套编解码上。

`FrameEncoder`（事件 → 字节帧，逐字节对齐参考实现）+ `FrameDecoder`
（字节流 → 帧，**半帧缓冲**）。

对应 backlog 任务：`backlog/tasks/NEX-31 - feat(api) 落地 SSE 帧编解码器与半帧缓冲.md`
依赖：无
对应设计条款：架构文档 §2.2.4.1；参考架构 §1.5、附录 C.1

- AC: `crates/nexus-api/src/sse.rs` 实现 `encode_frame(event: &str, data: &Value) -> Vec<u8>`，输出**逐字节**等于 `"event: {name}\ndata: {json}\n\n"`
- AC: 编码器是**纯函数** —— 同样输入得到同样字节
- AC: `crates/nexus-api/src/sse.rs` 实现**增量式**解码器 —— `push(chunk) -> Vec<Frame>`
- AC: 解码器**缓冲**不完整的尾部帧，缓冲字节数**可观测**
- AC: 分隔符**连同帧内容一起切走** —— 连续多帧解码后缓冲区**清空**（防死循环回归）
- AC: 解码器容忍 **CRLF 变体**（`\r\n\r\n`）
- AC: 空帧 / 既无 `event` 又无 `data` 的帧 → **返回错误**，不静默通过
- AC: 逐字节喂入测试：把一帧切成 N 片喂入，最终得到**完整**的一帧
- AC: 任意切分点测试：在帧的**每一个**字节位置切断，两次喂入后结果相同
- AC: 多行 `data:` 按 SSE 规范用 `\n` 连接
- AC: 注释行（心跳）被忽略
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-002 — 落地事件名映射与 SSE 桥接

**决策 D8 的落地**：手写 `Body::from_stream`，**不用** axum 的 `Sse`。

12 个事件名**原样**映射；事件流 → 帧序列 → 响应体。

对应 backlog 任务：`backlog/tasks/NEX-32 - feat(api) 落地事件名映射与 SSE 桥接.md`
依赖：T-001
对应设计条款：架构文档 §2.2.4.1；参考架构 §1.5、§2.1

- AC: `crates/nexus-api/src/bridge.rs` 实现 `event_name(&EventPayload) -> &'static str`，**全部 12 个**事件名正确（`meta` / `delta` / `reasoning` / `plan` / `tool_call` / `tool_result` / `agent_event` / `refs` / `usage` / `approval` / `done` / `error`）
- AC: 事件名测试**覆盖全部 12 个** —— 不是抽查
- AC: `crates/nexus-api/src/bridge.rs` 实现事件流 → `Body::from_stream` 的桥接
- AC: 桥接**手写**字节输出 —— 源码里**不含** `axum::response::sse`
- AC: 事件流结束 → 流结束，**不补**额外的帧
- AC: 事件流出错 → 发 `error` 帧，含**错误码**，**不含**内部原因（`cause`）
- AC: **客户端断开** → `CancellationToken` 触发
- AC: 桥接的**每帧输出**都能被 T-001 的解码器**完整解回**（round-trip 形状的断言）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-003 — 落地统一响应包与错误兜底

决策 D11 的落地：成功 `code` 是数字 `0`；失败 `code` 是业务码；
`message` 不含内部原因。

对应 backlog 任务：`backlog/tasks/NEX-33 - feat(api) 落地统一响应包与错误兜底.md`
依赖：无
对应设计条款：架构文档 §2.6.5；参考架构 `internal/pkg/response`

- AC: `crates/nexus-api/src/response.rs` 定义统一响应包 —— 成功 `{code: 0, message: "ok"}`，失败 `{code: <业务码>, message: "..."}`
- AC: `data` 缺省时**省略**该字段 —— 不输出 `"data": null`
- AC: `code` 是**数字** —— 不是字符串变体名（架构文档 §2.6.3 的验证记录）
- AC: 响应体**不含**内部原因（`cause`）
- AC: 失败响应的 HTTP 状态码与业务码**一致**（`ErrorCode::http_status()`）
- AC: `crates/nexus-api/src/error.rs` 的 `ApiError` 的响应体**只有** `code` 与 `message` 两个字段
- AC: 兜底路径：序列化失败时仍有**合法 JSON** body（不是空 body）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-004 — 落地协议层中间件与路由骨架

决策 D10 的落地：鉴权的**机制**在 P5，**策略**在上层（trait 接缝）。

路由可**增量注册**；交付**一个**真实可跑的 SSE 端点（数据源是注入的 trait）。

对应 backlog 任务：`backlog/completed/NEX-34 - feat(api) 落地协议层中间件与路由骨架.md`
依赖：T-002、T-003
对应设计条款：架构文档 §1.4；参考架构 §0.3、§0.4

- AC: `crates/nexus-api/src/middleware.rs` 定义 `IdentityExtractor` trait —— `nexus-api` **不实现**具体的 JWT 校验
- AC: 提取失败 → `401` / `403` + 统一响应包
- AC: 提取成功 → 身份放进请求 `Extension`，供 handler 取用
- AC: `crates/nexus-api/src/router.rs` 提供**可增量注册**的路由构建器
- AC: 交付**一个**真实可跑的 SSE 端点 —— 数据源是**注入的** trait（不依赖 `nexus-service` 的具体实现）
- AC: `nexus-api` **不引用** `nexus-service` 的**具体类型**（同 P4 的 D4 纪律）
- AC: `crates/nexus-api/tests/architecture.rs` 断言 `nexus-api` 不依赖 `nexus-repo` / `nexus-infra`
- AC: `crates/nexus-api/Cargo.toml` **不新增**任何内部 crate 依赖
- AC: 路由层注入**心跳** —— 空闲连接按配置间隔写出 `: ping\n\n` 注释帧；桥接**不自带**定时器（spec §3.2）
- AC: **请求 ID 透传** —— 中间件为每个请求分配或透传请求 ID，放进请求 `Extension` 与响应头（spec §1.1）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-005 — SSE 解码器帧大小上限（加固）

P5 完成判据之外的**加固**任务：`FrameDecoder` 的缓冲区当前**没有上限** ——
上游持续发送不含分隔符的字节会让内存无界增长。

收口任务 T-006 在本任务之后执行 —— 收口必须覆盖全部 P5 代码改动。

对应 backlog 任务：`backlog/tasks/NEX-35 - sse-decoder-frame-size-limit.md`
依赖：无（T-001 已交付；本任务修改其实现）
对应设计条款：`docs/specs/p5/spec.md` §3.1.2（半帧缓冲）

- AC: 定义帧大小上限常量（默认 1 MiB），且可被测试引用
- AC: 超限时 `push` 返回 `FrameError::FrameTooLarge`，**不 panic**
- AC: 返回错误后缓冲区**不再增长**
- AC: 恰好等于上限的帧仍能正常解码（边界用例）
- AC: 上限对 LF 与 CRLF 两种分隔符都生效
- AC: `FrameError::FrameTooLarge` 的 `Display` **不含缓冲内容**
- AC: 既有解码测试（逐字节喂入、任意切分点、分隔符不残留）全部保持通过
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-006 — P5 收口：架构文档同步 + 契约升格 + 故障注入

把 P5 落地后的事实**回写**到架构文档，并把不变量契约**升格**为 `verified`。

同时做**门禁自身的防伪验证**（NEX-24 / NEX-25 的教训）。

对应 backlog 任务：`backlog/tasks/NEX-36 - docs(api) P5 收口 —— 架构文档同步与契约升格.md`
依赖：T-001、T-002、T-003、T-004、T-005
对应设计条款：架构文档 §6.1、§6.2

- AC: 架构文档 §6.1 的 P5 行标记为**完成**，并附**可核验**的交付摘要（不变量条数、测试数）
- AC: `docs/specs/p5/contracts/invariants.json` 的全部不变量 `status` 提升为 `verified`
- AC: `bash scripts/check-specs.sh` 通过（规则 3b 要求：任务全部归档后不得残留 `planned`）
- AC: **故障注入**：对 P5 的关键断言做注入，验证**确实会红** —— 至少覆盖：帧格式、分隔符残留、事件名映射、cause 泄露
- AC: 故障注入的记录写进任务文件（含「注入什么 / 期望 / 实际」）
- AC: `docs/operations/` 有 P5 的运行说明（`/metrics` 已在 P3 runbook；SSE 端点需补「怎么访问 / 怎么解析」）
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
