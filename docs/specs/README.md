# Spec 层产物

本目录承载 AI-SDLC **三层模型**的 Tier 2 产出（见 `.cursor/rules/ai-sdlc-artifacts.mdc` §1）。

## 三层模型在本仓库的落法

| 层 | 产物 | 本仓库载体 | 何时用 |
|---|---|---|---|
| **Tier 1 · RFC** | 决策文档（问题 / 选项 / 建议 / 后果） | `docs/rfcs/` | 跨特性、架构级、有争议的开放问题 |
| **Tier 2 · Spec** | `spec.md` + `plan.md` + `tasks.md` + `contracts/` | **本目录** | 一个特性拆成多个任务 |
| **Tier 3 · Task** | 通过 DoR 门禁的待办任务 | `backlog/tasks/` | 单个 PR 规模的交付物 |

- 层级**独立可用**，不强制向上遍历 —— 单个 PR 的工作可以直接落 Task 层。
- 但**任何工作都不能跳过 Task 层**。
- Task 层**不能跳过** Spec 层的情形：**一个特性拆成多个任务**时，
  必须先有 Spec，否则「任务拆解」没有可追溯的契约。

## 目录结构

```
docs/specs/<spec-slug>/
├── spec.md      # 契约：范围、红线、核心契约、已定稿决策
├── tasks.md     # 逐任务 AC（spec-kit v0.8-headings 布局）
└── contracts/   # 机器可读的不变量与错误码白名单
```

## `spec.md` 写什么

| 小节 | 内容 |
|---|---|
| 范围 | 在范围内 / 不在范围内（**明确划界**比什么都重要） |
| 红线 | 不可让步的契约（错误码、依赖方向、类型形态） |
| 核心契约 | 管道顺序、边界语义、错误分类 |
| 已定稿决策 | 决策 + 依据 + 类别（(a)/(b)/(c)） |
| 完成判据 | 拆成可验证的子目标 + **防伪要求** |

## `tasks.md` 的布局

遵循 [GitHub Spec Kit](https://github.com/github/spec-kit) 的 `v0.8-headings`：

```markdown
### T-001 — <title>

正文（落到 Task 层就是 `## Description`）。

- AC: <可二元判定的验收条件>
- AC: <...>
```

`taskId`（`T-NNN`）与 Task 层的 `NEX-nn` **一一对应** ——
由 `scripts/check-specs.sh` 规则 2 双向校验。

## `contracts/` 写什么

**机器可读的契约** —— 让契约可以被门禁校验，而不是只停留在散文。

| 文件 | 作用 |
|---|---|
| `invariants.json` | 每条不变量 + 它的验证方式（测试路径 / 测试名） |
| `error-codes.json` | 本阶段允许触碰的业务码白名单 |

`invariants.json` 的 `verified_by` 指向**真实的测试** ——
门禁会核对该测试名在对应文件里**确实存在**。
声称「某测试验证了这条不变量」但那个测试根本不存在时，契约就是**空头支票**。

## 门禁

```bash
bash scripts/check-specs.sh
```

| 规则 | 拦什么 |
|---|---|
| 1. Spec 三件套齐全 | 只做 Task 层、跳过 Spec 层 |
| 2. `tasks.md` ↔ backlog 双向对应 | 文档漂移 |
| 3. 契约引用的测试必须存在 | 「空头支票」契约 |
| 4. 错误码白名单与 `nexus-core` 一致 | §6.3 已定稿条款漂移 |
| 5. `nexus-llm` 只用白名单内的码 | 偷偷新增业务码 |

> **门禁本身必须被验证过会红** —— 沿用 NEX-14 的教训。
> 本门禁已用注入故障验证：篡改码值 / 引用不存在的测试 / 引用不存在的 backlog 任务。

## 现有 Spec

| Spec | 阶段 | 状态 |
|---|---|---|
| [`p3/`](p3/spec.md) | P3 LLM 网关 | 🚧 进行中（NEX-16/17/18 已完成） |
