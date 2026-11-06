---
id: NEX-48
title: "refactor(core): 用 token 级扫描器替换三份手写注释剥离器"
status: To Do
assignee: []
labels: [core, testing, architecture, defect]
priority: high
dependencies: []
references:
  - "docs/architecture/2026-11-06-nexusagent-improvement-plan.md §4.1 F-02"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §5.3（结构层断言）"
  - "docs/specs/p4/contracts/invariants.json"
  - "docs/specs/p5/contracts/invariants.json"
---

## Description

**问题**：同一份「剥离注释」的逻辑被**手抄了 3 份**，且**三份的行为不一样**。

| 副本 | 位置 | 失效方式 |
|---|---|---|
| api | `crates/nexus-api/tests/architecture.rs` | `&'static` 的生命周期 `'` **只开不关** → 从该点起几乎全部文本不再被剥注释（**假阳性**） |
| agent | `crates/nexus-agent/tests/events.rs` | **完全没有字符串状态** → `"http://x"` 里的 `//` 被当成行注释，**该行之后的真实代码被丢弃**（**真漏报**） |
| core | `crates/nexus-core/tests/architecture.rs` | 没有字符字面量状态 → 字符字面量 `'"'` 会**打开**字符串状态，其后的注释不再被剥（**假阳性**） |

实测对照（四组对抗输入，逐分支重放）：

| 对抗输入 | api | agent | core |
|---|---|---|---|
| 注释里含违规标记 | 正确 | 正确 | 正确 |
| `&'static` **之后**的注释 | ❌ 漏剥 | 正确 | 正确 |
| 字符串 `"http://x"` 后的真实代码 | 保留 | ❌ **吞掉整行** | 保留 |
| 字符字面量 `'"'` 之后的注释 | 正确 | 正确 | ❌ 漏剥 |

**为什么这比「手抄三份」本身更严重**：这三份是**结构层断言的底层工具**。
结构层断言是本仓库「让非法状态不可表示」的主要防线之一（架构文档 §5.3 列了 16 项）。
底层扫描器漏报，**上层断言全部失效**，而门禁**照样绿**。

**修法**：**删掉三个手写词法器**，改用 `syn` 解析成 token 流，只扫 token、不扫原始文本。
`syn` 是 **dev-dependency**，不影响生产依赖方向。

## 范围边界

### 做

- `crates/nexus-core/tests/support/` 下的**单一** token 级扫描实现
- `crates/nexus-api/tests/architecture.rs` —— 换成共用实现
- `crates/nexus-agent/tests/events.rs` —— 换成共用实现
- `crates/nexus-core/tests/architecture.rs` —— 换成共用实现
- 四组对抗输入的**负控**测试

### 不做

| 项 | 理由 |
|---|---|
| 改**生产代码** | 本任务只换**测试工具**；生产代码一个字都不动 |
| 改任何**架构断言的判定内容** | 断言的**语义**不变 —— 只换「怎么读源码」。放宽断言 = 门禁失效 |
| 新增 `syn` 到**生产依赖** | `syn` 只能是 dev-dependency（架构文档 §4.4 规则 2） |
| 引入 `proc-macro2` 作为直接依赖 | 用 `syn` 的 re-export 即可，不额外声明 |

## Acceptance Criteria

