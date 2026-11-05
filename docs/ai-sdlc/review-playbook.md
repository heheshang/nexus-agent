# 评审操作手册

> **判据在 `.cursor/rules/ai-sdlc-quality-gates.mdc`**。本手册讲**怎么组织一次评审**、
> **怎么写结论**、**什么时候停**。两者冲突时以规则文件为准。

---

## 0. 评审在流程中的位置

```
S4 实现 → S5 三层验证 → S6 评审 ══╗ changes-requested → 回 S4（最多 2 轮）
                            ↓ 通过
                          S7 合并与交付
```

**S6 → S7 是第二道硬边界** —— 它防的是「没人看过的变更进入 `main`」。

评审**不是**实现的一部分。评审者**只评审不修复** —— 一旦开始改代码，独立性就没了，
且改动绕过了 DoR 与门禁。

---

## 1. 三类 remit

| Remit | 关注点 | 提问方式 |
|---|---|---|
| **正确性** | 逻辑错误、边界条件、并发正确性、失败语义 | 「这段逻辑在什么输入下会给出错误结果？」 |
| **测试** | 覆盖缺口、断言有效性、测试是否真的在测东西 | 「删掉这条断言，测试会变红吗？」 |
| **安全** | 注入、越权、敏感信息泄露、资源耗尽 | 「不可信输入能到达这里吗？到达后能造成什么？」 |

### 正确性 remit 的必查清单

| 项 | 判据 |
|---|---|
| 边界值 | 空集合 / 单元素 / 满容量 / 刚好等于上限 / 超出 1 字节 |
| 失败语义 | `UnknownOutcome` 是否被当成可重试处理（**不允许**） |
| 取消 | 取消是否在正确的时点被响应；取消是否被计入统计样本 |
| 并发 | `.await` 期间是否持有 `std::sync::MutexGuard`（**不允许**） |
| 幂等 | 重试路径是否幂等，或显式声明不幂等 |
| 契约 | 是否触及错误码值 / 事件名 / ID 底层类型 / 对外响应形状 |

### 测试 remit 的必查清单

| 项 | 判据 |
|---|---|
| 覆盖层次 | 新代码是否至少落在一层（类型层 / 结构层 / 行为层） |
| 断言有效性 | 「看起来显然」的不变量是否有变异测试背书 |
| 剥离注释 | 结构层扫描源码的断言是否**剥离了注释**（NEX-18 的教训） |
| 集成测试真跑 | 集成测试的判据是 `DATABASE_URL` 存在与否 —— 不存在时**全部跳过但显示 passed**。CI 额外断言各套件的实际运行数量 |
| 空头支票 | 契约里声称的验证测试**确实存在**（`scripts/check-specs.sh` 规则 3） |

### 安全 remit 的必查清单

| 项 | 判据 |
|---|---|
| 凭据 | 是否有可能把凭据写进仓库 / 日志 / 错误消息 / 快照期望输出 |
| 注入 | SQL / 路径 / 命令是否参数化 |
| 越权 | 租户边界、generation fencing 是否被绕过 |
| 资源耗尽 | 无界分配、无界重试、无界队列 |
| 内部信息泄露 | `cause` 是否进入对外响应（**不允许**） |

---

## 2. 评审者独立性

### 本仓库的最低要求

| 规则 | 等级 | 判据 |
|---|---|---|
| AI 产出代码需人工评审记录 | `hard-mandatory` | PR 上有 `approved` 的人工评审 |
| **AI 作者的改动需额外一名评审** | `hard-mandatory` | AI 产出的 PR 评审人数 ≥ 2（含 1 名人工） |
| 评审人不得是该改动的作者 | — | 独立性判据 |
| AI 产出需 provenance 标注 | `hard-mandatory` | 提交信息含 `Co-Authored-By` 尾注 |

### 独立性在本仓库的边界

上游的独立性有**三层**（`none` / `attested` / `isolated`），
本仓库**只落地第一层**：

| 层 | 含义 | 本仓库 |
|---|---|---|
| `none` | 不作声明 | ✅ 现状 |
| `attested` | 「评审者是另一个代理」的启发式信号 | ⏳ 待接入（需沙箱边界） |
| `isolated` | 评审跑在实现者无法触及的沙箱里，签名密钥只有 CI 持有 | ⏳ 待接入（需 CI 独占签名密钥） |

**这意味着什么**：本仓库的「独立」是**程序性**的 —— 靠「评审者只评审不修复」
+「评审人 ≠ 作者」+「评审结论写在 PR 上、实现者不能改」来保证。
它**不是**密码学意义上的隔离。

**若将来开放外部贡献者 PR**：这是必须同时引入的第一道门
（见 `docs/ai-sdlc/coverage-matrix.md` §5 根因 D）。

---

## 3. 评审的组织方式

### 本仓库用**三类并行评审**（不是上游的分阶段评审）

```
                 ┌─ code-reviewer    （正确性）─┐
一个 PR  ───────┼─ test-reviewer    （测试）    ├─→ 汇总 ─→ 结论
                 └─ security-reviewer（安全）    ┘
```

| 规则 | 说明 |
|---|---|
| **必须并行发出** | 三个评审代理在同一条消息里发出，不是串行等待 |
| **产出独立** | 每个评审代理**不得互相引用**（避免意见污染） |
| **汇总独立** | 汇总由**独立的汇总步骤**做，**不得**由某个评审代理代劳 |
| **不可解析 = 未通过** | 任一评审者返回不可解析的裁决 → 该评审计为**未通过** |

> **「静默通过是最危险的失败模式」** —— 评审者返回了无法解析的东西，
> 如果被当成「大概没问题」，那这次评审就等于没发生。

