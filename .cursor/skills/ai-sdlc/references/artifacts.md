# 产出物模板

判据在 `.cursor/rules/ai-sdlc-artifacts.mdc`。本文档给**可直接复制的模板**。

---

## Tier 3 · Task

载体：`backlog/tasks/NEX-<n> - <slug>.md`

文件名 ASCII、空格用 `-` 连接、slug 小写短横线；文件名与 frontmatter `id` **必须一致**。

```markdown
---
id: NEX-101
title: "feat(core): 增加取消语义类型状态"
status: To Do
assignee: []
labels: [core, typestate]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.1"
---

## Description

<问题描述：谁需要什么，为什么现在做>

## Acceptance Criteria

- [ ] #1 <可二元判定的验收条件>
- [ ] #2 <可二元判定的验收条件>

## DoR 判定

<七项门禁的逐项判定与结论>
```

**字段语义**：对齐上游 `spec/schemas/backlog-task.v1.schema.json`。

**`status` 取值**：`To Do` / `In Progress` / `Done` / `Needs Clarification`。

**AC 必须逐条编号**（`- [ ] #N`）—— 编号是评审阶段的引用锚点。
勾选框是**观测**：只在**实际验证通过后**才勾。

**`references` 每条必须可解析** —— 文件路径存在、章节号存在、任务号可定位。

---

## Tier 2 · Spec

载体：`docs/specs/<feature>/`

```
docs/specs/<feature>/
├── spec.md      # 契约：范围、红线、核心契约、已定稿决策
├── tasks.md     # 任务清单：T-NNN ↔ backlog 任务一一对应
└── contracts/   # 机器可读的契约
```

> `plan.md` 是上游四件套之一，但**本仓库不强制** ——
> 实施顺序与依赖已由 `tasks.md` 的 `T-NNN` + `backlog/` 的 `dependencies` 表达。
> `scripts/check-specs.sh` 强制的是**三件套**。

### `spec.md` 小节

| 小节 | 内容 |
|---|---|
| 范围 | 在范围内 / **不在范围内**（明确划界比什么都重要） |
| 红线 | 不可让步的契约（错误码、依赖方向、类型形态） |
| 核心契约 | 管道顺序、边界语义、错误分类 |
| 已定稿决策 | 决策 + 依据 + 类别 (a)/(b)/(c) |
| 完成判据 | 拆成可验证子目标 + **防伪要求** |

### `tasks.md` 布局

遵循 [GitHub Spec Kit](https://github.com/github/spec-kit) 的 `v0.8-headings`：

```markdown
### T-001 — <title>

正文（落到 Task 层就是 `## Description`）。

- AC: <可二元判定的验收条件>
- AC: <...>
```

`T-NNN` 与 Task 层的 `NEX-nn` **一一对应** —— 由 `check-specs.sh` 规则 2 双向校验。

### `contracts/`

| 文件 | 作用 |
|---|---|
| `invariants.json` | 每条不变量 + 验证方式（测试路径 / 测试名） |
| `error-codes.json` | 本阶段允许触碰的业务码白名单 |

`invariants.json` 的 `verified_by` 指向**真实的测试** —— 门禁会核对该测试名在对应文件里确实存在。
声称「某测试验证了这条不变量」但那个测试不存在时，契约就是**空头支票**。

`contracts/` 与架构文档 §6.3 冲突时**以架构文档为准** —— 契约用于**细化**而非**覆盖**。

---

## Tier 1 · RFC

载体：`docs/rfcs/NEX-RFC-<n> - <slug>.md`

```markdown
---
id: NEX-RFC-3
title: "..."
status: Draft | Ready for Review | Accepted | Superseded
created: YYYY-MM-DD
requires: [NEX-RFC-1, ...]
---

## 1. Summary        <一句话结论>
## 2. Motivation     <为什么要做这个决策>
## 3. Options        <候选方案，每个含取舍>
## 4. Recommendation  <建议 + 反面论证>
## 5. Consequences   <采纳/不采纳的后果>
## 6. Open Questions <未决问题 —— 只能由操作者解决>
```

**OQ 禁令**：`## 6. Open Questions` 段落**不得**由代理自行消解。
不得添加 `**Resolution:**` / `RESOLVED:` 标记。

**状态推进**：`Draft → Ready for Review → Accepted` 只由**操作者**推进。
代理最多推进到 `Ready for Review`。

---

## 归档纪律

任务**合入 `main` 之后**，在同一提交内：

```bash
git mv "backlog/tasks/NEX-101 - feat-core-xxx.md" "backlog/completed/"
```

并把 frontmatter `status` 改为完成态，同时更新所有引用了该路径的位置。

编辑器可能在旧路径重建同名文件 —— 出现即 `git rm` 旧路径那一份
（`check-backlog.sh` 规则 1：同一任务不得同时存在于两个目录）。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `backlog/README.md` | 任务文件格式与生命周期 |
| `docs/specs/README.md` | Spec 层产物说明 |
| `docs/rfcs/README.md` | RFC 层产物说明 |
| `.cursor/rules/ai-sdlc-artifacts.mdc` | **判据** |
