# RFC · Tier 1 决策文档

本目录承载 AI-SDLC 的 **Tier 1 产出**（见 `.cursor/rules/ai-sdlc-artifacts.mdc` §1）。

## 什么时候用 RFC

只有满足**任一**条件时才写 RFC：

- 存在**合理工程师会就正确做法产生分歧**的开放问题。
- 影响是**跨特性**的。
- 决策**难以回滚**，且做错的代价高。
- 决策触及架构文档 §6.3 的**已定稿条款**。

否则直接进 Task 层（或 Spec 层）。

## 编号与文件名

```
docs/rfcs/NEX-RFC-<n> - <slug>.md
```

- `NEX-RFC-` 前缀与任务号 `NEX-` 同族，避免与任务号混淆。
- 文件名 ASCII，空格用 `-` 连接。

## 最小结构

见 `.cursor/rules/ai-sdlc-artifacts.mdc` §3。

## 状态推进

`Draft → Ready for Review → Accepted` **只由操作者推进**。
代理最多推进到 `Ready for Review`。

## OQ 禁令

`## 6. Open Questions` 段落的解决权**专属操作者**。
不得添加 `**Resolution:**` / `RESOLVED:` 标记，不得用设计决策替换开放问题，
不得用代码隐式解决。

## 当前状态

本目录暂无 RFC。当出现第一个 Tier 1 决策时在此登记。
