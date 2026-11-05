---
id: NEX-32
title: "feat(api): 落地事件名映射与 SSE 桥接"
status: Done
assignee: []
labels: [api, sse, p5]
priority: high
dependencies:
  - NEX-31
references:
  - "docs/specs/p5/spec.md §3.2、§3.3"
  - "docs/specs/p5/tasks.md T-002"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1"
---

## Description

**决策 D8 的落地**：手写 `Body::from_stream`，**不用** axum 的 `Sse`。

12 个事件名**原样**映射；事件流 → 帧序列 → 响应体。

### 为什么不用 axum 的 `Sse`

参考架构 §1.5 **已定稿**：

> axum 的 `Sse` 会用 `Event::default().data()` 自动转义，与 Go 版手写
> `event: / data:` 的字节输出不完全一致。**建议 Rust 版手写 `Body::from_stream`**。

前端 `lib/sse.ts` 是**按字节解析**的 —— 格式差异会表现为
「前端收不到事件」这类**极难排查**的问题。

### 事件名是前后端契约

`nexus_core::EventPayload` 的 12 个变体 → SSE `event:` 名：

`meta` / `delta` / `reasoning` / `plan` / `tool_call` / `tool_result` /
`agent_event` / `refs` / `usage` / `approval` / `done` / `error`

**不得**加前缀、不得改大小写、不得用 Rust 变体名（`ToolCall` ≠ `tool_call`）。

改一个名字，前端那个分支就**静默失效**了。

## 范围边界

### 做

- `crates/nexus-api/src/bridge.rs`：`event_name(&EventPayload) -> &'static str` + 桥接
- `crates/nexus-api/tests/sse_bridge.rs`：行为层测试

### 不做

| 项 | 理由 |
|---|---|
| 鉴权中间件 | 属 T-004 |
| 心跳的**定时器** | 心跳频率是**路由**的职责；桥接只做「事件 → 帧」 |
| 落库 / 事件泵 | 属 `nexus-service`（决策 D9） |

## Acceptance Criteria

- [x] #1 `crates/nexus-api/src/bridge.rs` 实现 `event_name(&EventPayload) -> &'static str`，**全部 12 个**事件名正确
- [x] #2 事件名测试**覆盖全部 12 个** —— 不是抽查
- [x] #3 `crates/nexus-api/src/bridge.rs` 实现事件流 → `Body::from_stream` 的桥接
- [x] #4 桥接**手写**字节输出 —— 源码里**不含** `axum::response::sse`
- [x] #5 事件流结束 → 流结束，**不补**额外的帧
- [x] #6 事件流出错 → 发 `error` 帧，含**错误码**，**不含**内部原因（`cause`）
- [x] #7 **客户端断开** → `CancellationToken` 触发
- [x] #8 桥接的**每帧输出**都能被 T-001 的解码器**完整解回**
- [x] #9 `crates/nexus-api/tests/architecture.rs` 断言源码里**不含** `axum::response::sse`
- [x] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

10 条 AC 全部可二元判定 —— 每条都指明「函数存在 / 行为可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.2、§3.3` —— 存在
- `docs/specs/p5/tasks.md T-002` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1` —— 存在
- `crates/nexus-agent/src/events.rs`（`EventStream` 类型）—— 存在

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增**一个**模块 + 一个测试文件。

**不触及**：42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/bridge.rs` | **新增** |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/tests/sse_bridge.rs` | **新增** |
| `crates/nexus-api/tests/architecture.rs` | **修改/新增** —— 结构层断言 |
| `crates/nexus-api/Cargo.toml` | **不修改** —— `tokio` / `axum` / `nexus-agent` / `nexus-core` 已在依赖里 |

### #6 完成态可描述

`nexus-api` 能把一条 `EventStream` 转成一个**真实可跑**的 SSE 响应体，
字节格式与参考实现一致，且客户端断开能触发取消。

### #7 无隐形依赖

- 前置任务 NEX-31 已登记在 `backlog/tasks/`
- `nexus-agent` 已在 `nexus-api` 的依赖里（`check-deps.sh` 允许）
- `nexus-core` 已在依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-31 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：桥接的输入类型是 `nexus_agent::events::EventStream`。
- **依据**：P4 已交付的 sink 产出的就是这条流；复用它**不重复造轮子**，
  且保证「序号严格单调」这条不变量**贯穿**到 SSE 帧。
- **可逆性**：中 —— 若 P4 的 sink 形态变化，桥接要跟着改。
- **影响面**：`crates/nexus-api/src/bridge.rs`。

