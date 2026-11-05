---
id: NEX-31
title: "feat(api): 落地 SSE 帧编解码器与半帧缓冲"
status: To Do
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

- [ ] #1 `crates/nexus-api/src/sse.rs` 实现 `encode_frame(event: &str, data: &Value) -> Vec<u8>`，输出**逐字节**等于 `"event: {name}\ndata: {json}\n\n"`
- [ ] #2 编码器是**纯函数** —— 同样输入得到同样字节
- [ ] #3 `crates/nexus-api/src/sse.rs` 实现**增量式**解码器 —— `push(chunk) -> Vec<Frame>`
- [ ] #4 解码器**缓冲**不完整的尾部帧，缓冲字节数**可观测**（有 `buffered()` 之类的方法）
- [ ] #5 分隔符**连同帧内容一起切走** —— 连续多帧解码后缓冲区**清空**（防死循环回归）
- [ ] #6 解码器容忍 **CRLF 变体**（`\r\n\r\n` 分隔）
- [ ] #7 空帧 / 既无 `event` 又无 `data` 的帧 → **返回错误**，不静默通过
- [ ] #8 逐字节喂入测试：把一帧切成 N 片喂入，最终得到**完整**的一帧
- [ ] #9 任意切分点测试：在帧的**每一个**字节位置切断，两次喂入后结果相同
- [ ] #10 多行 `data:` 按 SSE 规范用 `\n` 连接
- [ ] #11 注释行（心跳）被忽略
- [ ] #12 断言用的是**字面量** —— 不是引用被测代码里的常量（NEX-25 的教训：同义反复）
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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