上游的分阶段评审管道（强模型出计划 → 弱模型跑探针 → 强模型裁决）**成本更低**，
但需要 judgment layer 与探针执行器，**本仓库未接入**。
详见 `docs/ai-sdlc/coverage-matrix.md` §3 `staged-review.md` 行。

### 汇总的判据

| 规则 | 原因 |
|---|---|
| 汇总者**不是**任一评审者 | 避免单一视角偏置 |
| 汇总**基于各评审自己的裁决** | 不得由汇总者「综合感觉」 |
| `critical` / `major` 发现 → 必须回到实现阶段 | 不能「下次再说」 |
| 每个 finding **必须指向具体的 AC 编号** | 否则无法判定该 AC 是否被满足 |

---

## 4. 评审结论模板

### 评审代理的裁决（结构化，不许散文）

```json
{
  "verdict": "approved",
  "findings": [
    {
      "severity": "critical",
      "file": "crates/nexus-llm/src/retry.rs",
      "line": 142,
      "acRef": "#3",
      "remit": "correctness",
      "summary": "超时被当作 Retryable，但超时语义是 UnknownOutcome",
      "evidence": "ModelTimeout 走的是 retryable 分支，见 retry_policy() 实现"
    }
  ],
  "acCoverage": { "#3": "not-met", "#7": "met" }
}
```

| 字段 | 必填 | 说明 |
|---|---|---|
| `verdict` | 是 | `approved` / `changes-requested` / `blocked` |
| `findings[].severity` | 是 | `critical` / `major` / `minor` / `nit` |
| `findings[].acRef` | 是 | **指向具体 AC 编号** —— 没有 AC 锚点的 finding 无法判定 |
| `findings[].evidence` | 是 | 依据是什么。**没有证据的 finding 不算 finding** |
| `acCoverage` | 是 | 逐条 AC 的结论：`met` / `not-met` / `unclear` |

### 不可解析时的处理

**禁止**「善意补全」。

报告方必须如实报告 `approved: false` / `failed: true`，并附上**原始返回**。
**编造或软化结论是比评审失败更严重的事故** —— 它让门禁变成假象。

---

## 5. 迭代与终止

```
第 1 轮 changes-requested → 回 S4 修复 → 第 2 轮
                              │
                              ├─ 第 2 轮通过 → 合并
                              └─ 第 2 轮仍有 critical/major → 停止
                                                        ↓
                                          标记 needs-human-attention
                                                        ↓
                                                       交人工
```

| 规则 | 说明 |
|---|---|
| **最多迭代 2 轮** | 第 2 轮还有 critical/major → **交人工**，不无限循环 |
| `critical` / `major` → 必须回 S4 | 不能「下次再说」 |
| 修复后**必须重跑评审** | 改了代码的评审结论自动失效 |
| **契约级问题不得就地修复** | 见下 |

### 契约级问题：升级，不就地修

评审发现的下列问题**不得**在评审回合里就地修复 —— 它们触及**架构文档 §6.3 的已定稿条款**：

- 错误码值
- 事件名与负载形状
- ID 底层类型
- crate 依赖方向
- 对外响应形状

**正确动作**：按 `.cursor/rules/ai-sdlc-decision-authority.mdc` §4.2 升级（登记 + 呈现选项），
修复后**必须重跑评审**。

**为什么**：就地修复 = 代理在代码里顺手定了决策 —— 这正是 S4 要防的越权。
而且改了契约却不重跑评审，就等于**评审通过的是旧版本**。

---

## 6. Provenance 标注

每个 AI 生成的产物都要记录 provenance。

| 字段 | 必填 | 本仓库载体 |
|---|---|---|
| `model` | 是 | `Co-Authored-By` 尾注中的模型名 |
| `tool` | 是 | `Co-Authored-By` 中的工具标识 |
| `promptHash` | 是 | 不适用（本地开发不强制） |
| `timestamp` | 是 | 提交时间戳（git 记录） |
| `humanReviewer` | 否 | PR 评审记录 |
| `reviewDecision` | 否 | PR 评审结论 |

### 示例

```
feat(llm): 落地重试退避与熔断器 (NEX-18)

Co-Authored-By: MiniMax-M3.1-Flash <noreply@cursor.sh>
```

**Provenance 记录一旦创建即不可变。**

---

## 7. 评审的检查清单

发出评审前：

- [ ] diff 完整（不是只看了片段）
- [ ] 三类 remit 都覆盖了
- [ ] 每个 finding 有 `acRef` 与 `evidence`
- [ ] 每个 finding 有 severity，且 critical/major 的判据成立
- [ ] 评审者不是该改动的作者
- [ ] 结论用结构化格式，不是散文

接收评审结论时：

- [ ] 裁决可解析（不可解析 = 未通过）
- [ ] `acCoverage` 覆盖了**全部** AC，不只是有 finding 的
- [ ] 有 `critical` / `major` → 回到 S4，不「下次再说」
- [ ] 契约级问题 → 升级，不就地修
- [ ] 修复后 → **重跑评审**

---

## 相关文件

| 文件 | 作用 |
|---|---|
| `.cursor/rules/ai-sdlc-quality-gates.mdc` | **判据**（执行等级、评审门禁、override、provenance） |
| `.cursor/rules/ai-sdlc-agents.mdc` | 角色定义、编排模式、handoff 契约 |
| `.cursor/rules/ai-sdlc-decision-authority.mdc` | 契约级问题的升级路径 |
| `docs/ai-sdlc/dor-playbook.md` | 上一道门：DoR |
| `docs/ai-sdlc/coverage-matrix.md` | 上游 staged review / reviewer independence 的落地状态 |
