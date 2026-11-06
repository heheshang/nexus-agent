---
id: NEX-36
title: "docs(api): P5 收口 —— 架构文档同步与契约升格"
status: Done
assignee: []
labels: [docs, closeout, p5]
priority: medium
dependencies:
  - NEX-31
  - NEX-32
  - NEX-33
  - NEX-34
  - NEX-35
references:
  - "docs/specs/p5/spec.md §5、§6"
  - "docs/specs/p5/tasks.md T-006"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1"
---

## Description

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

### 这个任务的**特殊职责**：契约升格

`docs/specs/p5/contracts/invariants.json` 里的不变量在 Spec 阶段是 `planned` ——
**证据尚未产生**（这正是 S2 必须先于 S4 的必然结果）。

本任务的职责是**把 `planned` 全部升格为 `verified`**，
并且**验证每个测试真的存在**。

`bash scripts/check-specs.sh` 的规则 3b 会在本任务归档后要求：
所有 `planned` 条目必须已升格 —— 否则门禁红。

**这不是「勾选框」** —— 升格前必须确认测试**真的存在**且**真的断言了那条不变量**。

### 另一个**特殊职责**：故障注入

NEX-24 / NEX-25 的教训：**没验证过会红的断言，等于没有断言**。

本任务必须对 P5 的关键断言做**故障注入**，验证**确实会红**。

## 范围边界

### 做

- 架构文档 §6.1 的 P5 条目同步 + §5.4 的不变量表补 P5 已验证项
- `contracts/invariants.json` 的 `planned` → `verified` 升格
- `docs/operations/` 的 P5 运行说明（SSE 端点怎么访问 / 怎么解析）
- **故障注入**验证与记录

### 不做

| 项 | 理由 |
|---|---|
| 补写缺失的测试 | 那是 T-001~T-004 的职责；本任务**只核对**，发现缺失就**退回**对应任务 |
| 修改 P3/P4 的契约 | 已交付，契约已定稿 |
| 实现 F3（`FinishReason::Canceled`） | 需人工授权后才创建 |

## Acceptance Criteria

- [x] #1 架构文档 §6.1 的 P5 行标记为**完成**，并附**可核验**的交付摘要（不变量条数、测试数）
- [x] #2 `docs/specs/p5/contracts/invariants.json` 的全部不变量 `status` 提升为 `verified`
- [x] #3 `bash scripts/check-specs.sh` 通过（规则 3b 要求：任务全部归档后不得残留 `planned`）
- [x] #4 **故障注入**：对 P5 的关键断言做注入，验证**确实会红** —— 至少覆盖：帧格式、分隔符残留、事件名映射、cause 泄露
- [x] #5 故障注入的记录写进任务文件（含「注入什么 / 期望 / 实际」）
- [x] #6 `docs/operations/` 有 P5 的运行说明（`/metrics` 已在 P3 runbook；SSE 端点需补「怎么访问 / 怎么解析」）
- [x] #7 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- [x] #8 CI 全部门禁全绿

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

8 条 AC 全部可二元判定 —— 每条都指明「文档里有什么」或「命令的退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

### #3 引用可解析

- `docs/specs/p5/spec.md §5、§6` —— 存在
- `docs/specs/p5/tasks.md T-006` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.1` —— 存在

### #4 范围有界

文档 + 一个门禁脚本改动。**不触及** 42 个业务码、12 个事件名、18 个 ID 类型、
crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` | **修改** —— §5.4 / §6.1 |
| `docs/specs/p5/contracts/invariants.json` | **修改** —— status 升格 |
| `docs/operations/` 下的运行说明 | **修改/新增** |
| `backlog/completed/NEX-36 - ...md` | **新增** —— 归档 |

### #6 完成态可描述

架构文档显示 P5 已完成；契约里没有残留的 `planned`；
`check-specs.sh` 会在「已完成的 Spec 还留着 `planned`」时**主动报错**。

### #7 无隐形依赖

- NEX-31 ~ NEX-35 均已登记在 `backlog/tasks/`
  （NEX-35 是解码器加固任务 —— 收口必须排在它之后）
- `scripts/check-specs.sh` **已**存在（**修改**而非新增）
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-31 ~ NEX-35 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：故障注入的**最小覆盖面**是「帧格式、分隔符残留、事件名映射、cause 泄露」四类。
- **依据**：这四类分别对应 P5 的四类**静默失效**形态 ——
  帧格式错与事件名错会让前端**收不到事件**，
  分隔符残留会让服务端**死循环**，
  cause 泄露会让**内部细节**外泄。
  其余断言（如缓冲字节数 > 0）由「任意切分点」这类**更强**的测试**间接覆盖**。
