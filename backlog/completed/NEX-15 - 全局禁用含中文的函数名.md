---
id: NEX-15
title: "fix(naming): 全局禁用含中文的函数名并接入 CI 门禁"
status: Completed
assignee: []
labels: [naming, tooling, ci]
priority: medium
dependencies:
  - NEX-3
references:
  - ".cursor/rules/rust-code-standards.mdc §11"
  - "scripts/check-naming.sh"
  - ".github/workflows/ci.yml"
  - "AGENTS.md §3"
---

## Description

**标识符不得包含中文** —— 这条规则此前没有独立的 DoR 载体，只是夹带在
NEX-3（分层配置骨架）的提交里落地：

| 内容 | 落地于 | 问题 |
|---|---|---|
| `scripts/check-naming.sh` 引入 | `4e5e53a`（NEX-3） | 提交信息里写的是「顺带（同一批门禁收紧）」，不是一项被规划的交付物 |
| 99 处中文函数名重命名 | `4e5e53a`（NEX-3） | 同上 —— 没有 AC，没有验收记录 |
| 正则加固（覆盖「英文前缀 + 中文在后」） | `d28156b`（无任务号） | 该提交的 Co-Authored-By 是 Cursor，**无任务号**，违反提交规范 |

仓库里已有一条更早的同类提交 `d28156b`，但它只改了 11 个函数名 ——
仓库中仍有大量中文函数名未被处理（`nexus-config` 的 28 处、
`examples/` 的 12 个 demo 的 60 余处）。也就是说：

- 规则文本（`rust-code-standards.mdc` §11）**已经存在**
- 校验脚本**已经存在**，但正则最初只匹配「中文开头」
- **重命名没有做干净** —— 门禁一旦真正跑起来就是红的

本任务把这三件事收口成一项**可验收、可追溯**的交付。

### 规则的边界（本任务明确要界定的）

| 范围 | 判定 | 理由 |
|---|---|---|
| Rust **函数名**（含 `async` / `unsafe` / `const` / `extern` 前缀） | **禁止** | Rust 允许非 ASCII 标识符（RFC 2457），但中文函数名在 rust-analyzer、终端 diff、`grep`、CI 日志里都难以检索与引用 |
| Rust **测试函数名** | **禁止**（同等严格） | 测试名是「断言的行为」的契约，中文说明应放在测试体上方的注释里 |
| TS/JS **函数名**（`function` / `const` / `let` / `var` / `class` 五种声明形式） | **禁止** | 中文标识符会破坏 `npm` 生态的大量工具链 |
| 注释、字符串字面量、文档注释 | **允许** | 中文解释「为什么」是本仓库的既定风格（§10） |
| 类型名 / 结构体字段 / 常量 | **本任务不覆盖** | 收口范围限定在「函数名」；类型与字段的中文名当前为 0，扩展需另开任务 |

### 关键陷阱：「英文前缀 + 中文在后」

早期版本的正则只匹配「中文开头」：

```
fn[[:space:]]+\p{Han}
```

于是 `fn sqlx_只出现在数据访问层()` 这类写法**长期漏检** ——
而它恰恰是规则 §11 明令禁止的形式（后半段仍是中文）。
正则必须匹配标识符**任意位置**含 Han 字符。

这个陷阱只有写进规则文档才会被记住，所以 §11 里必须有「常见误判」小节。

## Acceptance Criteria

