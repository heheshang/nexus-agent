---
id: NEX-38
title: "feat(workflow): 落地冻结定义与图编译"
status: To Do
assignee: []
labels: [workflow, graph-engine, core]
priority: high
dependencies: []
references:
  - "docs/specs/p6/spec.md §3.1、§3.2"
  - "docs/specs/p6/tasks.md T-002"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2、§2.5.3"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.5.1、§3.5.2"
---

## Description

**问题**：Rust **没有** Eino 的对等物 —— 参考架构 §5.3 局限 #13 明写
「自研图引擎是最大工作量」。`crates/nexus-workflow/src/lib.rs` 目前只有 `CRATE_NAME`。

没有冻结定义与图编译，持久执行**无处落脚** —— driver 拿不到「这次跑的到底是什么」。

**修法**：落地两件事：

1. **冻结定义**（架构文档 §2.5.2 的三件套）—— `canonical` / `hash` / `engine_version`；
2. **图编译** —— 结构化 JSON 图定义 → `CompiledGraph`（拓扑排序 + 入度计数 + 展开上限）。

**为什么图编译要做「环检测」与「递归检测」**：编译期发现不了 = 运行时死循环或栈溢出。
参考架构 §3.5.2 的展开上限（深度 8 / 节点 128 / 状态 4 MiB）是**照抄**的，
不是本仓库的裁量。

## 范围边界

### 做

- `nexus-workflow/src/definition.rs`：`FrozenDefinition` + 哈希
- `nexus-workflow/src/graph.rs`：`CompiledGraph` / `CompiledNode` / `NodeKind` + 编译

### 不做

| 项 | 理由 |
|---|---|
| **DSL 文本前端** | 决策 D15（OQ-2）—— 登记为 F4 |
| 节点**执行**逻辑 | 属 NEX-39 |
| 节点**中断**提案 | 属 NEX-40 |
| 持久化 / 租约 / 幂等 | 属 NEX-37 / NEX-41 ~ NEX-45 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-workflow/src/definition.rs` 定义 `FrozenDefinition` —— 含 `canonical` / `hash` / `engine_version` **三个**字段
- [ ] #2 定义哈希用 `nexus_core::canonical::canonical_hash_hex` —— **不自己实现**规范化
- [ ] #3 规范化键序无关 —— 同一份定义、键序不同，`hash` **相同**
- [ ] #4 `crates/nexus-workflow/src/graph.rs` 定义 `CompiledGraph` —— 含 `nodes` / `roots` / `name`
- [ ] #5 节点种类**全部**存在：`Lambda` / `Branch` / `SubGraph` / `Parallel` / `Human` / `Tool`
- [ ] #6 触发语义是 `AllPredecessor` —— 节点**所有**前驱完成后才触发（测试：两个前驱只完成一个时节点**不**执行）
- [ ] #7 编译做**拓扑排序 + 入度计数**；图有**环**时编译**失败**（不是死循环）
- [ ] #8 **子图递归检测** —— 编译时用 `HashSet<NodeId>` 检测自引用，递归 → **错误**（不是无限展开）
- [ ] #9 展开上限**三个各自生效**：嵌套深度 > **8** → 错误；展开节点总数 > **128** → 错误；状态大小 > **4 MiB** → 错误
- [ ] #10 超限映射到 `ErrorCode::BudgetExceeded`（30006），**不是** panic
- [ ] #11 `crates/nexus-workflow/src/lib.rs` 的 crate 级文档写明「本 crate 负责图引擎 / 不负责持久化」
- [ ] #12 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

12 条 AC 全部可二元判定 —— 每条都指明「类型 / 常量存在」「返回值可观测」
「哈希相等 / 不等」「错误码可断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.1、§3.2` —— 存在
- `docs/specs/p6/tasks.md T-002` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §2.5.2、§2.5.3` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §3.5.1、§3.5.2` —— 存在
- `nexus_core::canonical::canonical_hash_hex` —— 存在于 `crates/nexus-core/src/canonical.rs:100`

### #4 范围有界

**单 crate**（`nexus-workflow`）的**两个**新模块 + 测试。
纯内存计算，**无**数据库、**无**并发写、**不新增** crate 依赖方向。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-workflow/src/definition.rs` | **新增** |
| `crates/nexus-workflow/src/graph.rs` | **新增** |
| `crates/nexus-workflow/src/lib.rs` | **修改**（模块声明 + crate 级文档） |
| `crates/nexus-workflow/tests/graph.rs` | **新增** |
| `crates/nexus-workflow/Cargo.toml` | **不修改**（`evalexpr` / `serde_json` / `nexus-core` 已声明） |

### #6 完成态可描述

调用方可以把一份结构化 JSON 图定义**编译**成 `CompiledGraph`；
环、递归、超限三种非法输入**全部被编译期拒绝**并返回明确错误；
同一份定义的哈希**与键序无关**。

### #7 无隐形依赖

- 无前置任务（`nexus_core::canonical` 已在 P3 落地）
- `evalexpr` 与 `serde_json` 已在 `crates/nexus-workflow/Cargo.toml` 声明 —— **不引入新依赖**
- DSL 文本前端**已明确划出范围**（决策 D15）—— 不是隐形依赖

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：图定义的输入是**结构化 JSON**，**不含** DSL 文本解析器。
- **依据**：DSL **文本语法**是产品选择（用户写什么），不是执行语义 ——
  它不阻塞 P6 的完成判据「审批恢复有测试」。
  参考架构 §5.3 局限 #5 已指出参考项目**同时存在两套工作流引擎**，
  Rust 版**只做一套**（编译式）—— 少一个前端不增加「两套路径」的维护负担。
  `crates/nexus-workflow/Cargo.toml` 已声明 `evalexpr`，脚手架本身预告了这个落点。
- **可逆性**：高 —— DSL 前端是**增量**（解析器 + 一层编译），不改动已定稿的图语义。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：展开上限（深度 8 / 节点 128 / 状态 4 MiB / 并发槽 4）**照抄**参考架构 §3.5.2。
- **依据**：这些上限在参考实现里是**防 DSL 炸弹**的既有设计，不是本仓库可以另定调的数字。
  超限映射到 `BudgetExceeded`（已定稿业务码），**不是** panic。
- **可逆性**：中 —— 上限值是常量，改动需同步测试与文档。
- **影响面**：`crates/nexus-workflow/src/graph.rs`。

### 类别：decide-and-proceed

- **决策**：定义哈希**复用** `nexus_core::canonical::canonical_hash_hex`，**不自己实现**规范化。
- **依据**：规范化规则（架构文档 §2.5.3）是**已定稿**的；
  `nexus-core` 里已有**实测过**的实现（附录 B：「规范化 JSON 键序无关、数组序敏感」）。
  再写一份 = 两份规范化 = 哈希漂移的入口。
  放在 `nexus-core` 正是因为 `nexus-workflow` 与 `nexus-execution` 互相不能依赖。
- **可逆性**：高 —— 换一个调用点。
- **影响面**：`crates/nexus-workflow/src/definition.rs`。