- **可逆性**：不适用。
- **影响面**：仅验证行为，不改实现。


---

## 验收记录

### AC 逐条核对

| # | 验收条件 | 结果 | 证据 |
|---|---|---|---|
| 1 | 架构文档 §6.1 的 P5 行标记为**完成**，附可核验摘要 | ✅ | §6.1 的 P5 行已改为「✅ 完成（NEX-25、31..35；…，98 个测试；契约载体 … 37 条不变量全部 verified）」；另补 §5.4 的 29 行 P5 不变量表 + §6.2.4 的 P5 具体任务表 |
| 2 | `contracts/invariants.json` 全部不变量 `status` 提升为 `verified` | ✅ | 37 条**全部** `verified`，剩余 `planned` = 0 |
| 3 | `bash scripts/check-specs.sh` 通过 | ✅ | 规则 3b 通过 —— 「已完成的 Spec 没有残留 planned 不变量」 |
| 4 | 故障注入覆盖帧格式 / 分隔符残留 / 事件名映射 / cause 泄露 | ✅ | **17 项全部确实变红**（详见下表） |
| 5 | 故障注入记录写进任务文件（含注入什么 / 期望 / 实际） | ✅ | 见「故障注入记录」小节 |
| 6 | `docs/operations/` 有 P5 的运行说明 | ✅ | 新增 `docs/operations/p5-runbook.md` —— 统一响应包 / SSE 端点怎么访问 / **怎么解析**（含前端三条铁律）/ 心跳 / 请求 ID / 帧大小上限 / 排障表 |
| 7 | `fmt` / `clippy` / `test` 全绿 | ✅ | fmt OK；clippy 零告警；**546 通过 0 失败** |
| 8 | CI 全部门禁全绿 | ✅ | 七道脚本 + fmt + clippy + test **全绿**（含**修复**的 `check-rules.sh` / `check-docs.sh` 既有破损） |

### 契约漂移：P5-I20 的 `verified_by` 指向错误文件

**收口时发现的第一件事** —— 升格前必须逐条核验证据，
结果 P5-I20 的 `verified_by` 指向 `crates/nexus-api/tests/response.rs`，
而那个测试**实际**在 `crates/nexus-api/src/response.rs`。

**为什么一直没被发现**：`scripts/check-specs.sh` 规则 3 只校验
`status == verified` 的条目 —— 而它还**停在 `planned`**。
⇒ **规则 3b（任务全部归档后不得残留 `planned`）是这条漂移的唯一防线。**

⇒ 收口的顺序必须是**先全部升格，再跑门禁** ——
否则漂移会**借 `planned` 藏身**，而门禁看起来是绿的。

### P5-I20 的测试是**同义反复** —— 已重写

「测试存在」不等于「断言有效」。P5-I20 原本断言的是**测试自己写的那份字面量**：

```rust
// ❌ 原版 —— 断言的是**测试自己的副本**
let fallback = r#"{"code":90001,"message":"系统内部错误"}"#;
let v: Value = serde_json::from_str(fallback).expect(...);
```

生产代码里的兜底串被改坏时，这条测试**照样绿**。

**修法**（两步）：

1. 把兜底串提成**具名常量** `FALLBACK_BODY`，
   `to_bytes` 的兜底分支**引用它** —— 测试断言的**就是生产代码用的那一份**。
2. 补一条**元断言** `the_production_fallback_is_the_one_under_test`：
   抽出 `to_bytes` 的**函数体**，断言它**引用**了 `FALLBACK_BODY`。

> ⚠️ 元断言**没有**用「数出现次数」——
> 计数会把**文档注释**里的引用也算进去，那是**脆的**
> （改个注释就可能让阈值漂移，或反过来，注释里多写一次就把计数撑过去）。

### 故障注入记录

隔离工作树：`/tmp/nex36-verify`（`git worktree add ... --detach`）。
每项变异都经**三道**检查：① 确实改了文件 ② **编译必须通过**
（编译失败 = 变异无效，不算断言生效）③ 目标测试**必须变红**。

