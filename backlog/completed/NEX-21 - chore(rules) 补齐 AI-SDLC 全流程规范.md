---
id: NEX-21
title: "chore(rules): 补齐 AI-SDLC 全流程规范"
status: Done
assignee: []
labels: [ai-sdlc, governance, docs]
priority: high
dependencies: []
references:
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §6.3"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/spec.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/policy.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/autonomy.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/agents.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/adapters.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/metrics.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0011-definition-of-ready-gate.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0012-two-tier-pipeline-architecture.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0035-decision-catalog-operator-routing.md"
  - "~/.claude/plugins/marketplaces/ai-sdlc/spec/rfcs/RFC-0052-staged-review-pipeline.md"
---

## Description

本仓库已有 `.cursor/rules/ai-sdlc-workflow.mdc`，但它只覆盖了 AI-SDLC 的**局部**：

- 覆盖到的：任务分层、DoR 七项门禁、质量门禁分级、决策三分类、五步法、Git Flow。
- **未覆盖的**：完整的开发生命周期阶段定义、每阶段**进入条件 / 退出条件 / 产出物 / 失败处理**、
  评审门禁的完整语义（staged review / provenance / reviewer 独立性）、
  自治等级的晋级与降级、Agent 角色与 handoff 契约、适配器与外部系统边界、
  指标与可观测性维度、交付后运维闭环。

结果：代理在「跨阶段接力」时缺少逐阶段的判据 —— 例如「实现完成但评审未过」该做什么、
「评审提出 critical 发现但改动触及已定稿契约」该升级还是自行修复、
「什么算 hard-mandatory」只有结论没有推导过程。

**目标**：把 AI-SDLC 的规范**完整落到本仓库的载体上**，覆盖从需求到运维的**全部软件开发流程**，
每一条都给出可执行判据，而不是只有原则性表述。

**来源**：本机 AI-SDLC 插件仓库 `~/.claude/plugins/marketplaces/ai-sdlc/`
（`spec/` 规范性文档、`docs/concepts/` 解释性文档、`spec/rfcs/` RFC）。
本仓库**尚未安装** AI-SDLC 管道（无 `.ai-sdlc/`、`ai-sdlc` CLI 不在 PATH），
因此本次交付是**方法论适配**：条款语义严格照搬 AI-SDLC，落地载体为本仓库的
`.cursor/rules/`、`backlog/`、`scripts/`、`docs/`。

## DoR 判定

- 门禁 1（AC 可二元测试）：通过 —— 12 条 AC，均可写成命令或断言
- 门禁 2（无未决标记）：通过
- 门禁 3（引用可解析）：通过 —— 引用的 AI-SDLC 规范文件均在本机插件仓库中存在；
      本仓库路径由 `scripts/check-rules.sh` 自动校验
- 门禁 4（范围有界）：通过 —— 只改 `.cursor/rules/`、`AGENTS.md`、`scripts/`、`docs/`、`backlog/README.md`、CI 配置，不含 Rust 代码
- 门禁 5（受影响面被指名）：通过 —— `.cursor/rules/*.mdc`、`AGENTS.md`、`scripts/check-rules.sh`、`.github/workflows/ci.yml`、`docs/operations/`、`docs/rfcs/`、`backlog/README.md`
- 门禁 6（完成态可描述）：通过 —— 代理读到任何一条流程规则时，能知道该阶段的判据、产出物与失败处理
- 门禁 7（无隐形依赖）：通过 —— 无上游任务依赖

**结论：pass**

## Acceptance Criteria

- [x] #1 `.cursor/rules/` 下有**一份**覆盖全流程的规则集合，每份规则文件只承载一个主题
- [x] #2 全流程阶段被显式列出，每个阶段都有**进入条件 / 退出条件 / 产出物 / 失败处理**四要素
- [x] #3 规则集合覆盖：分层与产出物、DoR 门禁、实现、验证、评审、合并交付、运维与度量
- [x] #4 规则集合覆盖：决策权威与升级、OQ 解决禁令、评审门禁（含 provenance 与 AI 额外评审人）、
      自治等级与路由、Agent 角色与 handoff 契约、适配器边界、指标维度