- [x] #1 `.cursor/rules/rust-code-standards.mdc` §11 明确「标识符必须用 ASCII，中文只允许出现在注释、字符串字面量与文档注释」，并含「常见误判」小节（点名「英文前缀 + 中文在后」的写法）
- [x] #2 `scripts/check-naming.sh` 存在且可执行，正则覆盖标识符**任意位置**含 Han 字符（不只「中文开头」）
- [x] #3 脚本覆盖 Rust 的 `async` / `unsafe` / `const` / `extern` 前缀形式
- [x] #4 脚本覆盖 TS/JS 的 `function` / `const` / `let` / `var` / `class` 五种声明形式
- [x] #5 `crates/` 与 `examples/` 下**全部**中文函数名被重命名为 ASCII（脚本扫描 0 命中）
- [x] #6 重命名后的测试函数名用英文描述**断言的行为**；原中文名中英文名未覆盖的语义说明（如有）放在测试体上方注释里
- [x] #7 `bash scripts/check-naming.sh` 退出码为 0
- [x] #8 `bash scripts/check-naming.sh` 对含中文函数名的样例**返回非 0**（用探针验证：旧正则漏检的 `fn ast_校验不保证语义合理()` 必须被命中）
- [x] #9 `scripts/check-naming.sh` 已接入 `.github/workflows/ci.yml`（CI 拦截，非 advisory）
- [x] #10 `scripts/dev.sh audit` 串入命名检查，本地一条命令可跑全部门禁
- [x] #11 `AGENTS.md` §3 的门禁清单与 `.cursor/rules/ai-sdlc-workflow.mdc` §3 的可执行门禁清单均已含命名检查
- [x] #12 `cargo fmt --all --check` 通过
- [x] #13 `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- [x] #14 `cargo test --workspace --locked` 通过
- [x] #15 `cd examples && cargo test --workspace` 通过（独立 workspace，不参与主 workspace 的 clippy）
- [x] #16 CI 七道门禁全绿

## 备注

- **为什么重命名测试函数**：中文测试名的可读性优势，在 CI 日志里是**劣势** ——
  失败时输出 `test 解析非法_yaml报错 ... FAILED` 尚可，
  但 `test fn sqlx_只出现在数据访问层` 与 `test fn sqlx_is_only_in_data_access_layer`
  在日志聚合、趋势对比、跨仓库搜索上的差距是实质性的。
  测试名是**长期契约**，改名成本随时间指数上升。
- **重命名的风险面**：只改函数**定义位置**的标识符，不触碰注释与字符串字面量里的同名文本；
  `trybuild` 的 `.stderr` 快照若引用了函数名需同步核对。
  本次全量重命名的 18 个文件里没有 `.stderr` 快照受影响（快照只含编译器输出，不含测试函数名）。
- **`architecture.rs` 文件头的测试名索引表**需要与函数名**同步更新** ——
  该文件头部有一张「测试名 → 断言内容」的对照表，改名不改表就是**文档漂移**。
- **不要用 `sed` 批量替换**：中文标识符在 Rust 里是合法的 `IDENT`，
  粗暴替换会误伤注释与字符串。改名的可靠做法是**逐个函数、按定义位置精确替换**。
- 门禁等级：`hard-mandatory`（命名规范属契约类，绕过即视为违规）。
  若某处中文名确有保留必要，走**例外登记**流程而非放宽正则。
- 本任务**不改**架构文档的已定稿条款（错误码 / 事件名 / ID 类型 / crate 边界），
  因此不需要 RFC。

## 实现过程中的发现

### 发现 1：规则早已落地，但**没有被当成一项交付**

追查 `git log` 才发现，命名规则的三块内容其实都已存在于 `main`：

| 内容 | 落地提交 | 当时的表述 |
|---|---|---|
| `check-naming.sh` 引入 | `4e5e53a`（NEX-3） | 提交信息写「顺带（同一批门禁收紧）」 |
| 99 处中文函数名重命名 | `4e5e53a`（NEX-3） | 同上，无 AC |
| 正则加固 | `d28156b` | Co-Authored-By 是 Cursor，**无任务号** |

「无任务号」本身违反提交规范（`ai-sdlc-workflow.mdc` §8）——
但它之所以能合入，正是因为**没有任何机制检查提交是否带任务号**。

**教训**：门禁只能拦住「代码不满足规则」，拦不住「这件事根本没被规划」。
规则类交付必须有 DoR 载体，否则它会以「顺手做掉」的形式沉淀进历史。

### 发现 2：`d28156b` 只改了 11 个 —— 门禁一旦真跑就是红的

`d28156b` 加固了正则（能匹配「英文前缀 + 中文在后」），
但仓库里仍有 **90 余处**中文函数名未被处理：

| 位置 | 数量 |
|---|---|
| `crates/nexus-config/src/`（`env.rs` / `stack.rs` / `mapping.rs` / `loader.rs`） | 28 |
| `crates/nexus-config/tests/config_stack.rs` | 6 |
| `crates/nexus-core/tests/`（`architecture.rs` / `compile_fail.rs`） | 7 |
| `examples/` 12 个 demo | 60 |