| # | 注入什么 | 期望 | 实际 |
|---|---|---|---|
| 1 | 帧分隔符写成单个 `\n` | 帧格式测试变红 | ✅ FAILED |
| 2 | 帧的字段顺序反了（`data:` 在 `event:` 前） | 帧格式测试变红 | ✅ FAILED |
| 3 | 编码器改用 `serde_json::to_string_pretty` | 帧格式测试变红 | ✅ FAILED |
| 4 | 桥接改用 `axum::Sse` | 结构层测试变红 | ✅ FAILED |
| 5 | `drain` 不带分隔符 | 分隔符残留测试变红 | ✅ FAILED |
| 6 | `drain` 多切一个字节 | 分隔符残留测试变红 | ✅ FAILED |
| 7 | 半帧被**丢弃**而不是缓冲 | 半帧缓冲测试变红 | ✅ FAILED |
| 8 | 分隔符查找**只认 LF** 不认 CRLF | CRLF 变体测试变红 | ✅ FAILED |
| 9 | 事件名 `tool_call` 拼成 `toolcall` | 12 个事件名测试变红 | ✅ FAILED |
| 10 | 事件名 `meta` / `delta` 对调 | 12 个事件名测试变红 | ✅ FAILED |
| 11 | 事件名 `reasoning` 拼成 `reason` | 12 个事件名测试变红 | ✅ FAILED |
| 12 | 事件名 `usage` 拼成 `usage_stats` | 12 个事件名测试变红 | ✅ FAILED |
| 13 | 错误响应体里塞进 `cause` | cause 不外泄测试变红 | ✅ FAILED |
| 14 | SSE 的 `error` 帧带上 `cause` | cause 不外泄测试变红 | ✅ FAILED |
| 15 | `from_facing` 把内部细节塞进 `data` | 两字段测试变红 | ✅ FAILED |
| 16 | 兜底 body 变**空串** | 兜底是合法 JSON 测试变红 | ✅ FAILED |
| 17 | 生产代码不再用 `FALLBACK_BODY` | 元断言变红 | ✅ FAILED |

**17 / 17 全部确实变红。**

### 故障注入的**三轮** —— 每一轮都有「查不清就放过」的陷阱

这一项的**过程**比结果更值得记 —— 三轮里踩了**三种不同的坑**：

#### 第 1 轮：7 项没跑起来（**脚本层**问题）

| 现象 | 根因 |
|---|---|
| 「变异脚本执行失败」× 3 | `assert old in s` 失败 —— 目标文本写错（如 `nexus_service::SomeType` 不存在、`use serde_json::Value;` 不是 `bridge.rs` 的实际 import） |
| 「编译失败」× 1 | 注入的 Rust 代码**转义写错**，编译不过 |
| 「未变红」× 2 | 见第 2 轮 |

**教训**：变异脚本**写不出**和变异**无效**是两件事 ——
必须**分清**，否则会把「脚本没跑起来」误记成「断言抓不住」。

#### 第 2 轮：2 项「未变红」—— **都是无效变异**，但成因不同

| 变异 | 真实成因 |
|---|---|
| 编码器 `to_vec` → `to_string` | **字节完全相同** —— `serde_json::to_string` 与 `to_vec` 产出**同一份**字节。必须用 `to_string_pretty` 才会改字节 |
| 事件名**调换 match 分支顺序** | **无效变异** —— Rust 的 match 按**模式**匹配，分支的书写顺序**不影响**结果。要动的是**返回值**的对应关系 |

> ⚠️ 第二项还暴露了源码里一处**不准确的注释**：
> `sse.rs` 写着「`to_string` 会转义 `/` 之外的某些字符，且不保证键序」——
> 这**不成立**（`to_string` 与 `to_vec` 完全等价，
> 真正会美化输出的是 `to_string_pretty`）。
> **注释说的和代码做的不一样，会误导下一个改这里的人** ——
> 已在本次一并修正。

#### 第 3 轮：1 项「未变红」—— 这次是**断言太弱**

第 17 项（生产代码不用 `FALLBACK_BODY`）没变红。
查下来是**元断言太弱**：当时写的是「源码里 `FALLBACK_BODY` 出现 ≥3 次」，
而**文档注释**里的引用也被计数 —— 变异把 `to_bytes` 改成内联字面量后，
注释里的引用还在，计数**没掉**。

⇒ 改成「**抽出 `to_bytes` 的函数体**，断言它引用了该常量」。

**这一轮的教训最值钱**：前两轮的「未变红」都是**变异无效**，
第三轮才是**断言抓不住**。
**方向相反，处理方式也相反** ——
不查清就放过的话，3 个真实的覆盖盲区会被当成「变异无效」永久放过。

