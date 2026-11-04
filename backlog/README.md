# Backlog · DoR 任务载体

本目录承载 AI-SDLC 的 **Task 层**产出（见 `.cursor/rules/ai-sdlc-artifacts.mdc` §1、§2）。

## 目录

| 目录 | 含义 |
|---|---|
| `tasks/` | 进行中的任务 |
| `completed/` | 已交付并合入的任务 |

## 文件命名

```
backlog/tasks/<NEX-<n>> - <slug>.md
```

- 前缀 `NEX-`，与 AI-SDLC 的 `AISDLC-<n>` 同构。
- 文件名为 ASCII；空格用 `-` 连接；slug 用小写短横线。
- 文件名与 frontmatter `id` 必须一致。

## 文件格式

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

<七项门禁的逐项判定与结论 —— 见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §5>
```

字段语义对齐 AI-SDLC 的 `spec/schemas/backlog-task.v1.schema.json`。
`status` 取值：`To Do` / `In Progress` / `Done` / `Needs Clarification`。

**AC 必须逐条编号**（`- [ ] #N`）—— 编号是评审阶段的引用锚点。

## 门禁

任务进入实现前必须通过 DoR 七项检查（`.cursor/rules/ai-sdlc-dor-gate.mdc`）。
任一项不通过即退回澄清，不开工。**判定记录必须落到任务文件里**，
否则下次接手的人无法复核。

## 生命周期

1. 任务落盘 `tasks/`，文件名与 frontmatter `id` 一致。
2. 开发：Conventional Commits，标题末尾带任务号 `(... (NEX-101))`，
   带 `Co-Authored-By` 尾注（provenance）。
3. 交付后移入 `completed/`，并在**同一提交**内更新所有引用了该路径的位置。

## 纪律

`bash scripts/check-backlog.sh` 校验：

- 同一任务**不得**同时存在于 `tasks/` 与 `completed/`
  （归档用 `git mv`；编辑器可能在旧路径重建同名文件 —— 见 NEX-7 的教训）。
- `completed/` 下的任务 frontmatter `status` 必须标记完成。
