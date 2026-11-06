---
id: NEX-51
title: "docs(skill): 新增 nexus-rust 技能，把 Rust 写法与门禁固化为可执行手册"
status: To Do
assignee: []
labels: [docs, skill, rust]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.4、§5.2、§5.3、§6.3"
  - ".cursor/rules/rust-code-standards.mdc"
  - ".cursor/skills/ai-sdlc/SKILL.md"
  - "crates/nexus-core/tests/support/token_scan.rs"
---

## Description

**问题**：`.cursor/rules/rust-code-standards.mdc` 规定了 Rust 写法，但它是**规则**，
按 `globs: ["**/*.rs"]` 挂载，代理读它时拿到的是**判据**，不是**动作顺序**。

仓库现在缺一份「在这个仓库里写 Rust 该按什么顺序做」的执行手册：

- 改哪个 crate、依赖方向怎么查（`scripts/check-deps.sh` 的 `deps_of` 是 crate 图的可执行版本）
- 先命名数据形状，再选承载结构（仓库已有四种可复用形状）
- 失败语义怎么分层、怎么给重试分类
- 新代码落哪一层验证，三层各自的手段与现存范本
- 门禁命令与被门禁拒绝时的正确动作

**为什么是技能而不是规则**：规则是**判据**，技能是**执行器**。
`.cursor/skills/ai-sdlc/SKILL.md` 已经承担「推进一件工作走完 S0-S9」的同类角色，
Rust 版本补上「写代码」这一段，两者在 `AGENTS.md` 的权威顺序里各就各位。

**范围纪律**：技能**不重新定义**任何条款，只**指向**既有来源。
错误码值、事件名、ID 底层类型、crate 边界仍以架构文档 §6.3 为准。

## 范围边界

### 做

- 新增 `.cursor/skills/nexus-rust/SKILL.md`

### 不做

| 项 | 理由 |
|---|---|
| 改任何 **Rust 源码** | 本任务只新增一份文档，不动生产代码 |
| 改 `.cursor/rules/rust-code-standards.mdc` | 该文件是判据来源，技能**引用**它，不**复制**它 |
| 改架构文档 | 已定稿条款不需要修订 |
| 改 `scripts/` 下的门禁脚本 | 门禁**已经**能拦住违规，技能只是让人先知道 |

## Acceptance Criteria

- [ ] #1 `.cursor/skills/nexus-rust/SKILL.md` 存在，frontmatter 含 `name` 与 `description`
- [ ] #2 技能里引用的**每一个**仓库路径都**真实存在**（逐条核对）
- [ ] #3 技能**不重复**定义错误码值、事件名清单、ID 底层类型 —— 这些只**指向**架构文档
- [ ] #4 技能覆盖**数据形状 → 失败语义 → crate 归属 → 三层验证 → 门禁 → 红线**六段
- [ ] #5 三层验证段落给出**可复制的命令**与**现存范本路径**
- [ ] #6 技能里的结构层扫描命令与 `scripts/check-deps.sh` / `scripts/check-specs.sh` 实际使用的**同一条**
- [ ] #7 标识符与标题**不含中文**（正文与表格里的中文说明除外）
- [ ] #8 `bash scripts/check-backlog.sh` 通过（本任务文件的 DoR 判定段落存在）
- [ ] #9 `bash scripts/check-docs.sh` 通过
- [ ] #10 `bash scripts/check-rules.sh` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 10 条 AC，均可由「文件存在 + 路径可解析 + 脚本退出码」判定
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 架构文档 §4.4 / §5.2 / §5.3 / §6.3 存在；
  `.cursor/rules/rust-code-standards.mdc`、`.cursor/skills/ai-sdlc/SKILL.md`、
  `crates/nexus-core/tests/support/token_scan.rs` 均存在
- 门禁 4（范围有界）：通过 —— 单文件新增，不动生产代码，不动门禁脚本
- 门禁 5（受影响面被指名）：通过 —— `.cursor/skills/nexus-rust/SKILL.md`
- 门禁 6（完成态可描述）：通过 —— 代理在写 Rust 前有一份按顺序执行的手册，
  能直接照着跑门禁命令与三层验证
- 门禁 7（无隐形依赖）：通过 —— 无前置任务；技能内容全部来自已落盘的规则、
  架构文档与源码

**结论：pass**