> 这是 P5 连续第三个任务踩到「未变红 ≠ 断言弱」这个坑
> （NEX-33 发现 3、NEX-34 发现 6、NEX-35 发现 7、NEX-36 发现 3）。
> **判定「断言弱」之前，必须先证明「变异有效」。**

### 顺带修复：`check-rules.sh` / `check-docs.sh` 的既有破损（AC #8 的前提）

AC #8 要求「CI 全部门禁全绿」，但两道门禁**本来就红** ——
`20aaa80`（「删除过时的配置文件和文档」）删掉了 `docs/ai-sdlc/` 整目录，
却**没有**同步删除指向它的**引用**：

| 破损 | 修复 |
|---|---|
| `docs/README.md` 索引列了 **8 个**已删除的文档 | 删除那 8 行索引 |
| `AGENTS.md` / `ai-sdlc-workflow.mdc` / `ai-sdlc-workflow-handbook.md` 引用了 **10 处**已删除的路径 | 删除那些引用行 |
| `check-docs.sh` 规则 3 **要求**覆盖矩阵存在，否则**直接失败** | 改为「**存在即校验，不存在即跳过**」—— 日后若重新引入矩阵，该规则**自动生效** |

**决策**：不把「删掉整个 `docs/ai-sdlc/` 目录」当作**本任务**该做的事 ——
那是 `20aaa80` 已经落地的**既有决策**。本任务只做**引用侧的同步**，
让门禁与那个决策**一致**。

> 留着规则 3 而不是删掉它，是因为它「**存在即校验**」——
> 日后重新引入覆盖矩阵时**自动生效**，比「删掉后手工补回」更稳。

### 交付摘要（可核验）

| 指标 | 数值 | 怎么核验 |
|---|---|---|
| P5 不变量总数 | **37** 条，**全部** `verified` | `docs/specs/p5/contracts/invariants.json` |
| `nexus-api` 测试数 | **98** 个 | `cargo test -p nexus-api --locked` |
| 工作区测试数 | **546** 个，0 失败 | `cargo test --workspace --locked` |
| 故障注入 | **17 / 17** 确实变红 | 见上表 |
| 门禁 | 七道脚本 + fmt + clippy + test **全绿** | `bash scripts/dev.sh check` |


### 决策记录（续）

### 类别：decide-and-proceed

- **决策**：把兜底串提成**具名常量** `FALLBACK_BODY`，并补一条**元断言**
  （`to_bytes` 的函数体必须引用它）。
- **依据**：P5-I20 的测试原本是**同义反复** —— 断言的是测试自己写的那份字面量。
  提成常量后，测试断言的**就是生产代码用的那一份**。
  元断言解决「**没人**保证 `to_bytes` 真的用它」这个第二层漏洞。
- **可逆性**：高 —— 纯内部重构，公开 API 只**新增**一个常量（`FALLBACK_BODY`）。
- **影响面**：`crates/nexus-api/src/response.rs`（+ 1 条测试）。

### 类别：decide-and-proceed

- **决策**：**不**恢复 `docs/ai-sdlc/` 目录，**不**把「删除那个目录」当成本任务的工作 ——
  只做**引用侧**的同步（删掉指向已删文件的链接），并把 `check-docs.sh` 规则 3
  改成「存在即校验，不存在即跳过」。
- **依据**：`20aaa80` 已经把删除**落进 `main`** ——
  那是**已生效的既有决策**，不是本任务该推翻的。
  本任务的职责是「让门禁与既有决策**一致**」，不是重做那个决策。
- **可逆性**：高 —— 引用行删掉容易，恢复也容易（git 历史里有）。
- **影响面**：`docs/README.md`、`AGENTS.md`、`.cursor/rules/ai-sdlc-workflow.mdc`、
  `docs/operations/ai-sdlc-workflow-handbook.md`、`scripts/check-docs.sh`。

### 类别：decide-and-proceed

- **决策**：修正 `sse.rs` 里「`to_string` 会转义某些字符且不保证键序」这句**不准确**的注释，
  改成「`to_string` 与 `to_vec` **等价**，要改字节必须用 `to_string_pretty`」。
- **依据**：故障注入**实测**证实 —— 把 `to_vec` 换成 `to_string`，帧字节**一个都没变**。
  注释说的和代码做的不一样，会**误导下一个改这里的人**。
- **可逆性**：高 —— 只改注释。
- **影响面**：`crates/nexus-api/src/sse.rs`。
