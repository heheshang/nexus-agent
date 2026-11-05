---
id: NEX-35
title: "feat(api): SSE 解码器增加帧大小上限"
status: To Do
assignee: []
labels: [api, sse, hardening]
priority: medium
dependencies: []
references:
  - "docs/specs/p5/spec.md §3.1.2"
  - "docs/specs/p5/tasks.md T-005"
  - "crates/nexus-api/src/sse.rs"
---

## Description

**问题**：`FrameDecoder` 的缓冲区**没有上限**。只要上游持续发送不含分隔符
（`\n\n` / `\r\n\r\n`）的字节，`buf` 就会一直增长，直到进程内存耗尽。

触发面有两类：

1. **上游异常** —— 供应商的 SSE 流损坏、代理截断，分隔符永远不到达；
2. **恶意输入** —— 任何能让解析器读到攻击者可控字节流的场景。

P5 的解码器目前只用于**自有格式的回归测试**与未来的 SSE 桥接，
风险尚未暴露；但它是**可复用的解析器**，不应把「内存无界」留成默认行为。

**修法**：给 `FrameDecoder` 加帧大小上限（默认 1 MiB），
超限返回 `FrameError::FrameTooLarge` —— **不 panic、不静默截断**。
达到上限时立即返回错误，缓冲区不再继续增长。

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/src/sse.rs` 定义帧大小上限常量（默认 1 MiB），且可被测试引用
- [ ] #2 缓冲区超过上限时 `push` 返回 `FrameError::FrameTooLarge`，**不 panic**
- [ ] #3 返回错误后**不再继续增长**缓冲区（测试断言 `buffered()` 不随后续喂入增加）
- [ ] #4 恰好等于上限的帧仍能正常解码（边界用例）
- [ ] #5 上限对 LF（`\n\n`）与 CRLF（`\r\n\r\n`）两种分隔符都生效
- [ ] #6 `FrameError::FrameTooLarge` 的 `Display` **不含缓冲内容**（不把上游负载写进日志/响应）
- [ ] #7 既有解码测试（逐字节喂入、任意切分点、分隔符不残留）全部保持通过
- [ ] #8 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

### #1 验收条件可二元测试

8 条 AC 全部可二元判定 —— 每条都指明「函数/常量存在」「返回值可观测」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` 等未决标记。
上限取「1 MiB」是**本任务给定的默认值**，不是待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.1.2` —— 存在（半帧缓冲的定义处）
- `docs/specs/p5/tasks.md T-005` —— 存在
- `crates/nexus-api/src/sse.rs` —— 存在

### #4 范围有界

**单文件 + 单模块测试**（`crates/nexus-api/src/sse.rs`）。
不触及 42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、
对外响应形状（`FrameError` 是 `nexus-api` 内部类型，不是 HTTP 契约）。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/sse.rs` | **修改** —— 常量 + `FrameError` 变体 + `push` 上限检查 + 测试 |
| `crates/nexus-api/src/lib.rs` | 不修改（既有 re-export 已覆盖 `FrameError`） |
| `crates/nexus-api/Cargo.toml` | 不修改 |

### #6 完成态可描述

解码器在缓冲达到上限时返回一个**明确的**错误，
且缓冲区不再增长；正常帧（含边界长度）行为不变。

### #7 无隐形依赖

- 无前置任务
- 不引入新依赖（只用 `std`）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，可以开工。
