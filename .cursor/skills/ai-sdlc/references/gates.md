# 门禁执行细则

判据在 `.cursor/rules/ai-sdlc-dor-gate.mdc` 与 `.cursor/rules/ai-sdlc-quality-gates.mdc`。
本文档给**可执行的步骤**。

---

## 一、DoR 门禁（S3）

### Stage A：确定性检查（先跑，永远先跑）

Stage A 不依赖 LLM、可复现。**Stage A 失败直接短路**，不发起 Stage B。

对目标任务文件 `<task>.md`：

### A1 · AC 可二元测试

```bash
grep -cE '^- \[[ x]\] #[0-9]+' <task>.md
grep -inE '改善|提升|优化|更好|更可靠|感觉' <task>.md
```

**判据**：AC 数量 1–20，每条能写成「5 分钟内可判定的通过/失败检查」，第二条命令无实质命中。

- ❌ 「改善用户体验」→ 不可判定
- ❌ 「按架构文档实现」→ 没指名具体条款与行为
- ✅ 「`nexus-llm` 的 `Cargo.toml` 不依赖 `nexus-repo`」→ 可执行

### A2 · 正文无未决标记

```bash
grep -nEi '\b(TBD|TODO|XXX|FIXME|\?\?\?)\b' <task>.md
grep -nEi "not sure|we'll figure out|decide later|up to the dev|待定|后面再定" <task>.md
```

**判据**：两条命令都无输出。**本条完全确定性，无 Stage B 环节。**

### A3 · 引用可解析

| 引用类型 | 检查 |
|---|---|
| 仓库路径 | 路径真实存在（`docs/architecture/...`、`crates/...`、`scripts/...`） |
| 章节号 `#NN` | 目标文档里确实有该章节 |
| 任务号 `NEX-nn` | `backlog/tasks/` 或 `backlog/completed/` 下有该任务文件 |
| RFC 号 | `docs/rfcs/` 下有该 RFC 文件 |

- ❌ 「像 dashboard 那个 PR」→ 裸引用
- ❌ 「§5.3」但架构文档里没有 §5.3 → 章节号漂移

### A4 · 范围有界

范围是否落在一个 PR 的 diff 规模内。**语义判定**（Stage B）。

### A5 · 受影响面被指名

```bash
grep -nE '`[a-z-]+/[A-Za-z0-9._/-]+`' <task>.md   # 文件路径
grep -nE '(nexus-[a-z]+|crates/)' <task>.md       # crate / 组件
```

**判据**：明确指出改哪些文件 / crate / 系统组件 / 路由 / 界面。

### A6 · 完成态可描述

「上线后使用者能 ___ 什么？」—— **语义判定**（Stage B）。

### A7 · 无隐形依赖

```bash
grep -nEi 'requires|depends on|after .* ships|等 .* 完成' <task>.md
```

每条命中都要能对应到 `backlog/completed/` 下的已完成任务、`backlog/tasks/` 下已登记的任务，
或 frontmatter 的 `dependencies` 字段里的 `NEX-nn`。

---

### Stage B：语义判定（仅在 Stage A 全过后）

| 门禁 | Stage B 的问题 |
|---|---|
| 1 | 「每条 AC 真的是二元可判定的吗？」 |
| 4 | 「这落在一个合理的 PR 内吗？」 |
| 5 | 「这个面指得够具体吗？」 |
| 6 | 「用户可见的终态能一句话说清吗？」 |
| 7 | 「有没有未言明的结构性假设？」 |

**自问模板**（逐字回答，答案写进判定记录）：

1. **我具体在问什么？**（把抽象门禁换成一个具体问题）
2. **答案是什么？**（是 / 否，不许「基本是」）
3. **依据是什么？**（指到任务的哪一段）

---

### 判定记录模板

