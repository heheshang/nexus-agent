---
name: ai-sdlc
description: "推进一件工作走完 AI-SDLC 的 S0-S9 生命周期：需求捕获、分层判定、产出物编写、DoR 门禁、实现、三层验证、评审、合并交付、度量校准。Use when creating or advancing an RFC, Spec, or backlog task, running a DoR gate, doing review, committing, or archiving a task under the AI-SDLC workflow, gates, decision discipline, and artifact rules. Triggers on RFC, Spec, backlog 任务, NEX-, DoR, 门禁, 评审, 决策记录, OQ, provenance, AI-SDLC 流程."
---

# AI-SDLC 生命周期执行器

在本仓库推进一件工作，从诉求到交付，**不跳阶段**。

## 权威顺序

本技能**不定义判据**，只负责**执行**。判据按此顺序取：

1. `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` —— 架构与契约的唯一权威
2. `.cursor/rules/ai-sdlc-*.mdc` —— 流程判据（S0–S9 各阶段规范）
3. `.cursor/rules/rust-code-standards.mdc` —— Rust 代码规范
4. `AGENTS.md` —— 常驻指令入口

冲突时**以序号小的为准**，并且**不要静默选一个** —— 指出冲突并请求决策。

技能与规则文件冲突时，以规则文件为准，并**修正本技能**。

## 何时用 / 何时不用

| 用 | 不用 |
|---|---|
| 用户要求「按 AI-SDLC 流程」推进一件工作 | 纯问答、纯调研、无产出的解释性请求 |
| 需要创建 RFC / Spec / backlog 任务 | 一次性脚本、临时实验（不入 backlog） |
| 需要过 DoR、做评审、提交、归档任务 | 单行 bug 修复且已有对应任务号（直接走 S4 起的门禁即可） |

**不扩大范围**：只读 / 审计任务发现后续工作 → 只提建议并停下，不擅自登记任务或开 PR。

---

## 主循环

```
S0 需求捕获 → S1 分层判定 → S2 产出物编写 → S3 DoR 门禁 ══╗ 不过 → 退回 S2
                                                        ↓ pass
                                                      S4 实现 → S5 三层验证
                                                        ↓
                                                      S6 评审 ══╗ changes-requested → 回 S4（≤2 轮）
                                                        ↓ approved
                                                      S7 合并交付 → S8 部署运维 → S9 度量校准
```

**两道硬边界**（不可绕过）：

- **S3 → S4**：DoR 判定必须**先于**任何代码写入。
- **S6 → S7**：评审必须**先于**合并。

阶段表（进入条件 / 产出物 / 退出条件 / 失败处理）见
`.cursor/rules/ai-sdlc-workflow.mdc` §2 —— 执行前先读它。

---

## 各阶段执行要点

### S0 需求捕获

把诉求归类到某一层（S1 的判据）。**信息不足 → 提问**，勿擅自开工。

### S1 分层判定

判据不是「大小」，而是**分歧的存在**：

| 现象 | 层 | 产物 |
|---|---|---|
| 存在合理工程师会就正确做法产生分歧 | Tier 1 · RFC | `docs/rfcs/NEX-RFC-<n> - <slug>.md` |
| 一个特性能拆成多个任务 | Tier 2 · Spec | `docs/specs/<feature>/{spec.md,tasks.md,contracts/}` |
| 单个 PR 规模 | Tier 3 · Task | `backlog/tasks/NEX-<n> - <slug>.md` |

层级**独立可用**，不强制向上遍历。但**任何工作都不能跳过 Task 层**。

**复杂度打分**（`ai-sdlc-autonomy.mdc` §2）：取五个维度的最高分。
触及错误码 / 事件 / ID 类型 / crate 依赖方向 → **一律不得评为「低」**，必须走人工评审路径。

### S2 产出物编写

模板见 [references/artifacts.md](references/artifacts.md)。

**判据**：这份产出物里的每一句话，别人能不能独立验证？

- 写代码前先读架构文档相关章节，确认依赖条款**已定稿**（§6.3 清单）。
- 条款未定稿，或本次要**改**已定稿条款 → **停止**，走 RFC / 升级。

### S3 DoR 门禁

七项门禁，**先跑 Stage A（确定性）再跑 Stage B（语义）**。细则见
[references/gates.md](references/gates.md)。

**判定记录必须落到任务文件里**，逐项点名，结论只有三种：`pass` / `needs-clarification` / `blocked`。

不通过 → **退回 S2 澄清，不写代码**。

### S4 实现

先答**契约影响五问**（错误码 / 事件名与负载 / ID 底层类型 / crate 依赖方向 / 对外响应形状），
「否」也要显式写出来。硬性禁止见 `.cursor/rules/ai-sdlc-implementation.mdc` §3。

**失败语义必须显式建模**：`Retryable` / `UnknownOutcome` / `Terminal`。
超时默认映射到 `UnknownOutcome` —— **禁止**自动重试。

