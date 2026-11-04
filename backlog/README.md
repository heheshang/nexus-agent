# Backlog · DoR 任务载体

本目录承载 AI-SDLC 的 **Task 层**产出（见 `.cursor/rules/ai-sdlc-workflow.mdc` §1、§2.1）。

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
references: []
---

## Description

<问题描述：谁需要什么，为什么现在做>

## Acceptance Criteria

- [ ] #1 <可二元判定的验收条件>
- [ ] #2 <可二元判定的验收条件>
```

## 门禁

任务进入实现前必须通过 DoR 七项检查（`ai-sdlc-workflow.mdc` §2）。任一项不通过即退回澄清，不开工。

## 生命周期

1. 任务落盘 `tasks/`，文件名与 frontmatter `id` 一致。
2. 开发：Conventional Commits，标题末尾带任务号 `(... (NEX-101))`。
3. 交付后移入 `completed/`，并在**同一提交**内更新所有引用了该路径的位置。
