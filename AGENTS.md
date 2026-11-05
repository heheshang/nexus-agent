# NexusAgent · 代理指令

> 本文件是 AI 代理在本仓库工作的**常驻指令**。完整规范见 `.cursor/rules/`。

## 0. 权威顺序

1. `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` —— **架构与契约的唯一权威**（错误码表、事件清单、ID 类型表、crate 边界）。
2. `.cursor/rules/ai-sdlc-workflow.mdc` —— **研发流程总纲**（生命周期阶段表、分层判定、规则索引）。
3. `.cursor/rules/ai-sdlc-artifacts.mdc` —— 产出物规范（RFC / Spec / Task 三层载体）。
4. `.cursor/rules/ai-sdlc-dor-gate.mdc` —— DoR 门禁（七项检查、Stage A/B）。
5. `.cursor/rules/ai-sdlc-implementation.mdc` —— 实现阶段规范。
6. `.cursor/rules/ai-sdlc-quality-gates.mdc` —— 质量门禁与评审规范。
7. `.cursor/rules/ai-sdlc-decision-authority.mdc` —— 决策权威与升级。
8. `.cursor/rules/ai-sdlc-autonomy.mdc` —— 自治等级与路由。
9. `.cursor/rules/ai-sdlc-agents.mdc` —— Agent 角色与协作。
10. `.cursor/rules/ai-sdlc-adapters.mdc` —— 适配器与外部系统边界。
11. `.cursor/rules/ai-sdlc-observability.mdc` —— 可观测性与度量。
12. `.cursor/rules/rust-code-standards.mdc` —— Rust 代码规范。
13. `.cursor/rules/language.mdc` —— 语言约定（始终用简体中文回答）。

冲突时**以序号小的为准**，并且**不要静默选一个** —— 指出冲突并请求决策。

> 规则清单与 `.cursor/rules/` 下的实际文件**一一对应**，由 `bash scripts/check-rules.sh` 校验。

---

## 1. 全生命周期流程

**任何工作都必须走完 S0→S9，不得跳阶段。**
阶段表（进入条件 / 产出物 / 退出条件 / 失败处理）见 `.cursor/rules/ai-sdlc-workflow.mdc` §2。

| # | 阶段 | 关键退出条件 |
|---|---|---|
| S0 | 需求捕获 | 诉求能归类到某一层 |
| S1 | 分层判定 | 层级已判定且落在 Task 层 |
| S2 | 产出物编写 | 产出物落盘且引用可解析 |
| S3 | DoR 门禁 | 七项门禁全部通过 |
| S4 | 实现 | 七道门禁本地全绿 |
| S5 | 三层验证 | 每条新代码至少落在一层 |
| S6 | 评审 | 评审门禁通过（含 provenance 与评审人数） |
| S7 | 合并与交付 | 提交在 `main` 上，任务移入 `backlog/completed/` |
| S8 | 部署与运维 | 变更可观测、可回滚 |
| S9 | 度量与校准 | 指标口径完整，门禁按数据调整 |

**两道硬边界**：S3→S4（DoR 先于任何代码写入）、S6→S7（评审先于合并）。

## 2. 必须遵循 AI-SDLC 流程

本仓库的一切代码编写**必须**遵循 AI-SDLC 规范。要点：

- **先分层再动手**：跨特性/架构级争议 → RFC；一个特性多任务 → Spec；单个 PR → Task。任何工作都不能跳过 Task 层。
- **DoR 门禁七项全过**才能进入实现：AC 可二元测试（1–20 条）、无未决标记、引用可解析、范围有界、受影响面被指名、完成态可描述、无隐形依赖。默认严重度是 Block；先跑确定性检查（Stage A）再跑语义判定（Stage B）。
- **门禁分级**：`hard-mandatory`（依赖方向违规、契约破坏、`unsafe_code`、CI 红）不可覆盖；`soft-mandatory`（覆盖率、提交规范）需授权人覆盖并记录理由；`advisory`（文档同步）只记录。AI 产出的代码需人工评审，AI 作者的改动需**额外一名评审**。
- **决策权威来自仓库**：`main` 上的决策记录才是授权。决策分三类：(a) 可回滚、影响小 → 自行判定并记录，立即应用；(b) 难回滚、影响大或**削弱控制** → 记录并附时间盒；(c) 法律/金钱/凭据/只能以操作者身份执行 → **绝不自行决定**，只搁置该任务。
- **OQ 解决禁令**：不得添加 `**Resolution:**` 标记，不得用设计决策替换开放问题，不得用代码隐式解决。阻塞性 OQ → 停止并升级（列出 RFC 编号、问题、选项）。
- **Guardrails 永不绕过**：被门禁拒绝时的正确动作是登记任务补齐 sanctioned path，不是绕过 hook。
- **Away rule**：操作者不在场时不发起阻塞性提问，问题交上层决策流程。
- **五步法**：质疑需求 → 删除 → 简化 → 加速 → 最后才自动化（需逐条具名说明）。
- **不扩大范围**：审计/评审类任务发现后续工作，只提建议并停下，等待授权。
- **自治等级**：默认 `ai-with-review`（AI 产出 + 人工批准）；`fully-autonomous` 需低复杂度**且** Level 2/3，任一不满足即降级。

