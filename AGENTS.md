# NexusAgent · 代理指令

> 本文件是 AI 代理在本仓库工作的**常驻指令**。完整规范见 `.cursor/rules/`。

## 0. 权威顺序

1. `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` —— **架构与契约的唯一权威**（错误码表、事件清单、ID 类型表、crate 边界）。
2. `.cursor/rules/ai-sdlc-workflow.mdc` —— **研发流程规范**（DoR 门禁、Spec 驱动、质量门禁、决策权威）。
3. `.cursor/rules/rust-code-standards.mdc` —— **Rust 代码规范**。
4. `.cursor/rules/language.mdc` —— 语言约定（始终用简体中文回答）。

冲突时**以序号小的为准**，并且**不要静默选一个** —— 指出冲突并请求决策。

## 1. 必须遵循 AI-SDLC 流程

本仓库的一切代码编写**必须**遵循 AI-SDLC 规范。要点：

- **先分层再动手**：跨特性/架构级争议 → RFC；一个特性多任务 → Spec；单个 PR → Task。任何工作都不能跳过 Task 层。
- **DoR 门禁七项全过**才能进入实现：AC 可二元测试（1–20 条）、无未决标记、引用可解析、范围有界、受影响面被指名、完成态可描述、无隐形依赖。默认严重度是 Block；先跑确定性检查（Stage A）再跑语义判定（Stage B）。
- **门禁分级**：`hard-mandatory`（依赖方向违规、契约破坏、`unsafe_code`、CI 红）不可覆盖；`soft-mandatory`（覆盖率、提交规范）需授权人覆盖并记录理由；`advisory`（文档同步）只记录。AI 产出的代码需人工评审，AI 作者的改动需**额外一名评审**。
- **决策权威来自仓库**：`main` 上的决策记录才是授权。决策分三类：(a) 可回滚、影响小 → 自行判定并记录，立即应用；(b) 难回滚、影响大或**削弱控制** → 记录并附时间盒；(c) 法律/金钱/凭据/只能以操作者身份执行 → **绝不自行决定**，只搁置该任务。
- **OQ 解决禁令**：不得添加 `**Resolution:**` 标记，不得用设计决策替换开放问题，不得用代码隐式解决。阻塞性 OQ → 停止并升级（列出 RFC 编号、问题、选项）。
- **Guardrails 永不绕过**：被门禁拒绝时的正确动作是登记任务补齐 sanctioned path，不是绕过 hook。
- **Away rule**：操作者不在场时不发起阻塞式提问，问题交上层决策流程。
- **五步法**：质疑需求 → 删除 → 简化 → 加速 → 最后才自动化（需逐条具名说明）。
- **不扩大范围**：审计/评审类任务发现后续工作，只提建议并停下，等待授权。

## 2. 写代码前

1. 读架构文档中与本次改动相关的章节，确认所依赖的条款**已定稿**（§6.3 清单）。
2. 若条款未定稿或本次改动要改已定稿条款 → 停止，先出决策/RFC。
3. 明确本次改动的**契约影响**：是否触及错误码值、事件名与负载形状、ID 类型、crate 依赖方向。触及则必须更新契约测试与文档表格，并标注破坏性变更。

## 3. 提交前必须全绿

```bash
bash scripts/check-lock.sh        # 依赖锁定纪律
bash scripts/check-deps.sh        # crate 依赖方向铁律
bash scripts/check-naming.sh      # 标识符不得含中文
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

本地开发可用 `bash scripts/dev.sh <check|fmt|lint|test|audit>`。这六道就是 CI 的准入门禁（`hard-mandatory`）。

`examples/` 是独立 workspace，验证命令为 `cd examples && cargo test --workspace`。

## 4. 硬性编码红线

- 不得绕过 crate 依赖方向。`sqlx` 只允许出现在 `nexus-repo` / `nexus-infra` / `nexus-execution`；`nexus-model` 必须保持纯 DTO。
- 不得改动错误码值 / 事件名 / ID 底层类型（前后端契约）。
- 不得让内部错误原因（`cause`）进入对外响应。
- 不得把「结果未知」（`UnknownOutcome`）的错误当可重试处理。
- 不得用 `#[serde(flatten)]` 于 enum 变体；不得 derive `Serialize` 给 `ErrorCode`。
- 不得引入 `unsafe`（workspace 已 forbid）。release profile 必须保持 `panic = "unwind"`。

## 5. 测试要求

每个新代码至少落在三层验证之一：类型层（`trybuild` 负向测试）、结构层（架构测试）、行为层（不变量测试）。参考架构文档 §5。

## 6. 提交与任务

- 任务载体：`backlog/tasks/NEX-<n> - <slug>.md`，完成后移入 `backlog/completed/`。
- 提交信息：Conventional Commits，标题末尾带任务号，并带 `Co-Authored-By` 尾注。

```
feat(core): 增加取消语义类型状态 (NEX-101)
```

- Git Flow：**始终 rebase 到 `main`，绝不把 `main` merge 进来**，保持线性历史。
