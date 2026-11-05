---
id: NEX-35
title: "feat(api): SSE 解码器增加帧大小上限"
status: Done
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

- [x] #1 `crates/nexus-api/src/sse.rs` 定义帧大小上限常量（默认 1 MiB），且可被测试引用
- [x] #2 缓冲区超过上限时 `push` 返回 `FrameError::FrameTooLarge`，**不 panic**
- [x] #3 返回错误后**不再继续增长**缓冲区（测试断言 `buffered()` 不随后续喂入增加）
- [x] #4 恰好等于上限的帧仍能正常解码（边界用例）
- [x] #5 上限对 LF（`\n\n`）与 CRLF（`\r\n\r\n`）两种分隔符都生效
- [x] #6 `FrameError::FrameTooLarge` 的 `Display` **不含缓冲内容**（不把上游负载写进日志/响应）
- [x] #7 既有解码测试（逐字节喂入、任意切分点、分隔符不残留）全部保持通过
- [x] #8 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

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

## 决策记录

### 类别：decide-and-proceed

- **决策**：上限约束的是**单帧内容**（**不含**分隔符），**不是**整个 chunk。
- **依据**：一次 `push` 可以包含**多个**帧（每个帧各自成帧）。
  若按 chunk 长度计，一次喂入 5 个小帧就会被误杀 ——
  那等于「帧大小」这个概念失效。
- **可逆性**：高 —— 判据是本模块内的一个常量与一处比较。
- **影响面**：`crates/nexus-api/src/sse.rs`。

### 类别：decide-and-proceed

- **决策**：超限后解码器进入**不可恢复**状态 —— 释放缓冲，后续 `push` 直接返回**同一个**错误。
- **依据**：超限时**分隔符还没出现**，所以**不知道下一帧从哪开始** ——
  **无法重新同步**。
  若「清空缓冲后继续正常解析」，攻击者就能**反复**把内存撑到上限
  （每次超限 → 解码器复位 → 再撑一次），把一次 DoS 变成**无限**次。
- **可逆性**：中 —— 语义变更需要改 `push` 的状态机；
  但错误类型与对外行为不变。
- **影响面**：`crates/nexus-api/src/sse.rs`。

### 类别：decide-and-proceed

- **决策**：`FrameError::FrameTooLarge` 只携带 `limit` 与 `actual` 两个**长度**，
  **不携带**任何缓冲内容。
- **依据**：上游字节是**不可信输入**。把它写进 `Display` 就等于
  让它进入**日志**（进而可能进入响应体）。
  而 `actual` 是**长度**不是**内容** —— 长度不泄露任何负载，
  且是诊断必需的（运维要知道自己超了多少）。
- **可逆性**：高 —— 变体字段。
- **影响面**：`crates/nexus-api/src/sse.rs`。

### 类别：decide-and-proceed

- **决策**：提供 `FrameDecoder::with_limit(usize)` 让调用方覆盖上限。
- **依据**：AC #1 说「**默认** 1 MiB」—— 「默认」预设了「可覆盖」。
  不同上游的帧大小差别很大，写死 1 MiB 会让某些场景**无法使用**。
- **可逆性**：高 —— 一个构造函数。
- **影响面**：`crates/nexus-api/src/sse.rs`。

## 实现过程中的发现

### 发现 1 —— `Default` 不能 derive —— `usize` 的默认值是 0

`FrameDecoder` 原本 `#[derive(Debug, Default)]`。
加上 `limit: usize` 字段后，derive 出来的默认值是 **0** ——
那会让解码器**拒绝一切**非空帧（任何内容都 > 0）。

**修正**：手写 `Default`，`limit` 取 `MAX_FRAME_BYTES`。

**教训**：加了「有默认值语义」的字段后，derive 的默认值**不再**安全。
`0` 对 `usize` 是**合法**的值 —— 编译器**不会**提醒你。

### 发现 2 —— 超限检查必须在**切走之前**

最初把大小检查放在 `drain(..pos + delim_len)` **之后** ——
那时超大帧已经**完整搬进** `raw`（又一次完整拷贝）才报错。

**修正**：在 `find_delimiter` 拿到 `pos` 后**立刻**检查，
`drain` 之前返回错误。

**教训**：「检查」的位置决定了**峰值内存**。
放在拷贝之后 = 峰值 = 2 倍帧大小。

