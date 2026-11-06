---
id: NEX-49
title: "fix(scripts): 用 token 级扫描替换 sed 剥注释，规则 5 支持多 crate"
status: To Do
assignee: []
labels: [scripts, spec, defect, gate]
priority: high
dependencies:
  - NEX-48
references:
  - "docs/architecture/2026-11-06-nexusagent-improvement-plan.md §4.1 F-03、F-05"
  - "docs/specs/README.md（crate 字段与逐 Spec 判定纪律）"
  - "docs/specs/p6/contracts/invariants.json"
  - "scripts/check-specs.sh"
---

## Description

**问题 1（F-03）**：`scripts/check-specs.sh` 规则 5 与 `scripts/check-deps.sh` 规则 6b
都用同一条命令剥注释：

```bash
find "$crate/src" -name '*.rs' -type f -print0 | xargs -0 sed -e 's://.*::' | grep -ohE 'ErrorCode::[A-Za-z]+'
```

`sed` 的 `s://.*::` 是**纯文本**替换，**不理解字符串**。
一行里有 `"http://x"` 时，`//` 之后的内容**整行被丢弃**。

实测（真实源码）：

| 文件 | 真实 `ErrorCode::` 变体数 | `sed` 之后 | 差 |
|---|---|---|---|
| `crates/nexus-core/src/error.rs` | 43 | 42 | 1 被吞 |
| `crates/nexus-llm/src/provider.rs` | 1 | **0** | **全部被吞** |
| `crates/nexus-api/src/response.rs` | 2 | 2 | 0 |

被吞的内容恰好都在注释里，所以**目前**没有真实违规被吞 ——
但这是**运气**，不是防线：**一行真实代码只要在同一行前面出现过 `//`（哪怕在字符串里），
它使用的错误码就完全不进门禁的视野**。

**为什么这条是 critical**：规则 5 的职责是「新增码值 = 破坏性变更，必须被拦住」。
它是**契约破坏的唯一防线** —— 业务码表已定稿（架构文档 §6.3 第 3 项），
一旦有人绕过它新增 `ErrorCode` 变体，**没有任何东西会红**。

**问题 2（F-05）**：P6 契约的 `crate` 字段是**单值**（`"nexus-execution"`），
而同一份契约里有 **23 处** `nexus-workflow` 的路径引用。
规则 5 读 `crate` 字段，然后**只扫那一个 crate** 的源码
⇒ **`nexus-workflow/src` 里使用白名单外错误码，不会让门禁变红**。

**为什么现在修最便宜**：P6 **尚未开工**（`nexus-workflow` 与 `nexus-execution` 都是 7 行空壳）。
现在改规则 5 的多 crate 支持，是**改一个脚本**；
等 P6 落地、两个 crate 各有几百行代码之后改，就是**改一个已经有一堆「已知违规」要豁免的东西**。

## 范围边界

### 做

- `scripts/check-specs.sh` 规则 5：用 **token 级**扫描替换 `sed` 剥注释
- `scripts/check-specs.sh` 规则 5：支持**多个** crate
- `scripts/check-deps.sh` 规则 6b：用 **token 级**扫描替换 `sed` 剥注释
- `docs/specs/p6/contracts/invariants.json` 的 `crate` 字段声明**两个** crate
- 负控验证（见 AC）

### 不做

| 项 | 理由 |
|---|---|
| 改 `ErrorCode` 的**变体集合** | 42 个业务码是**已定稿**契约（架构文档 §6.3 第 3 项） |
| 改各 Spec 的**白名单内容** | 白名单是契约；本任务只改**怎么扫描**，不改**扫什么** |
| 改**「逐 Spec 判定、不用并集」**的纪律 | 那是 P3 建立的纪律，本任务**保持**它 |
| 碰 `nexus-execution` / `nexus-workflow` 的 `src/` | 那是 P6 的工作面；本任务只改**脚本**与**契约文件** |
| 引入 `cargo machete` / `cargo-deny` | 改进计划 H-03 / F-09 —— 现在不做 |

## Acceptance Criteria