### S5 三层验证

每条新代码**至少落在一层**：类型层（`trybuild`）/ 结构层（架构扫描）/ 行为层（不变量测试）。

结构层扫描源码的断言**必须剥离注释**。

### S6 评审

三类 remit：正确性 / 测试 / 安全。规则与模板见 [references/gates.md](references/gates.md)。

- AI 产出的 PR 评审人数 **≥ 2**（含 1 名人工）。
- 评审结论必须**指向具体 AC 编号**。
- `changes-requested` → 回 S4，**最多 2 轮**；第 2 轮仍有 critical/major → 交人工。
- **契约级问题不得就地修复** → 升级，修复后**必须重跑评审**。

### S7 合并与交付

- **始终 rebase** 到 `main`，**绝不把 `main` merge 进来**。
- 提交信息 Conventional Commits，标题末尾带**真实存在**的任务号 `(NEX-101)`，带 `Co-Authored-By` 尾注（provenance）。
- 任务移入 `backlog/completed/`（用 `git mv`），并在**同一提交**内更新引用了该路径的位置。

### S8 / S9

S8：变更可观测、可回滚；配置项有 `NX_` 映射。
S9：指标口径完整，**校准产物是规则修订，不是单次豁免**。

---

## 决策纪律

**权威来自仓库**：`main` 上的决策记录才构成授权。

决策分三类（细则见 [references/decisions.md](references/decisions.md)）：

| 类 | 判据 | 代理怎么做 |
|---|---|---|
| (a) decide-and-proceed | 可低成本回滚、影响小、不触碰信任链 | 自行判定，**记录决策**，立即应用 |
| (b) timeboxed | 难回滚、影响大、或**削弱**治理控制 | 记录决策 + 时间盒 + 无人干预时的回退方案 |
| (c) operator-only | 法律 / 金钱 / 凭据 / 只能以操作者身份执行 | **绝不自行决定**，只搁置该任务，其余继续 |

**OQ 三条硬禁令**（`ai-sdlc-decision-authority.mdc` §4.1）：

1. 不得添加 `**Resolution:**` / `RESOLVED:` 标记。
2. 不得用设计决策替换开放问题。
3. 不得用代码隐式解决开放问题。

**Away rule**：操作者不在场时**不发起阻塞性提问** —— 改用显式假设推进，并写出假设。
「沉默的假设是最贵的假设。」

**Velocity**：任何拒绝都必须给出**代理可自行执行的下一步**，不得以拒绝终结任务。

---

## 门禁命令

提交前全绿（本地 `bash scripts/dev.sh audit` + 编译三连）：

```bash
bash scripts/check-lock.sh      # 依赖锁定纪律
bash scripts/check-deps.sh      # crate 依赖方向铁律
bash scripts/check-naming.sh    # 标识符不得含中文
bash scripts/check-backlog.sh   # backlog 组织纪律
bash scripts/check-specs.sh     # Spec 层组织纪律
bash scripts/check-rules.sh     # 规则文件一致性
bash scripts/check-docs.sh      # 文档层一致性
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是独立 workspace：`cd examples && cargo test --workspace`。

被门禁拒绝时的正确动作是**补齐 sanctioned path**（登记任务、修产出物），
**不是**绕过 hook、改门禁配置、或本地跳过检查后声称通过。

---

## 硬性红线

- 不得绕过 crate 依赖方向（`sqlx` 只允许在 `nexus-repo` / `nexus-infra` / `nexus-execution`）。
- 不得改动错误码值 / 事件名 / ID 底层类型。
- 不得让内部错误原因（`cause`）进入对外响应。
- 不得把 `UnknownOutcome` 当可重试处理。
- 不得对 enum 变体用 `#[serde(flatten)]`；不得 derive `Serialize` 给 `ErrorCode`。
- 不得引入 `unsafe`；release profile 必须保持 `panic = "unwind"`。
- 不得在 `.await` 期间持有 `std::sync::MutexGuard`。
- 不得把凭据 / 密钥写入仓库、日志或错误消息。
- 不得 `git push --force`（rebase 后只用 `--force-with-lease`）；不得删分支；
  不得 `git reset --hard` / `git checkout -- .` / `git restore .`。

---

## 状态跟踪

用 TodoWrite 把当前工作的阶段显式列出来，每完成一个阶段就更新。
让用户随时能看到「现在在哪一阶段、下一步是什么」。

---

## 附加资源

- 产出物模板（Task / Spec / RFC）—— [references/artifacts.md](references/artifacts.md)
- 门禁执行细则（DoR Stage A/B、CI 清单、评审模板）—— [references/gates.md](references/gates.md)
- 决策纪律（分类判据、升级模板、五步法）—— [references/decisions.md](references/decisions.md)
- 流程手册（叙事版，理解设计意图）—— `docs/operations/ai-sdlc-workflow-handbook.md`
- 术语表 —— `docs/ai-sdlc/glossary.md`