### 类别：decide-and-proceed —— 架构文档 §4.1.1 依赖方向表的措辞修正

- **决策**：把架构文档 §4.1.1 依赖方向表的「协议 → 执行（API 不碰执行语义）」
  改为「协议 → **执行语义**（API 不碰执行语义；只读**类型**可借用）」，
  并补一条 2026-11-05 修正注记，把判据明确为「**类型可以借，语义不能借**」。
- **依据**：原措辞被读成「协议层**不得**依赖执行层」，与两处既有事实矛盾 ——
  ① `nexus-api` 的 `Cargo.toml` **本来就**依赖 `nexus-agent` / `nexus-execution`；
  ② `scripts/check-deps.sh` 一直把它们列为允许。
  也就是说原措辞与**已生效的**依赖声明自相矛盾，缺的正是一条「借用类型」的判据。
  桥接只读 `nexus_agent::events::EventStream` **类型**，不调用任何执行逻辑 ——
  符合架构文档 §1.4「协议层不能感知业务规则、事务边界」的**本意**。
- **可逆性**：高 —— 只是文档措辞；判据若被证明不合适，可再改回并同步
  `scripts/check-deps.sh` 的允许清单。
- **影响面**：`docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` §4.1.1 / §4.3.3 表。

> **为什么这属于 (a) decide-and-proceed 而不是 (b) timeboxed**：
> 这是**收紧**控制（把模糊的「不碰执行语义」写成可判定的「类型可以借，语义不能借」）
> 而非削弱；文档改动可低成本回滚；影响面限于一份文档的措辞与一张表。
>
> **边界变化仍按违规处理**：若协议层开始**调用** `nexus-agent` 的执行函数，
> 结构层测试与评审会拦下 —— 本次修正**不改变**依赖方向的实质约束。

## 实现过程中的发现

### 发现 1 —— `Body::from_stream` **不设置任何响应头**（真实缺陷，初版踩到）

初版的 `sse_body` 只返回裸 `Body`。测试断言 content-type 时才发现
响应头**完全缺失** —— `Body::from_stream` 只包字节流，**不碰**响应头。

不设 `Content-Type: text/event-stream` 的话，前端的 `fetch` + `ReadableStream`
解析器**不会**把它当 SSE 处理（`EventSource` 更是直接拒绝非该 MIME）。

**修正**：新增 `sse_response` 返回**完整响应**（含 `Content-Type`）。
`SSE_CONTENT_TYPE` 是**公开常量** —— 让调用方能断言。

### 发现 2 —— **正常结束**与**上游出错**都**不是**客户端断开（真实缺陷）

初版只用 `Drop` 触发取消。但这有个致命漏洞：

**响应体被 drop 有三种原因** ——
1. 客户端断开（真取消）
2. 流**正常结束**（响应体写完，axum 收尾时 drop）
3. 上游**出错**结束

初版把这三种**全部**当成「客户端断开」——
上层（service / agent）会把**一次正常收尾**当成「用户取消」，
直接违反 P4 的 P4-I21（取消 ≠ 失败）与 P4-I22（先固化错误再取消）。

**修正**：引入 `CancelOnDrop::disarm()` ——
流走完或上游出错时**解除**取消触发，只留「客户端断开」这一条路径。

新增两个回归测试：
- `finishing_the_stream_normally_does_not_cancel`
- `an_upstream_error_does_not_cancel`

### 发现 3 —— 内部错误的 `Display` **不能**拼进用户可见消息

初版的错误帧消息是 `format!("事件流中断: {err}")` ——
把内部错误的 `Display`（含内部类型名）拼进了**面向用户**的 `message` 字段，
违反架构文档 §2.6.5。

**修正**：消息固定为 `"事件流中断"`，**不含**任何内部原因。
错误码用 `ErrorCode::ServiceUnavailable.code()`（**不硬编码** `90003`）——
硬编码的码值会与架构文档 §2.6.4.1 漂移。

### 发现 4 —— `EventPayload` 与 `EventStreamError` 有**同名但不同类型**的两份

`nexus_core::EventStreamError` 与 `nexus_agent::events::EventStreamError`
**同名但不同类型**（P4 的 D7 决策的结果）。

写测试时 import 错了会得到一段极长的类型不匹配报错。
**必须显式写全路径**。

### 发现 5 —— `Extension` 逃生舱**不在** 12 个契约事件名里

架构文档 §2.2.4.1 定稿的是 **12 个**事件名，`Extension` **不在**其中
（它是**逃生舱**，不是契约的一部分）。