- [ ] #1 `scripts/check-specs.sh` 与 `scripts/check-deps.sh` 里**不再有** `sed -e 's://.*::'` —— `sed` 剥注释**完全移除**
- [ ] #2 规则 5 的错误码扫描走 **token 级**实现（`syn`），注释与字符串**在 token 层不是同一类东西**
- [ ] #3 规则 6b 的 `nexus-model` 数据库痕迹扫描走**同一份** token 级实现
- [ ] #4 **等价性**：token 级实现与 `sed` 版在**当前**全部真实源码上给出**相同**的变体集合（迁移期可比对）
- [ ] #5 **反例**：一行内含 `"http://x"` **且**使用 `ErrorCode::Forbidden` → 该变体**必须**被检出
- [ ] #6 规则 5 支持**多个** crate：契约的 `crates` 字段接受**字符串或数组**，**逐个**扫描
- [ ] #7 **纪律保持**：「逐 Spec 判定、不用并集」的行为**不变** —— P3 的 crate 不得用 P4 的码
- [ ] #8 P6 契约声明 `nexus-execution` **与** `nexus-workflow` **两个** crate
- [ ] #9 **负控**：在 `nexus-workflow/src` 加一个白名单外的 `ErrorCode::Forbidden` → 门禁**必须红**
- [ ] #10 **负控**：在任一 Spec 的 crate 加一个白名单外的 `ErrorCode::Forbidden` → 门禁**必须红**
- [ ] #11 单值 `crate` 字段**向后兼容**（不静默跳过）—— 旧写法**要么**被识别为单元素数组，**要么**硬失败
- [ ] #12 `bash scripts/check-specs.sh` 通过；`bash scripts/check-deps.sh` 通过；`bash scripts/check-lock.sh` 通过
- [ ] #13 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-06（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

- 门禁 1（AC 可二元测试）：通过 —— 13 条 AC，均可写成断言或命令退出码
- 门禁 2（无未决标记）：通过 —— 正文无 `TBD` / `TODO` / `XXX` / `FIXME` / `???` /
  `not sure` / `decide later` / `up to the dev`
- 门禁 3（引用可解析）：通过 —— 改进计划 §4.1 F-03/F-05、`docs/specs/README.md`、
  `docs/specs/p6/contracts/invariants.json`、`scripts/check-specs.sh` 均存在
- 门禁 4（范围有界）：通过 —— 两个脚本 + 一份契约文件的 `crate` 字段；
  **不碰**任何 `src/`，**不改**码值与白名单
- 门禁 5（受影响面被指名）：通过 —— `scripts/check-specs.sh`（规则 5、规则 6b 在 check-deps.sh）、
  `scripts/check-deps.sh`（规则 6b）、`docs/specs/p6/contracts/invariants.json`
- 门禁 6（完成态可描述）：通过 —— 门禁在「同一行含 `//` 与白名单外码」时**变红**，
  且 `nexus-workflow` 的错误码**进入**规则 5 的视野
- 门禁 7（无隐形依赖）：通过 —— 前置 NEX-48 提供 token 级实现；
  `syn` 3.0.9 **已在 `Cargo.lock`** 中

**结论：pass**

## 决策记录

### 类别：decide-and-proceed

- **决策**：规则 5 的 `crates` 字段**同时**接受字符串与数组，单值写法**向后兼容**。
- **依据**：P3 / P4 / P5 三份既有契约都写的是**单值** `crate`。
  硬失败会让三份既有契约同时变红 —— 那是**与本次改动无关**的破坏。
  向后兼容让「新增能力」与「既有契约」解耦：旧写法**不会**被静默跳过（AC #11 要求要么识别、要么硬失败）。
- **可逆性**：高 —— 改回数组即可。
- **影响面**：`scripts/check-specs.sh`、`docs/specs/*/contracts/invariants.json`。

### 类别：decide-and-proceed

- **决策**：P6 契约**同时**声明 `nexus-execution` 与 `nexus-workflow`。
- **依据**：`docs/specs/p6/contracts/invariants.json` 里 **23 处** `nexus-workflow` 的路径引用
  —— 那些不变量**就属于** `nexus-workflow`，却因为 `crate` 字段单值而**不受错误码扫描保护**。
  这不是「扩大扫描范围」，是**让扫描范围与契约声明的事实一致**。
- **可逆性**：高 —— 改回单值即可。
- **影响面**：`docs/specs/p6/contracts/invariants.json`。

### 类别：decide-and-proceed

- **决策**：**不**碰 `nexus-execution` / `nexus-workflow` 的 `src/`。
- **依据**：改进计划 §5.3 明确「W0 不得碰 P6 的实现面 —— 那是 P6 的工作面」。
  AC #9 的负控验证会在 `nexus-workflow/src` 里**临时**加一行违规代码，
  验证门禁变红后**立即移除** —— 那是**故障注入**，不是实现改动。
- **可逆性**：n/a（不动作）。
- **影响面**：`crates/nexus-workflow/src/`（仅负控验证期间）。
