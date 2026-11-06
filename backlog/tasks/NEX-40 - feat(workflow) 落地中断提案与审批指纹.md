---
id: NEX-40
title: "feat(workflow): 落地中断提案与审批指纹"
status: To Do
assignee: []
labels: [workflow, approval, security]
priority: high
dependencies:
  - NEX-38
  - NEX-39
references:
  - "docs/specs/p6/spec.md §3.6、§4.4"
  - "docs/specs/p6/tasks.md T-004"
  - "docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.6、§3.5.3"
---

## Description

**问题**：审批的语义核心是「**批的是哪一次调用、哪些参数**」。
参考架构 §2.6 的取舍说明把这条线标为「**该看什么**」：
`ApprovalInfo.ArgsHash` 把审批绑定成不可变指纹，**参数一变审批自动失效**。

没有这层，指纹就只是一串可被篡改的字符串 —— 「审批」退化成「一个可以绕过的人肉开关」。

**修法**：落地 `Interrupt`（`path` / `args_hash` / `prompt` / `resume_key`），
`args_hash = sha256({path, prompt, parameters})`；
并用**类型区分**（typestate）让审批能力**不能**从普通输入流进来。

## 范围边界

### 做

- `nexus-workflow/src/interrupt.rs`：`Interrupt` + `args_hash` 计算
- `nexus-workflow/src/approval.rs`：未授权 / 已授权作用域的**类型区分**
- `Human` 节点与**危险**工具节点产生中断提案

### 不做

| 项 | 理由 |
|---|---|
| 审批的**持久化**（挂起 / 决策 / 恢复） | 属 NEX-44 |
| 审批的 HTTP 端点 | 决策 D17（OQ-6）—— 属 `nexus-service` |
| 危险工具的**授权策略** | 属 P8（`nexus-tool`）；P6 只交付**中断提案** |

## Acceptance Criteria

- [ ] #1 `crates/nexus-workflow/src/interrupt.rs` 定义 `Interrupt` —— 含 `path` / `args_hash` / `prompt` / `resume_key`
- [ ] #2 `args_hash = sha256({path, prompt, parameters})` —— 三个输入**全部**参与
- [ ] #3 **参数变化 → 指纹不同** —— 同一 `path` / `prompt`、不同 `parameters` → `args_hash` **不同**
- [ ] #4 **同一** `path` / `prompt` / `parameters` → `args_hash` **相同**（指纹是确定性的，不是随机的）
- [ ] #5 节点类型 `Human` → 产生中断提案；工具节点且**危险** → 产生中断提案
- [ ] #6 中断提案经 `NodeError::Interrupt` **从节点返回**（不是由引擎旁路生成）
- [ ] #7 恢复时 `decision.args_hash != proposal.args_hash` → **拒绝恢复**（参数已变）
- [ ] #8 审批能力**不走 context / 不走参数传递** —— 用**类型区分**（typestate）：未授权作用域与已授权作用域是**不同类型**，已授权类型的构造函数**模块私有**
- [ ] #9 `crates/nexus-workflow` 的源码**不含** `context.` / `ctx.set(` 形态的审批能力传递（结构层断言）
- [ ] #10 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

10 条 AC 全部可二元判定 —— 每条都指明「类型存在」「两个哈希相等 / 不等」
「结构层源码断言」或「命令退出码」。

### #2 无未决标记

正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` / `not sure` / `decide later` / `up to the dev`。

### #3 引用可解析

- `docs/specs/p6/spec.md §3.6、§4.4` —— 存在
- `docs/specs/p6/tasks.md T-004` —— 存在
- `docs/architecture/2026-11-04-nexusagent-eino-rust-reference-architecture.md §2.6、§3.5.3` —— 存在
- `crates/nexus-workflow/src/` —— 存在

### #4 范围有界

**单 crate**（`nexus-workflow`）的**两个**新模块 + 测试。
纯内存计算，**无**数据库、**不新增**依赖方向、**不新增**业务码。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-workflow/src/interrupt.rs` | **新增** |
| `crates/nexus-workflow/src/approval.rs` | **新增** |
| `crates/nexus-workflow/src/node.rs` | **修改**（`Human` / 危险工具节点产生中断） |
| `crates/nexus-workflow/src/lib.rs` | **修改**（模块声明） |
| `crates/nexus-workflow/tests/architecture.rs` | **修改**（新增审批能力隔离断言） |
| `crates/nexus-workflow/Cargo.toml` | **不修改**（`sha2` 需确认 —— 见 #7） |

### #6 完成态可描述

节点可以在需要人工确认时提出一个**不可变指纹**的中断提案；
恢复时引擎能判断「批的是不是同一次调用」；
未授权路径**拿不到**已授权作用域的类型 —— 越权路径**编译不过**。

### #7 无隐形依赖

- 前置：NEX-38（冻结定义与图编译）、NEX-39（节点执行契约 —— `NodeError::Interrupt` 由节点返回）——
  均已登记于 `backlog/tasks/`
- SHA-256 的实现来源**必须**在开工前确认：优先复用 workspace 里已有的实现
  （`nexus-core` 的规范化哈希内部已用 SHA-256，见 `crates/nexus-core/src/canonical.rs`）；
  若需要新依赖，**必须**走根 manifest 并同步 `check-deps.sh` 规则 7 —— **这是已指名的动作**
- 危险工具的判定来源（`nexus-tool` 的 `DangerLevel`）属 P8 —— P6 只交付
  「工具节点可被标记为危险」这个**接缝**，具体判定由上层注入 —— **不构成隐形依赖**

### 结论

**pass** —— 七项全过，可以开工。

## 决策记录

### 类别：decide-and-proceed

- **决策**：审批能力用**类型区分**（typestate）隔离，不用参数传递、不用 context。
- **依据**：参考架构 §2.6 的取舍说明原文 ——
  「Go 版靠注释说明『审批能力不来自普通 Agent 输入』，靠自觉；
  Rust 版可以让越权路径**编译不过**。这是 Rust 移植的一个真实收益。」
  Rust 没有 context values —— **必须**显式传递；用类型让「已授权」不可伪造。
- **可逆性**：中 —— 公开类型是 API，但改动的调用点只在 P6 内。
- **影响面**：`crates/nexus-workflow/src/approval.rs`。

### 类别：decide-and-proceed

- **决策**：`args_hash` 覆盖 **path + prompt + parameters** 三项。
- **依据**：参考架构 §3.5.3 明写 `ArgsHash = sha256({path,prompt,parameters})`，
  且「**参数变化会导致审批自动失效**」。
  少算一项 = 改了 `parameters` 审批仍然有效 = 人工审批形同虚设。
- **可逆性**：中 —— 指纹算法变更会让既有审批失效。
- **影响面**：`crates/nexus-workflow/src/interrupt.rs`。