```markdown
## DoR 判定

- 门禁 1（AC 可二元测试）：通过 —— 11 条 AC，均可写成命令或断言
- 门禁 2（无未决标记）：通过
- 门禁 3（引用可解析）：通过 —— §2.6.3、§3.1.1 均存在于架构文档
- 门禁 4（范围有界）：通过 —— 单 crate 改动，无跨 crate 契约变更
- 门禁 5（受影响面被指名）：通过 —— `crates/nexus-llm/src/**`
- 门禁 6（完成态可描述）：通过 —— 调用方可以用 `bound_chat_descriptor` 在本地裁剪预算
- 门禁 7（无隐形依赖）：通过 —— NEX-16 已完成

**结论：pass**
```

| 结论 | 何时用 | 下一步 |
|---|---|---|
| `pass` | 七项全过 | 开工（S4） |
| `needs-clarification` | 门禁 1/2/3/5/7 失败 | **退回 S2 澄清，不写代码** |
| `blocked` | 被上游任务或未决 OQ 阻塞 | 登记依赖，等上游 |

**三条纪律**：逐项点名不写「整体看起来可以」；失败项写清**建议的改法**；结论与记录一致。

### 澄清回路

```
门禁不通过
   ├─ 1. 任务 status 置为 `Needs Clarification`
   ├─ 2. 写清哪几项失败 + 建议的改法（指名）
   ├─ 3. 停下 —— 不写代码
   └─ 4. 补齐后重跑门禁
```

**禁止的短路**：把 AC 改成不可判定来骗过门禁；在实现里隐式解决 OQ；
用「先写着，回头补」绕过门禁 2；降级严重度来通过。

---

## 二、CI 门禁清单（S4 退出条件）

`.github/workflows/ci.yml` 强制执行，**全部 `hard-mandatory`**：

| # | 命令 | 拦截什么 |
|---|---|---|
| 1 | `bash scripts/check-lock.sh` | 成员 crate 硬编码版本号；`Cargo.lock` 未入库 |
| 2 | `bash scripts/check-deps.sh` | crate 依赖方向违规；`sqlx` 越界；依赖环 |
| 3 | `bash scripts/check-naming.sh` | 标识符含中文 |
| 4 | `bash scripts/check-backlog.sh` | 任务重复归档；完成态未标记 |
| 5 | `bash scripts/check-specs.sh` | Spec 三件套缺失；`tasks.md` ↔ backlog 漂移；契约引用不存在的测试；错误码白名单漂移 |
| 6 | `bash scripts/check-rules.sh` | 规则 frontmatter 非法；引用的仓库路径不存在；规则清单漂移 |
| 7 | `bash scripts/check-docs.sh` | 文档断链；`docs/README.md` 索引漂移；覆盖矩阵引用不存在的路径 |
| 8 | `cargo fmt --all --check` | 格式化 |
| 9 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 静态检查有任何告警 |
| 10 | `cargo test --workspace --locked` | 全量测试 |
| 11 | 集成测试数量断言（CI 内联） | 集成测试被静默跳过 |

第 11 条是本仓库特有的：集成测试判据是 `DATABASE_URL` 是否存在，
不存在时**全部跳过**但摘要显示 `passed`。CI 额外断言每个集成测试套件的**实际运行数量**达到预期下限。

**本地一键**：`bash scripts/dev.sh audit`（7 个 check 脚本）+ `bash scripts/dev.sh test`。

---

## 三、三级门禁分级

| 等级 | 行为 | 谁能覆盖 |
|---|---|---|
| `advisory` | 评估但**不拦截**；必须记录警告 | 不适用 |
| `soft-mandatory` | **拦截**；授权人可覆盖并记录理由 | `override.requiredRole` 匹配者，且必须写理由 |
| `hard-mandatory` | **拦截，不可覆盖** | **无人** |

**多门禁并存时全部通过才准入**；任一 `hard-mandatory` 失败即**整体拒绝**。

**Override 语义**：只对 `soft-mandatory` 生效；只作用于**单次评估**，不得持久化或自动续期；
每次覆盖必须产生不可变审计条目（门禁名、actor、role、理由、时间戳、受影响资源）。

---

## 四、评审门禁（S6）

### 最小评审要求

| 规则 | 等级 | 判据 |
|---|---|---|
| AI 产出代码需人工评审记录 | `hard-mandatory` | PR 上有 `approved` 的人工评审 |
| AI 作者的改动需额外一名评审 | `hard-mandatory` | AI 产出的 PR 评审人数 ≥ 2（含 1 名人工） |
| 变更文件需同步文档 | `advisory` | 记录警告，不拦截 |
| AI 产出需 provenance 标注 | `hard-mandatory` | 提交信息含 `Co-Authored-By` 尾注 |

**评审人独立性**：评审人不得是该改动的作者。

### 三类 remit

| Remit | 关注点 |
|---|---|
| **正确性** | 逻辑错误、边界条件、并发正确性、失败语义 |
| **测试** | 覆盖缺口、断言有效性、测试是否真的在测东西（变异测试） |
| **安全** | 注入、越权、敏感信息泄露、资源耗尽 |

### 评审结论模板

```markdown
## 评审结论

- 结论：approved | changes-requested | needs-human-attention
- 覆盖 AC：#1 ✅ #2 ✅ #3 ❌（见发现 2）
- 评审人独立性：<评审人 ≠ 作者，确认>
- 本轮：第 N 轮（上限 2 轮）

### 发现

| # | 严重度 | remit | 位置 | 问题 | 建议 |
|---|---|---|---|---|---|
| 1 | critical/major/minor | 正确性 | `path:line` | ... | ... |
```

**纪律**：

- 评审结论必须**指向具体 AC 编号**。
- 评审者**只评审不修复** —— 一旦开始改代码，独立性就没了，且改动绕过了 DoR 与门禁。
- `changes-requested` → 回 S4，**最多 2 轮**；第 2 轮仍有 critical/major → **needs-human-attention**。
- **契约级问题不得就地修复** → 升级，修复后**必须重跑评审**。

### Provenance 记录

| 字段 | 必填 | 本仓库载体 |
|---|---|---|
| `model` | 是 | 提交信息 `Co-Authored-By` 尾注中的模型名 |
| `tool` | 是 | `Co-Authored-By` 中的工具标识 |
| `timestamp` | 是 | 提交时间戳（git 记录） |
| `humanReviewer` | 否 | PR 评审记录 |
| `reviewDecision` | 否 | PR 评审结论 |

**最低要求**：模型标识 + 工具标识 + 时间戳。
**Provenance 记录一旦创建即不可变。**

---

## 五、三层验证（S5）

| 层次 | 手段 | 用途 |
|---|---|---|
| 类型层 | `trybuild` 负向测试 | 断言「写错的代码**编译不过**」 |
| 结构层 | 架构测试 / 依赖方向校验 | 依赖方向、无环、`sqlx` 收敛、源码扫描 |
| 行为层 | 集成 / 不变量测试 | 语义不变量（如 fencing 拒绝僵尸写入） |

**结构层断言的实践要点**：

- 扫描源码的断言**必须剥离注释** —— 文档注释里解释「为什么不用 X」是正常文字，不是实际使用。
- 价值在于拦截**静默的行为退化**（编译通过但语义变了）。
- **变异测试**：当一条不变量「看起来显然」时，用变异测试验证断言真的有效 ——
  去掉该检查后，对应测试必须变红。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `.cursor/rules/ai-sdlc-dor-gate.mdc` | **DoR 判据** |
| `.cursor/rules/ai-sdlc-quality-gates.mdc` | **门禁分级与评审判据** |
| `docs/ai-sdlc/dor-playbook.md` | DoR 手册（叙事版） |
| `docs/ai-sdlc/review-playbook.md` | 评审手册（叙事版） |
| `.github/workflows/ci.yml` | 门禁的可执行实现 |