但它**必须**有事件名 —— 否则无法产出帧。
这里给它 `"extension"`，与 serde 内部标签的 snake_case 形态一致。

`event_name` 用**穷尽匹配**而非 serde 的 `type` 字段 ——
后者会让新变体**自动**有名字，但**没人会注意到**它进了 SSE 协议。
穷尽匹配让新增变体**编译失败**，强制补齐映射。

### 发现 6 —— **故障注入**验证（10 项，全部**确实会红**）

| 注入 | 结果 |
|---|---|
| 事件名 `tool_call` → `toolCall` | FAILED ✓ |
| 事件名 `done` → `finish` | FAILED ✓ |
| 事件名 `agent_event` → `agent` | FAILED ✓ |
| 错误帧带 `cause` | FAILED ✓ |
| 错误码 `ServiceUnavailable` → `Internal` | FAILED ✓ |
| 正常结束**也**取消（删 `disarm`） | FAILED ✓ |
| 上游出错**也**取消（删 `disarm`） | FAILED ✓ |
| 断开**不**触发取消 | FAILED ✓ |
| `data` 里加 `run_id` | FAILED ✓ |
| 源码里用 `axum::response::sse` | FAILED ✓（结构层断言） |

最后一项的注入过程有**两次**失败（类型约束不对导致编译错误），
最终用「仅类型路径」的写法验证 —— 结构层断言的报错精确指出了违规文件。

### 验证结果

- `cargo test -p nexus-api`：38 个测试（18 单元 + 4 结构 + 7 指标 + 9 桥接）全绿
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- `scripts/check-specs.sh` 通过（P5-I10~I15、I24、I25 已升格为 `verified`）

### 复核 —— 故障注入（**独立 worktree** 重跑，2026-11-05）

原提交的注入记录见「发现 6」。为排除「注入脚本本身写错导致假红」，
在**独立 git worktree**（`@ 7c56065`，不碰主工作区）重跑了 13 项注入：

| 注入 | 结果 |
|---|---|
| 事件名 `tool_call` → `toolCall` | FAILED（17 passed / 1 failed）✓ |
| 事件名 `done` → `finish` | FAILED（17 passed / 1 failed）✓ |
| 事件名 `agent_event` → `agent` | FAILED（17 passed / 1 failed）✓ |
| 错误帧带 `cause` | FAILED（17 passed / 1 failed）✓ |
| 错误码 `ServiceUnavailable` → `Internal` | FAILED（8 passed / 1 failed）✓ |
| 正常结束**也**取消（删 `disarm`） | FAILED（8 passed / 1 failed）✓ |
| 上游出错**也**取消（删 `disarm`） | FAILED（8 passed / 1 failed）✓ |
| 断开**不**触发取消 | FAILED（8 passed / 1 failed）✓ |
| `data` 里加 `run_id` | FAILED（17 passed / 1 failed）✓ |
| 帧字节格式被破坏（`event:` 后少空格） | FAILED（16 passed / 2 failed）✓ |
| 帧分隔符缺失（`\n\n` → `\n`） | FAILED（13 passed / 5 failed）✓ |
| 心跳被当畸形帧 | FAILED（8 passed / 10 failed）✓ |
| 源码里用 `axum::response::sse` | FAILED —— 结构层断言**精确指出违规文件** ✓ |

恢复后：`18 + 4 + 7 + 9` 全绿。

#### 复核中发现的一处**方法论缺陷**

第一版复核脚本把 cargo 结尾的 `error: test failed, to rerun pass ...`
误判为「编译失败」—— 于是 13 项注入**全部**显示「会红」，
其中「使用 axum 的 `Sse`」那一项其实**只是编译不过**（泛型参数写错），
**并没有真正触发**结构层断言。

**这正是「测试存在 ≠ 断言有效」的同一类陷阱的变体**：
判定脚本自己也会有假绿 / 假红。修正判据（只认 `error[E…]` 与
`could not compile`，测试失败单独看 `test result: … FAILED`）后重跑，
并对 `Sse` 注入改用**正确的泛型参数**，结构层断言才真正被触发：

```text
test the_api_crate_does_not_use_the_axum_sse_type ... FAILED
决策 D8 要求手写 Body::from_stream，不得使用 axum 的 Sse 类型。
违规文件：["…/crates/nexus-api/src/bridge.rs"]
```

**教训**：注入验证的**判据**与**被验证的断言**一样需要被验证。
「13/13 会红」这个结论本身，在判据写错时是**无意义的**。