也就是说**加固后的正则一进 CI 就是红的**。
这是「正则加固」与「存量重命名」必须**成对交付**的实证 ——
只加固不清理，等于把门禁变成红灯。

### 发现 3：重命名必须**按定义位置精确替换**，不能批量 sed

中文标识符在 Rust 里是**合法**的 `IDENT`，
`fn 名称` 既可能是函数定义，也可能是**注释里提到某个函数**，
还可能出现在**字符串字面量**（如文档示例）里。

用 `sed 's/中文名/english_name/g'` 会误伤后两者。
可靠做法是**逐个函数、按定义位置精确替换**：
匹配 `fn\s+<中文名>` 边界，且只替换定义处。

### 发现 4：`architecture.rs` 文件头有**测试名索引表** —— 改名必须同步

`crates/nexus-core/tests/architecture.rs` 头部有一张
「测试名 → 检验的设计条款」对照表：

```text
//! | `capability_layer_does_not_depend_on_service_api_cli` | §4.4 规则 1 |
```

改名不改表就是**文档漂移** —— 表里的名字指向一个已不存在的测试。
这类「表格式索引」是重命名时最容易漏掉的同步点。

### 发现 5：AC #6 的表述需要**去掉歧义**

原 AC 写「中文语义说明放在测试体上方注释里」，隐含「每个测试都要补一行中文注释」。

但逐个核验后发现**不需要补**：中文名 → 英文名的映射已**完整覆盖**语义
（如 `拒绝_cte_里的写操作` → `rejects_write_inside_cte`），
英文名自解释。此时再加 `// 断言：拒绝 CTE 里的写操作` 反而**违反 §10**
（「注释用中文解释『为什么』，不解释『做了什么』」）。

已把 AC #6 改为无歧义表述：「英文名未覆盖的语义说明（如有）放在注释里」。

### 发现 6：门禁脚本本身要用**探针**证伪

「脚本返回 0」不能证明脚本有效 —— 它可能只是**什么都没匹配**。

验证办法：造一个含中文函数名的样例文件放进 `crates/nexus-core/tests/`，
断言 `check-naming.sh` 退出码为 **1**，移除后回到 **0**。已实测通过。

探针内容刻意选了**旧正则漏检**的形式：

```rust
fn ast_校验不保证语义合理() {}   // 英文前缀 + 中文在后
fn 纯中文开头() {}               // 中文开头
fn ok_pure_english() {}          // 纯英文 —— 不得误报
```

第二个探针（纯英文不误报）同样重要 ——
正则写得太宽会把 `fn 中` 之后所有内容都吞掉。

### 落点

| 文件 | 内容 |
|---|---|
| `.cursor/rules/rust-code-standards.mdc` §11 | 规则正文 + 「常见误判」小节 |
| `scripts/check-naming.sh` | 校验脚本（CI 拦截） |
| `.github/workflows/ci.yml` | 第 70 行接入 |
| `scripts/dev.sh` | `audit` 子命令串入 |
| `AGENTS.md` §3 / `.cursor/rules/ai-sdlc-workflow.mdc` §3 | 门禁清单同步 |
| `crates/` 5 个文件 + `examples/` 13 个文件 | 98 处函数名重命名 |

### 未做的部分（明确划界）

| 项 | 原因 |
|---|---|
| **类型名 / 结构体字段 / 常量名** | 本任务范围限定「函数名」。当前这类的中文名为 0，扩展需另开任务并同步放宽/收紧正则 |
| **中文注释的自动检测** | 中文注释是**允许且鼓励**的（§10），规则只约束标识符，不约束注释 |
| **提交信息带任务号的强制校验** | 发现 1 指出这是个真实缺口，但需要新的 `check-commit-msg` 门禁 + git hook，属独立任务 |
| **`.stderr` 快照的联动更新** | 18 个被改文件里没有 `trybuild` 快照受影响（快照只含编译器输出，不含测试函数名）；`d28156b` 改过 `compile_fail.rs`，已核验其 `.stderr` 未变 |

