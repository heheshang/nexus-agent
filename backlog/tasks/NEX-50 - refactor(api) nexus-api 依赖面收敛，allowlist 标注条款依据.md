---
id: NEX-50
title: "refactor(api): nexus-api 依赖面收敛，allowlist 标注条款依据"
status: To Do
assignee: []
labels: [api, deps, architecture]
priority: medium
dependencies: []
references:
  - "docs/architecture/2026-11-06-nexusagent-improvement-plan.md §4.1 F-04、§4.2 D-03"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.1 crate 图"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §4.1.1（2026-11-05 修正）"
  - "crates/nexus-api/tests/architecture.rs"
---

## Description

**问题**：`nexus-api` 声明 **9 条**内部 crate 依赖，其中 **6 条**在 `src/` 里**一次都没用到**。

| 依赖 | 在 `src/` 中的使用 | 处置 |
|---|---|---|
| `nexus-core` | ✅ 4 个文件 | **保留** |
| `nexus-observability` | ✅ 1 个文件（`metrics.rs`） | **保留** |
| `nexus-agent` | ✅ 2 个文件（`bridge.rs` / `router.rs` 借事件流**类型**） | **保留** |
| `nexus-config` | ❌ **0** | **删除** |
| `nexus-model` | ❌ **0** | **删除** |
| `nexus-security` | ❌ **0** | **删除** |
| `nexus-service` | ❌ **0** | **保留**（§4.1 crate 图 `API --> SVC`） |
| `nexus-execution` | ❌ **0** | **保留**（§4.1.1 的 2026-11-05 修正明确承认） |
| `nexus-llm` | ❌ **0** | **删除** |

**为什么「没用」是问题**：架构文档 §4.1.1 的判据是「**类型可以借，语义不能借**」。
但「**一个类型都没借**，只留下一条依赖边」不是「借了类型」，是**纯粹的依赖噪音**。

更要紧的是 `nexus-service`：架构测试 `the_api_crate_does_not_use_the_service_crate`
只扫 `src/` 的**标识符**，**不查 manifest**。依赖已在，闸门只是没检查。
一旦有人写 `use nexus_service::...`，那条断言会红 —— 但那时已经晚了一步。

**同时要收紧 allowlist**：现在 `the_api_crate_adds_no_internal_dependencies` 的
allowlist 是 **9 条全放行**，**每条都没有条款依据**。
修法是给每条依赖标注「依据架构文档哪一条」，未标注的不得放行。

## 范围边界

### 做

- `crates/nexus-api/Cargo.toml` —— 删除 **4 条**零引用依赖
- `scripts/check-deps.sh` 的 `deps_of nexus-api` —— 同步
- `crates/nexus-api/tests/architecture.rs` —— allowlist 标注**条款依据** + 加「零引用依赖」的断言

### 不做

| 项 | 理由 |
|---|---|
| 删 `nexus-service` / `nexus-execution` | §4.1 crate 图与 §4.1.1 的修正**明确承认**这两条边 —— 见下方决策记录 |
| 改 **crate 依赖方向** | 架构文档 §6.3 第 5 项**已定稿**；本任务**不涉及**方向 |
| 改任何**架构断言的判定内容** | 只改 allowlist 的**标注形式**与**新增**一条断言 |
| 碰 `nexus-service` / `nexus-execution` 的 `src/` | 那是 P6 的工作面 |

## Acceptance Criteria