- [ ] #1 三份手写 `strip_comments` **全部删除** —— 全仓**只**剩**一份** token 级实现
- [ ] #2 共用实现放在**一个** crate 的测试支撑目录下，由其他 crate 引用 —— **不复制**代码
- [ ] #3 引用方向：`nexus-api` / `nexus-agent` 的测试依赖 `nexus-core` 的**测试支撑** —— **不得**产生**生产**依赖边
- [ ] #4 **反例 1**：`&'static str` **之后**的注释被正确剥离（api 副本今天的失败点）
- [ ] #5 **反例 2**：字符串 `"http://x"` 之后的真实代码**保留**（agent 副本今天的失败点）
- [ ] #6 **反例 3**：字符字面量 `'"'` 之后的注释被正确剥离（core 副本今天的失败点）
- [ ] #7 **反例 4**：字符串内含 `/*` **不**被当成块注释开始
- [ ] #8 **负控**：任一反例写错时对应测试**必须红**
- [ ] #9 三处**原有的**结构层断言（api 6 条、agent 2 条、core 5 条）**全部**保持通过 —— 断言的**判定内容**不变
- [ ] #10 **全仓复扫**：修正后**全仓复扫**的每条新暴露违规**要么修、要么登记为新 finding** —— **不得**靠放宽断言让它变绿
- [ ] #11 `bash scripts/check-deps.sh` 通过（`deps_of` ↔ `Cargo.toml` 一致 —— **不新增**生产依赖边）
- [ ] #12 `bash scripts/check-specs.sh` 通过（规则 3 引用的测试名**必须**仍然存在 —— 断言**改名**会红）
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 13 条 AC，均可写成断言或命令退出码
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 改进计划 §4.1 F-02、架构文档 §5.3、
  `docs/specs/p4/contracts/invariants.json`、`docs/specs/p5/contracts/invariants.json` 均存在
- 门禁 4（范围有界）：通过 —— 三个测试文件 + 一个新的测试支撑模块；
  **不动生产代码**，不改断言语义，不动契约
- 门禁 5（受影响面被指名）：通过 —— `crates/nexus-core/tests/support/`、
  `crates/nexus-api/tests/architecture.rs`、`crates/nexus-agent/tests/events.rs`、
  `crates/nexus-core/tests/architecture.rs`
- 门禁 6（完成态可描述）：通过 —— 三处结构层断言读源码时**只看 token**，
  四组对抗输入全部正确，漏报与假阳性同时消失
- 门禁 7（无隐形依赖）：通过 —— `syn` 3.0.9 及其传递依赖
  （`proc-macro2` / `quote` / `unicode-ident`）**已在 `Cargo.lock` 中**，
  不需要新的网络依赖；无前置任务

**结论：pass**

## 决策记录

### 类别：decide-and-proceed

- **决策**：用 `syn` 的 token 流替换手写词法器，**不**自己修那三个手写词法器。
- **依据**：手写词法器要正确处理 Rust 的**全部**词法形态
  （raw string / 字符字面量 / 嵌套块注释 / 生命周期 / 宏里的 token）——
  那等于**重新实现** `syn` 的一部分。`syn` 已在 `Cargo.lock` 里，
  作为 dev-dependency **不触碰**生产依赖方向（架构文档 §4.4 规则 2）。
- **可逆性**：高 —— 回退 = 恢复三个手写函数。
- **影响面**：三个测试文件 + 一个新的测试支撑模块。

### 类别：decide-and-proceed

- **决策**：共用实现放在 `crates/nexus-core/tests/support/`，
  由 `nexus-api` / `nexus-agent` 的测试**以 dev-dependency 的方式**引用。
- **依据**：`nexus-core` 是**依赖图的根**（零内部依赖）——
  其他 crate 依赖它**不产生新的边**，只是让「根」多了一条**向下的**引用。
  `nexus-core` 自己**不依赖**任何内部 crate（该约束由
  `nexus_core_depends_on_no_internal_crate` 断言）——
  所以由 `nexus-core` **提供**共用代码是最安全的方向。
- **可逆性**：高 —— 换一个目录即可。
- **影响面**：`crates/nexus-core/Cargo.toml`（新增 `[dev-dependencies]` 导出配置）、
  `crates/nexus-api/Cargo.toml`、`crates/nexus-agent/Cargo.toml`（**仅** dev-dependencies）。

### 类别：decide-and-proceed

- **决策**：AC #10 的「全仓复扫」在本任务内**执行并记录**，产物登记到改进计划的发现清单。
- **依据**：改进计划 §7.1 已把「修复扫描器会暴露大量新违规」列为**本计划本身的风险**，
  缓解手段是「逐条分诊：修 / 登记 / 豁免」。
  本任务**不**放宽任何断言来换取绿灯 —— 那是门禁失效的形态。
- **可逆性**：n/a（不动作）。
- **影响面**：`docs/architecture/2026-11-06-nexusagent-improvement-plan.md`（追加登记）。