## 3. 写代码前

1. 读架构文档中与本次改动相关的章节，确认所依赖的条款**已定稿**（§6.3 清单）。
2. 若条款未定稿或本次改动要改已定稿条款 → 停止，先出决策/RFC。
3. 明确本次改动的**契约影响**（五问，逐项回答「是」或「否」）：错误码值 / 事件名与负载形状 / ID 底层类型 / crate 依赖方向 / 对外响应形状。触及则必须更新契约测试与文档表格，并标注**破坏性变更**。

## 4. 提交前必须全绿

```bash
bash scripts/check-lock.sh        # 依赖锁定纪律
bash scripts/check-deps.sh        # crate 依赖方向铁律
bash scripts/check-naming.sh      # 标识符不得含中文
bash scripts/check-backlog.sh     # backlog 组织纪律
bash scripts/check-specs.sh       # Spec 层组织纪律
bash scripts/check-rules.sh       # 规则文件一致性
bash scripts/check-docs.sh        # 文档层一致性
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

本地开发可用 `bash scripts/dev.sh <check|fmt|lint|test|audit>`。这些就是 CI 的准入门禁（`hard-mandatory`）。

`examples/` 是独立 workspace，验证命令为 `cd examples && cargo test --workspace`。

## 5. 硬性编码红线

- 不得绕过 crate 依赖方向。`sqlx` 只允许出现在 `nexus-repo` / `nexus-infra` / `nexus-execution`；`nexus-model` 必须保持纯 DTO。
- 不得改动错误码值 / 事件名 / ID 底层类型（前后端契约）。
- 不得让内部错误原因（`cause`）进入对外响应。
- 不得把「结果未知」（`UnknownOutcome`）的错误当可重试处理。
- 不得用 `#[serde(flatten)]` 于 enum 变体；不得 derive `Serialize` 给 `ErrorCode`。
- 不得引入 `unsafe`（workspace 已 forbid）。release profile 必须保持 `panic = "unwind"`。
- 不得在 `.await` 期间持有 `std::sync::MutexGuard`。
- 不得把凭据 / 密钥写入仓库、日志或错误消息。

## 6. 测试要求

每个新代码至少落在三层验证之一：类型层（`trybuild` 负向测试）、结构层（架构测试）、行为层（不变量测试）。参考架构文档 §5。

结构层扫描源码的断言**必须剥离注释**；「看起来显然」的不变量用变异测试验证断言真的有效。

## 7. 提交与任务

- 任务载体：`backlog/tasks/NEX-<n> - <slug>.md`，完成后移入 `backlog/completed/`。
- 提交信息：Conventional Commits，标题末尾带任务号，并带 `Co-Authored-By` 尾注（provenance）。

```
feat(core): 增加取消语义类型状态 (NEX-101)
```

- Git Flow：**始终 rebase** 到 `main`，**绝不把 `main` merge 进来**，保持线性历史。
- 禁止 `git push --force`、禁止删除分支、禁止 `git reset --hard` / `git checkout -- .` / `git restore .`。

## 8. 相关文件

| 文件 | 作用 |
|---|---|
| `docs/README.md` | `docs/` 文档总索引 |
| `docs/operations/ai-sdlc-workflow-handbook.md` | 人类可读的流程手册 |
| `docs/ai-sdlc/README.md` | AI-SDLC 文档导航（覆盖矩阵 / 术语表 / DoR / 评审 / 度量 / 适配器手册） |
| `docs/ai-sdlc/coverage-matrix.md` | 上游 AI-SDLC 规范逐条适配状态 |
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | 架构与契约的**唯一权威** |
| `backlog/README.md` | 任务文件格式与生命周期 |
| `scripts/check-rules.sh` | 规则文件一致性校验 |
| `scripts/check-docs.sh` | 文档层一致性校验 |
