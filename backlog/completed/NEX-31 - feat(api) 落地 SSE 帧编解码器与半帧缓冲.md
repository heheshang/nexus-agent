---
id: NEX-31
title: "feat(api): 落地 SSE 帧编解码器与半帧缓冲"
status: Done
assignee: []
labels: [api, sse, p5]
priority: high
dependencies: []
references:
  - "docs/specs/p5/spec.md §3.1"
  - "docs/specs/p5/tasks.md T-001"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §1.5"
---

## Description

**P5 的地基**：所有后续任务都建立在这套编解码上。

`FrameEncoder`（事件 → 字节帧）+ `FrameDecoder`（字节流 → 帧，**半帧缓冲**）。

### 为什么半帧缓冲是 P5 的完成判据

架构文档 §6.1 给 P5 的完成判据是「**SSE 半帧处理有测试**」。

这不是随便挑的 —— 它是 SSE 协议里**唯一**真正会出错的地方：
TCP 分包可能把一帧劈成两半。读侧收到半帧就解析会得到**截断的 JSON**，
前端表现为「事件莫名其妙少了几条」，**极难排查**。

### 回归基准

`examples/demo-sse-frames` 已有**逐字节喂入**与**任意切分点**的测试 ——
那是参考实现的回归基准。P5 的解码器**必须**通过同形态的测试。

### ⚠️ 附录 C.1 的陷阱

> SSE 帧解码只切走帧内容、不切分隔符 → **死循环** ——
> 分隔符留在缓冲区头部，`position()` 永远返回 0。

正确做法是 `split_to(pos + DELIM.len())` —— 分隔符**连同帧内容一起**切走。

## 范围边界

### 做

- `crates/nexus-api/src/sse.rs`：`encode_frame` + `FrameDecoder` + `Frame` 类型
- 对应的单元测试（放在同文件的 `#[cfg(test)]` 模块里）

### 不做

| 项 | 理由 |
|---|---|
| 事件名映射 | 属 T-002（`EventPayload` → SSE `event:`） |
| 桥接（流 → `Body`） | 属 T-002 |
| 用 `eventsource-stream` crate | 那是**读侧**（解析上游 SSE）的库；本模块是**写侧**（产出 SSE 帧）。`nexus-llm` 已经用了它 |

## Acceptance Criteria

- [x] #1 `crates/nexus-api/src/sse.rs` 实现 `encode_frame(event: &str, data: &Value) -> Vec<u8>`，输出**逐字节**等于 `"event: {name}\ndata: {json}\n\n"`
- [x] #2 编码器是**纯函数** —— 同样输入得到同样字节
- [x] #3 `crates/nexus-api/src/sse.rs` 实现**增量式**解码器 —— `push(chunk) -> Vec<Frame>`
- [x] #4 解码器**缓冲**不完整的尾部帧，缓冲字节数**可观测**（有 `buffered()` 之类的方法）
- [x] #5 分隔符**连同帧内容一起切走** —— 连续多帧解码后缓冲区**清空**（防死循环回归）
- [x] #6 解码器容忍 **CRLF 变体**（`\r\n\r\n` 分隔）
- [x] #7 空帧 / 既无 `event` 又无 `data` 的帧 → **返回错误**，不静默通过
- [x] #8 逐字节喂入测试：把一帧切成 N 片喂入，最终得到**完整**的一帧
- [x] #9 任意切分点测试：在帧的**每一个**字节位置切断，两次喂入后结果相同
- [x] #10 多行 `data:` 按 SSE 规范用 `\n` 连接
- [x] #11 注释行（心跳）被忽略
- [x] #12 断言用的是**字面量** —— 不是引用被测代码里的常量（NEX-25 的教训：同义反复）
- [x] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