- [x] #5 每条规范都给出**可执行判据**（命令、正则、可数阈值），禁止只有原则性表述
- [x] #6 规则与 `AGENTS.md` 的权威顺序一致，**无冲突**；`AGENTS.md` 列出全部规则文件的职责
- [x] #7 规则中引用的**本仓库路径全部真实存在**（脚本、文档、目录）—— 有一条 `check-rules.sh` 校验
- [x] #8 `bash scripts/check-rules.sh` 通过（校验规则文件引用的路径存在、frontmatter 合法）
- [x] #9 `bash scripts/check-rules.sh` 被接入 `.github/workflows/ci.yml`
- [x] #10 新增规则文件被 `.cursor/rules/` 收录，且 `AGENTS.md` 的规则清单与实际文件**一一对应**
- [x] #11 `docs/` 下有一份**人类可读的流程手册**，与规则文件不矛盾（规则是判据，手册是叙事）
- [x] #12 `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、
      `cargo test --workspace --locked` 全部通过（本次改动不含 Rust 代码，应零影响但仍须验证）

## 备注

- 本任务**不改**任何 Rust 代码，不改错误码 / 事件名 / ID 类型，不改 crate 边界。
- 本任务**不改**架构文档的已定稿条款（§6.3）—— 只补流程侧载体。
- 「全部软件开发流程」指 AI-SDLC 视角下的**完整生命周期**：
  需求捕获 → 分层与产出物 → DoR → 实现 → 三层验证 → 评审 → 合并与交付 → 部署与运维 → 度量与校准。
  它**不等于**「每个阶段都要新建一个规则文件」—— 文件粒度按主题划分，见 AC #1。

## 实现过程中的发现

### 1. 「规则引用了不存在的路径」是一种**静默失效**

规则文件里写「见 `docs/xxx.md`」，而那个文件不存在 —— **没有任何东西会报错**。
代理读到这条规则时会去找，找不到就跳过，然后**以为这条已经被覆盖了**。

这是最危险的一类失效：**看起来覆盖了，实际没有**。
比「没有这条规则」更糟，因为「没有规则」至少是可见的缺口。

因此必须有 `scripts/check-rules.sh` —— 把规则的引用变成**可执行的断言**。
这与 `check-specs.sh` 规则 3（契约引用的测试必须真的存在）是同一条道理：
**契约（规则、invariant）里指向具体产物（文件、测试）的引用，必须被机器校验存在性。**

### 2. 门禁脚本本身必须被**验证过会红**

`check-rules.sh` 写完后，做了三个注入故障的负向测试：

| 注入的故障 | 期望 | 实际 |
|---|---|---|
| 规则里引用 `scripts/does-not-exist.sh` | 红 | ✓ 红，报出文件名与路径 |
| `AGENTS.md` 漏列一个规则文件 | 红 | ✓ 红，指出「存在于 `.cursor/rules/` 但未出现在清单中」 |
| `AGENTS.md` 列了一个不存在的规则文件 | 红 | ✓ 红，指出「清单里的 `ghost-rule.mdc` 不存在」 |

**没验证过会红的门禁，等于没有门禁** —— 沿用 NEX-14 的教训。

### 3. bash 的多字节变量名陷阱

`echo "$name：xxx"` 里，`$name` 紧跟中文全角冒号时，bash 会把冒号的**首字节**
并入变量名，导致 `unbound variable` 错误。

修法：`${name}：xxx` —— 显式界定变量名边界。
这类问题在**中文输出**的脚本里特别容易出现，因为中文标点的首字节
与 ASCII 标识符字符的判定区间重叠。

### 4. `read -rf` 不是合法选项

`read -r` 是「不转义反斜杠」，`-f` 不是 bash `read` 的选项（那是ksh 的）。
写负向测试时顺手用了 `read -rf`，报 `invalid option` 但**脚本继续跑**了 ——
因为错误发生在**子 shell** 的 `while read` 里。

**这暴露了一个更值得注意的点**：门禁脚本里的计数如果在子 shell 里累加，
**计数不会回流到父 shell**。
第一版的 `path_bad` 计数就踩了这个坑（永远为 0，门禁形同虚设）。
修法：把失败项写进临时文件，父 shell 再数行数。

> 教训：**门禁脚本自身的计数逻辑必须被负向测试覆盖** ——
> 一个永远报「通过」的门禁，比没有门禁更危险。