### 发现 3 —— 「无分隔符」这条路径**也要**检查

只在「找到分隔符」的分支里检查大小，会**漏掉**真正的攻击形态 ——
上游**永远不送分隔符**时，永远走的是「找不到分隔符」的分支。

**修正**：该分支里也检查 `self.buf.len() > self.limit`。
那里的缓冲区长度就是该帧内容的**长度下界**（只会更大，不会更小）。

### 发现 4 —— 释放缓冲要用 `Vec::new()` 而不是 `clear()`

`clear()` 只把**长度**归零，**容量**还在 —— 那 1 MiB 仍然占着内存。
而解码器已经不可恢复，这块内存**不会被复用**。

**修正**：`self.buf = Vec::new()`。

**教训**：「不再增长」与「不再占用」是两件事。
`clear()` 只满足前者。

### 发现 5 —— 上限约束**保留量**，不约束**瞬时峰值**

`push` 先把整个 chunk 拷进缓冲区，再判断超没超限。
所以单次传入巨型 chunk 时，峰值内存会短暂达到 `chunk.len() + 旧缓冲`。

**这不是漏洞**：调用方本来就**持有**那个 chunk（上游 socket 读出来的字节），
我们的拷贝不会让它超出**一个副本**。
上限防的是「**跨多次 push 累积**」—— 那才是无界的。

**已在 `push` 的文档注释里显式写明** —— 不让读代码的人误以为峰值有界。

### 发现 6 —— 我自己写的两条测试**期望错了**

`an_oversized_frame_is_rejected_without_panicking` 最初用
「64 字节内容 = 超限」来写 —— 但 64 **恰好等于**上限，应该**成功**。
结果是返回了 `MalformedFrame`（内容全是 `x`，既无 event 也无 data）。

`the_too_large_error_carries_no_buffered_content` 最初用
`with_limit(1024)` 塞 35 字节 —— **根本没超限**，所以 `push` 返回 `Ok`。

**修正**：分别改成「65 字节 > 上限 64」与「塞到超过 64」。

**教训**：这两条**不是**实现的问题，是**测试自己写错了**。
「测试红了」的第一反应应该是「**断言错了还是实现错了**」——
先分清再改，否则会把测试改成「迎合实现」。

## 故障注入（9 项，最终全部**确实会红**）

在隔离工作树里逐项注入变异，确认对应测试**失败**。

| # | 注入 | 期望变红 | 结果 |
|---|---|---|---|
| 1 | 上限检查改成 `>=`（off-by-one） | `a_frame_exactly_at_the_limit_is_decoded` | FAILED ✓ |
| 2 | 上限按 `chunk.len()` 而非单帧计 | `the_limit_is_applied_per_frame_not_per_chunk` | FAILED ✓ |
| 3 | 只在「找到分隔符」分支检查大小 | `an_oversized_frame_without_a_delimiter_is_rejected` | FAILED ✓ |
| 4 | 超限后只 `clear()` 不释放容量 | `the_decoder_stays_broken_after_exceeding_the_limit` | FAILED ✓ |
| 5 | 超限后**继续**缓冲（不置不可恢复态） | `the_buffer_stops_growing_after_the_limit_is_exceeded` | FAILED ✓ |
| 6 | `Display` 里带上缓冲内容 | `the_too_large_error_carries_no_buffered_content` | FAILED ✓ |
| 7 | CRLF 分支不检查大小 | `the_limit_applies_to_the_crlf_delimiter_variant` | FAILED ✓ |
| 8 | 大小检查挪到 `drain` 之后 | `an_oversized_frame_after_a_normal_frame_is_rejected` | FAILED ✓ |
| 9 | `Default` 的 limit 取 0 | `the_default_limit_is_one_mebibyte` | FAILED ✓ |

## 验证结果

- `cargo test -p nexus-api --lib`：45 个单元测试全绿（原 34 + 新增 11）
- `cargo test --workspace --locked`：543 个测试全绿，0 失败
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- `scripts/check-lock.sh` / `check-deps.sh` / `check-naming.sh` /
  `check-backlog.sh` / `check-specs.sh` 通过
- 不变量契约新增 P5-I31..P5-I36（6 条，全部 `verified`）
- `docs/specs/p5/spec.md` §3.1.2 补入上限语义表
