---
id: NEX-47
title: "fix(api): FrameDecoder 畸形帧后进入显式错误态，消除 panic"
status: To Do
assignee: []
labels: [api, sse, defect, panic]
priority: high
dependencies: []
references:
  - "docs/architecture/2026-11-06-nexusagent-improvement-plan.md §4.1 F-01"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1（P5 完成判据）"
  - "docs/specs/p5/spec.md §5（半帧缓冲的七条子目标）"
  - "docs/specs/p5/tasks.md T-001、T-005"
---

## Description

**问题**：`FrameDecoder` 在**畸形帧之后**会 `panic`，而不是返回错误。

复现序列（`c37d61e` 实测确认）：

```rust
let mut d = FrameDecoder::new();
d.push(&[b'x'; 100]).unwrap();   // ① 无分隔符 → scanned = 97
let r = d.push(b"ab\n\n");        // ② 畸形帧 → Err，但缓冲区已被 drain 到 1 字节
assert!(r.is_err());
let r = catch_unwind(|| d.push(b"z"));  // ③ → panic: &self.buf[97..] 越界
```

**根因**：`push` 用 `self.scanned` 缓存「上次扫过的字节数」，但**游标与缓冲区长度之间没有不变量**。
`parse_frame` 失败时 `?` **提前返回**，`scan_from` 仍是上一轮的旧值（97），
而缓冲区已被 `drain` 到只剩 1 字节 → **下一次** `push` 用旧游标切新缓冲区 → 越界。

**为什么 27 条解码器测试全绿**：测试覆盖了「畸形帧被拒绝」，
但**没有覆盖「拒绝之后继续喂」**。这是**状态转换缺口**，不是输入空间缺口 ——
补再多畸形输入也测不出来。

**为什么这是状态机问题而不是扫描问题**：`scanned` 失效的根因是
「游标与缓冲区长度之间没有不变量」。修法是让「畸形帧之后解码器进入显式错误态」
变成**类型可见**的事实 —— 调用方不必知道内部发生过 drain。

**当前严重度说明**：F-01 目前**没有生产调用方** ——
`FrameDecoder` 只被 `crates/nexus-api/src/sse.rs` 自己的单元测试和 `lib.rs` 的 re-export 引用；
`bridge.rs` 走的是 `encode_frame`（编码方向）。它仍是 `pub` 契约的一部分，
且这是**状态转换缺口**而非「输入空间没测全」，所以仍需修复。

## 范围边界

### 做

- `crates/nexus-api/src/sse.rs` 的 `FrameDecoder` 状态机改造
- 畸形帧之后进入**显式错误态**（`Poisoned`），后续 `push` **幂等**返回同一个错误
- 对应的单元测试 + **负控**

### 不做

| 项 | 理由 |
|---|---|
| 改 `FrameError` 的**变体集合** | `FrameError` 是 `pub` 契约；新增变体是破坏性变更 |
| 改 `parse_frame` 的语义 | 心跳（纯注释帧）必须仍被**跳过**而不是判错 —— 那是 P5 已验证的行为 |
| 改 SSE 帧格式 | 逐字节对齐是**已定稿**契约（架构文档 §2.2.4.1、参考架构 §1.5） |
| 改超限（`FrameTooLarge`）的语义 | NEX-35 已交付并有变异测试；本任务**不碰**那条路径 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/src/sse.rs` 的 `FrameDecoder` 用**显式状态**表达「正常」与「已中毒」两种状态 —— 中毒态下 `scanned` 游标**不再是**独立于状态的自由变量
- [ ] #2 畸形帧（既无 `event` 又无 `data`）被 `push` 拒绝时，解码器进入**显式错误态**
- [ ] #3 畸形帧之后，后续 `push` 返回**同一个**错误（幂等）—— 错误内容**逐字相等**，不只是「也报错」
- [ ] #4 上面的复现序列（① 无分隔符 → ② 畸形帧 → ③ 继续喂）**不 panic**，第 ③ 步返回 `Err`
- [ ] #5 中毒态下**缓冲区不再增长**；`limit` 仍然生效
- [ ] #6 中毒态下 `buffered()` 反映**实际保留量**，且解码器**不保留**已排空的畸形帧内容
- [ ] #7 **负控**：把游标重置移回 `parse_frame` 之后（恢复旧的失效行为）→ AC #4 的测试**必须红**
- [ ] #8 既有解码器测试（逐字节喂入、任意切分点、分隔符不残留、CRLF 变体、心跳跳过、超限系列）**全部**保持通过
- [ ] #9 `bash scripts/check-specs.sh` 通过（规则 3 引用的测试必须真实存在）
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 10 条 AC，均可写成断言或命令退出码
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 改进计划 §4.1 F-01、架构文档 §6.1、
  `docs/specs/p5/spec.md §5`、`docs/specs/p5/tasks.md` T-001/T-005 均存在
- 门禁 4（范围有界）：通过 —— 单 crate 的一个类型 + 其单元测试，无跨 crate 契约变更
- 门禁 5（受影响面被指名）：通过 —— `crates/nexus-api/src/sse.rs`（其余文件**不修改**）
- 门禁 6（完成态可描述）：通过 —— 调用方喂入畸形帧后持续拿到**同一个** `Err`，
  既不 panic 也不产出帧，且不必知道内部发生过 `drain`
- 门禁 7（无隐形依赖）：通过 —— 无前置任务；`catch_unwind` 是 `std` 内置，不引入依赖

**结论：pass**

## 决策记录

### 类别：decide-and-proceed

- **决策**：畸形帧之后解码器进入**显式错误态**（中毒），后续 `push` **幂等**返回同一个错误。
- **依据**：现有实现对**超限**（`FrameTooLarge`）已经是这个形态 ——
  `over_limit: Option<FrameError>` 让「不可恢复」成为类型可见的事实。
  畸形帧**同样**意味着上游已坏（现有文档已写明「畸形帧说明**上游已经坏了**，
  继续读只会产出更多垃圾。调用方应当**终止**这条流」），
  却**没有**同样的机制 —— 这是**不一致**，不是设计选择。
- **可逆性**：高 —— 纯局部状态机改造，`git revert` 即回到现状。
- **影响面**：`crates/nexus-api/src/sse.rs`。

### 类别：decide-and-proceed

- **决策**：**不**新增 `FrameError` 变体 —— 中毒态复用既有的 `MalformedFrame`。
- **依据**：`FrameError` 是 `pub` 契约（`lib.rs` 有 re-export）。
  新增变体 = 破坏性变更 = 需要前后端同步（架构文档 §6.3 第 3 项）。
  而「畸形帧」这个语义**已经**由 `MalformedFrame` 表达 —— 新增变体不增加信息。
- **可逆性**：n/a（不动作）。
- **影响面**：`crates/nexus-api/src/sse.rs`。

### 类别：decide-and-proceed

- **决策**：AC #7 的**负控**用「手工回退实现」验证，不进 CI。
- **依据**：仓库既有做法（NEX-12 的围栏变异测试、NEX-36 的故障注入）都是
  「手工 patch + 记录」而非脚本化 —— 规则文件里「变异测试」的要求**没有**脚本也没有 CI 步骤，
  改进计划的 H-02 已把它登记为「现在不做」。
  本任务沿用既有做法：负控在本任务内**执行并记录**，不引入新的 CI 机制。
- **可逆性**：n/a（不动作）。
- **影响面**：`crates/nexus-api/src/sse.rs`。