- [ ] #1 `crates/nexus-api/Cargo.toml` **不再声明** `nexus-config` / `nexus-model` / `nexus-security` / `nexus-llm`
- [ ] #2 `crates/nexus-api/Cargo.toml` **仍声明** `nexus-core` / `nexus-observability` / `nexus-agent` / `nexus-service` / `nexus-execution`
- [ ] #3 `scripts/check-deps.sh` 的 `deps_of nexus-api` 与 `Cargo.toml` **一致**（规则 7 通过）
- [ ] #4 allowlist 的**每一条**都标注了「依据架构文档哪一条」—— **无条款依据**的依赖不得放行
- [ ] #5 **新增**断言：`nexus-api` 的 `src/` 里**零引用**的内部依赖**不得**出现在 manifest 里
- [ ] #6 **负控**：加一条**零引用**的内部依赖 → 门禁**必须红**
- [ ] #7 **负控**：加一条**有引用但无条款依据**的依赖 → 门禁**必须红**
- [ ] #8 `the_api_crate_does_not_use_the_service_crate` 仍然通过（`nexus-service` 边**保留**，但 `src/` 里仍然零引用）
- [ ] #9 `the_api_crate_adds_no_internal_dependencies` 的**原有**判定内容**不变**（P5-I25 契约的 `verified_by` 仍指向它）
- [ ] #10 `bash scripts/check-deps.sh` 通过；`bash scripts/check-specs.sh` 通过（规则 3 引用的测试名仍存在）
- [ ] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 11 条 AC，均可写成断言或命令退出码
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 改进计划 §4.1 F-04 与 §4.2 D-03、架构文档 §4.1、
  §4.1.1、`crates/nexus-api/tests/architecture.rs` 均存在
- 门禁 4（范围有界）：通过 —— 一个 manifest + 一个脚本的**一行** + 一个测试文件；
  **不涉及**依赖**方向**，**不涉及**契约变更
- 门禁 5（受影响面被指名）：通过 —— `crates/nexus-api/Cargo.toml`、
  `scripts/check-deps.sh`（`deps_of nexus-api`）、`crates/nexus-api/tests/architecture.rs`
- 门禁 6（完成态可描述）：通过 —— `nexus-api` 的依赖面只剩**有条款依据**的边，
  零引用的依赖边**被门禁拦住**
- 门禁 7（无隐形依赖）：通过 —— 无前置任务；
  被删除的 4 条依赖在 `src/` 里**零引用**（已逐条 grep 确认），删除**不影响**编译

**结论：pass**

## 决策记录

### 类别：decide-and-proceed

- **决策**：删除 `nexus-config` / `nexus-model` / `nexus-security` / `nexus-llm` **四条**；
  **保留** `nexus-service` / `nexus-execution`。
- **依据**：前四条**零引用**且**不在** §4.1 crate 图的 `API -->` 出边上。
  后两条**有**明确条款依据：§4.1 的 crate 图第 1327 行画着 `API --> SVC`，
  §4.1.1 的 2026-11-05 修正明确承认「`nexus-api` 的 `Cargo.toml` 依赖
  `nexus-agent` / `nexus-execution`」是**既有事实**。
- **可逆性**：高 —— 加回 manifest 行即可。
- **影响面**：`crates/nexus-api/Cargo.toml`、`scripts/check-deps.sh`。
- **备注**：改进计划 §4.1 F-04 记录了本轮评估中曾有一个候选方案主张
  「删掉全部 6 条」—— 那是**错的**，会让协议层永久失去接业务编排的能力。
  本任务**不重开**该判断。

### 类别：decide-and-proceed

- **决策**：`nexus-service` / `nexus-execution` 两条边**保留**，
  并在 `scripts/check-deps.sh` 的 `deps_of nexus-api` 旁边注明
  「此边为 §4.1 目标态预留，待 P6 接线」。
- **依据**：改进计划 §4.2 **D-03** 记录了「保留未使用的边是否是想要的」属于
  **架构意图**而非机械问题 —— 改进计划给出的**建议是 A（留）**。
  按 `ai-sdlc-decision-authority.mdc` §2.1，「收紧控制不算削弱」——
  保留**已授权**的边是**维持**现状，不是**削弱**控制。
  而**删除**它会**推翻** §4.1 crate 图的 `API --> SVC` —— 那才是削弱。
- **可逆性**：高 —— 改脚本注释即可。
- **影响面**：`scripts/check-deps.sh`。
- **⚠️ 阻塞关系**：D-03 在改进计划里被登记为「需操作者确认」。
  本任务**不阻塞**于它 —— 采纳的是改进计划**自己给出的建议**（A），
  且 A 是**维持现状**的选项（不动作），可逆性最高。
  若操作者裁决为 B（删），AC #2 与 AC #5 需相应调整，其余 AC 不变。