13 条 AC 全部可二元判定 —— 每条都指明「函数存在 / 行为可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.1` —— 存在
- `docs/specs/p5/tasks.md T-001` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.2.4.1` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §1.5` —— 存在
- `examples/demo-sse-frames/src/main.rs` —— 存在（回归基准）

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增**一个**模块。

**不触及**：42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/sse.rs` | **新增** |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/Cargo.toml` | **不修改** —— `serde_json` / `bytes` 已在依赖里（或需确认 `bytes`） |

### #6 完成态可描述

`nexus-api` 有一个**独立可用**的 SSE 帧编解码器：
给一串字节，能解出全部帧；给一个事件名与负载，能得到逐字节正确的帧。

### #7 无隐形依赖

- 无前置任务
- `serde_json` 已在 `nexus-api` 的依赖里
- `bytes` —— **需在实现时确认**；若无则用 `Vec<u8>`（不构成阻塞）

### 结论

**pass** —— 七项全过，可以开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：解码器自己实现，**不引入** `eventsource-stream` crate。
- **依据**：`eventsource-stream` 是**读侧**（解析上游 SSE）的库；
  本模块是**写侧**（产出 SSE 帧）。`nexus-llm` 已经用了前者，
  两者不冲突。引入它会让「自己产出自己解析」的回归测试**失去意义** ——
  那样测的是「crate A 能否解析 crate A 的输出」，而不是「输出是否符合参考实现的字节格式」。
- **可逆性**：高 —— 局部改动。
- **影响面**：`crates/nexus-api/src/sse.rs`。

## 实现过程中的发现

### 发现 1 —— 心跳帧**不能**判成畸形帧（真实缺陷，初版踩到）

初版把「既无 `event` 又无 `data`」一律判成 `MalformedFrame`。
结果**心跳帧**（`: ping\n\n`，只有注释行）让 `push` 直接返回 `Err` ——
**每隔一次心跳就中断整条流**。

心跳是**保活**机制，让它打断连接是**完全相反**的效果。

**修正**：`parse_frame` 的返回类型从 `Result<Frame, _>` 改成 `Result<Option<Frame>, _>`：
- `Ok(None)` —— **纯注释帧**（心跳），被**跳过**；
- `Err(MalformedFrame)` —— **真空帧**（`\n\n`，分隔符紧跟分隔符）或既无 `event` 又无 `data` 的非注释帧。

⚠️ **「真空帧」与「纯注释帧」必须区分** —— 这是本任务最容易搞混的一处：
前者是**畸形**，后者是**合法的心跳**。

### 发现 2 —— `find_delimiter` 必须比较**位置**而非只认一种分隔符

LF（`\n\n`）与 CRLF（`\r\n\r\n`）都要认，且要取**更早**的那个。
只找一种会在另一种出现时把缓冲区切错位置。

### 发现 3 —— `drain(..pos)` 之后**还要** `drain(..delim_len)`

用 `split_to` 的写法（`examples/demo-sse-frames`）一次切走「内容 + 分隔符」。
用 `drain` 两次切是等价的，但**两次切更容易写漏** —— 漏掉第二次就
让分隔符留在缓冲区头部（发现 4 的注入正是这一处）。

### 发现 4 —— **故障注入**验证（6 项，全部**确实会红**）

| 注入 | 结果 |
|---|---|
| 帧尾分隔符少一个 `\n` | FAILED（10 passed / 4 failed）✓ |
| `event:` 后**两个**空格 | FAILED（11 passed / 3 failed）✓ |
| **分隔符不切走**（死循环回归） | FAILED（9 passed / 5 failed）✓ |
| 半帧**丢弃**而非缓冲 | FAILED（8 passed / 6 failed）✓ |
| **不识别** CRLF 变体 | FAILED（13 passed / 1 failed）✓ |
| 畸形帧**静默通过** | FAILED（13 passed / 1 failed）✓ |

恢复后：`14 passed / 0 failed`。

### 发现 5 —— 「测试存在」不等于「断言真的有效」

第 4 项注入（分隔符不切走）会让 `push` **死循环** ——
旧实现下那个测试会**挂住**而不是失败。
所以「缓冲区清空」这条断言同时是**实现不会退化**的保证。

### 验证结果

- `cargo test -p nexus-api`：14 个新测试 + 7 个既有测试全绿
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- `scripts/check-specs.sh` 通过（P5-I01..I09 已升格为 `verified`）
