# NexusAgent · 改进建议与实施计划

> 生成日期：2026-11-06
> 状态：**建议稿**。本文档只做两件事——**陈述发现**、**给出计划**。
> 它**不**决定任何事：所有需要操作者裁决的问题都登记在 §6，等人回答。
>
> 基线：`c37d61e`（`feature/p5-protocol-layer`）
> 方法与证据口径见 §3；本轮**未运行**的验证见 §3.3。

---

## 0. 这份文档怎么用

### 0.1 读者契约

| 你是 | 从这里得到什么 |
|---|---|
| **贡献者**（要动手改代码） | §5 的波次与 §4 的登记条目：先做哪一件、验收标准是什么 |
| **操作者**（要拍板） | §6 的待决策清单 + §7 风险登记；每条都有选项与建议 |
| **评审者**（要验一次修复） | §4 每条发现带的 **falsifier**（证伪条件）与 **验收方式** |
| **AI 代理**（要领活） | §4 的 `处置` 列 + §5 的波次；**不要**自行扩范围 |

### 0.2 这份文档**不是**什么

- **不是**决议。§6 的每条都还没有结论。
- **不是**任务清单。任务在 `backlog/tasks/`，由 DoR 门禁管；本文档只说「为什么值得做」和「证据是什么」。
- **不是**新架构。已定稿条款（42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向）**一条都不动**。

### 0.3 修订记录

| 日期 | 改了什么 | 触发 |
|---|---|---|
| 2026-11-06 | 首版 | 本次架构评估 |

### 0.4 复审触发条件

出现下列任一情况，**必须重做**本次评估（而不是在本文档上打补丁）：

- P6 落地完成（`nexus-execution` / `nexus-workflow` 从空壳变成实装）——§4 的严重度基线会整体漂移。
- 架构文档 §6.3 的任何已定稿条款被修订。
- §6 的任一待决策被裁决——对应波次要重排。
- 距今超过 3 个月。

---

## 1. 结论摘要

**判决**：代码的**架构方向**是对的——依赖方向、类型化契约、取消语义、错误分类都按设计落地了，
`scripts/` 下的七道门禁也真的在跑。**问题不在设计，在「门禁声称覆盖的东西比实际覆盖的多」。**

本轮评估发现的最要命的一条：

> **门禁全绿 ≠ 断言有效。**
> 三条独立的机制让「看起来被守住的不变量」实际处于无人看守的状态：
> 一个会 panic 的解码器、一套手抄三份且**各自失效方式不同**的注释剥离器、
> 一条用 `sed` 剥注释因而会**吞掉真实代码**的错误码扫描。

按严重度排序的前 5 条：

| # | 发现 | 严重度 | 类型 |
|---|---|---|---|
| **F-01** | `FrameDecoder` 在畸形帧之后 **panic** | critical | 缺陷 |
| **F-02** | 三份手抄的注释剥离器**各自**有不同失效方式；其中一份会**漏报** | critical | 缺陷 |
| **F-03** | `check-specs.sh` 规则 5 用 `sed` 剥注释，会**吞掉整行真实代码** | critical | 缺陷 |
| **F-04** | `nexus-api` 声明 9 条内部依赖，**6 条在 `src/` 里一次都没用到** | high | 缺陷 |
| **F-05** | P6 契约的 `crate` 字段单值，错误码扫描**漏扫 `nexus-workflow`** | high | 缺陷 |

> ⚠️ **关于严重度的诚实声明**：
> F-01 目前**没有生产调用方**——`FrameDecoder` 只被 `crates/nexus-api/src/sse.rs` 自己的
> 单元测试和 `lib.rs` 的 re-export 引用；`bridge.rs` 走的是 `encode_frame`（编码方向）。
> 它是**已发布 API**（`pub use`），但当前不会被线上流量触发。
> 之所以仍评 critical：它是 `pub` 契约的一部分，且这是**状态转换缺口**——
> 不是「输入空间没测全」，是「错了之后状态机就不自洽了」。
> 修它的成本很低（见 §4 F-01），而留着它的成本是「下一个调用方踩到 panic」。

---

## 2. 现状基线

### 2.1 规模

| 维度 | 数字 |
|---|---|
| crate 总数 | 22 个成员 |
| **实装** crate | 10 个（`core` / `api` / `agent` / `llm` / `repo` / `infra` / `observability` / `config` / `model` / `cli`） |
| **空壳** crate | **12 个**（`service` / `execution` / `workflow` / `security` / `rag` / `memory` / `tool` / `mcp` / `a2a` / `capacity` / `ingestion` / `vector`） |
| `src/` 总行数 | 19,151 行 |
| 单元测试 | `nexus-api` 48 条（其中 `sse.rs` 27 条） |
| `examples/` 测试 | 61 条（**独立 workspace，CI 不跑**） |
| 架构文档 | 2,422 行（单文件） |

### 2.2 已完成阶段

P0（骨架）→ P5（协议层）已完成；**P6（工作流层）刚立项**（`docs/specs/p6/`，51 条不变量全部 `planned`）。

### 2.3 门禁现状（实测全绿）

```
check-lock ✓  check-deps ✓  check-naming ✓  check-backlog ✓
check-specs ✓  check-rules ✓  check-docs ✓   cargo test --workspace ✓
```

> **这份全绿正是本轮评估的起点**——
> 下面 §4 的多数发现，都是「门禁绿着，但被守的东西没被守住」。
> 架构文档 §5.4 的不变量表已经列了 150+ 条「✅ 已验证」，
> 但**「已验证」的判定标准是「测试存在且绿」，不是「断言有效」**。
> 本仓库自己的教训写在 NEX-36 收口记录里：
> 「**测试存在 ≠ 断言有效**」——断言的是测试自己写的那份字面量（同义反复）。
> 本轮的 F-02 / F-03 是那条教训的**新实例**，而且发生在**结构层**而不是行为层。

---

## 3. 判定方法

### 3.1 「已验证」在本仓库的定义

| 级别 | 判据 | 本文档是否使用 |
|---|---|---|
| **L1 可复现** | 有可执行的复现步骤，且**本次实测复现** | ✅ 主用 |
| **L2 可核对** | 引用具体文件与行号，可静态核对 | ✅ 用于文档/契约类 |
| **L3 推演** | 由代码逻辑推出，未实测 | ⚠️ 明确标注 |

### 3.2 证据分级

每条发现带一个 **falsifier**（证伪条件）：**「如果执行 X 而现象不出现，本条不成立。」**

这不是形式主义。本轮至少有 3 条候选发现因为**证伪成功**而被降级或修正（见 §3.4）。

### 3.3 本轮**未运行**的验证

| 未跑 | 影响 |
|---|---|
| PostgreSQL 集成测试（本机无 `DATABASE_URL`） | 涉及 `nexus-repo` / `nexus-infra` 的结论仅静态核对 |
| `examples/` workspace（`cd examples && cargo test`） | 61 个测试的实际状态未复验 |
| `cargo deny` / `cargo audit` / 渗透测试 | 安全类结论不覆盖 |

### 3.4 本轮**修正过**的判断（自我证伪记录）

诚实记录比「看起来全对」重要。以下是评估过程中被**自己的证据推翻**的判断：

| 初始判断 | 实测结果 | 处置 |
|---|---|---|
| 注释剥离器**同时**造成假阳性与漏报 | **按副本而异**：api 副本只假阳性；agent 副本只漏报（`"http://x"` 吞掉行尾代码）；core 副本两者都不是，但遇字符字面量 `'"'` 会假阳性 | 拆成三条独立发现（F-02a/b/c） |
| `nexus-repo` 持有 `workflow_runs`，架构测试「应该红」 | **不应红**——`nexus-repo/src` 里 `workflow_runs` **全部出现在注释里**，实际表是 `execution_runs`（`leases.rs:19` 明确写了「不是 `workflow_runs`」） | 撤回该条；真正的未决项是 `execution_*` 的归属（§6 D-01） |
| `sse.rs` 有 34 条单元测试 | **27 条**（`grep -c '#\[test\]'`） | 全文更正 |
| 有 13 个空壳 crate | **12 个** | 全文更正 |
| `nexus-api` 的依赖门禁「只扫 `src/`，从不读 `Cargo.toml`」 | **错**——`the_api_crate_adds_no_internal_dependencies` **确实读 manifest**，且有 9 条依赖的 allowlist | 改为「allowlist **无条款依据**」，修法从「加检查」改为「收紧既有 allowlist」 |

---

## 4. 发现登记

### 4.0 根因主题

四条跨条目的主题。**没有它们，这 6 条发现会退化成一个扁平 bug 列表**，
读者修完一条就忘了下一条的共同成因。

| 主题 | 含义 | 涉及发现 |
|---|---|---|
| **T1 门禁检查了错误的表面** | 断言存在，但它检查的东西不是不变量所在的地方 | F-02、F-03、F-05 |
| **T2 同一知识手抄多份** | 一份知识 N 个副本，无同步机制，副本各自漂移 | F-02、F-06 |
| **T3 文档承载了进度** | 文档里写着「尚未完成」，而事实早已相反 | F-07 |
| **T4 权威文档内部矛盾** | 唯一权威的文档自己跟自己冲突 | F-08 |

### 4.1 缺陷（已复现）

---

#### F-01 · `FrameDecoder` 在畸形帧之后 panic

- **类型**：缺陷 ｜ **严重度**：critical ｜ **证据级别**：L1 可复现
- **落点**：`crates/nexus-api/src/sse.rs`

**现象**

```text
thread '...' panicked at crates/nexus-api/src/sse.rs:259:66:
range start index 97 out of range for slice of length 1
```

**复现**（`catch_unwind` 实测确认 panic）：

```rust
let mut d = FrameDecoder::new();
d.push(&[b'x'; 100]).unwrap();   // ① 无分隔符 → scanned = 97
let r = d.push(b"ab\n\n");        // ② 畸形帧 → Err，但缓冲区已被 drain 到 1 字节
assert!(r.is_err());
let r = catch_unwind(|| d.push(b"z"));  // ③ → panic: &self.buf[97..] 越界
```

**根因**

`push` 用 `self.scanned` 缓存「上次扫过的字节数」：

```rust
let mut scan_from = self.scanned;
// ...
let Some((pos, delim_len)) = find_delimiter(&self.buf[scan_from..]) else { ... };
// ...
let mut raw: Vec<u8> = self.buf.drain(..pos + delim_len).collect();
// ...
let Some(frame) = parse_frame(&raw)? else {   // ← `?` 提前返回
    continue;
};
```

`scan_from` 在**成功**路径上每轮重置为 0，但 `parse_frame` 失败时 `?` **提前返回**，
`scan_from` 仍是上一轮的旧值（97），而缓冲区已被 `drain` 到只剩 1 字节。
**下一次** `push` 拿旧游标去切新缓冲区 → 越界。

**为什么 27 条解码器测试全绿**

测试覆盖了「畸形帧被拒绝」，但**没有覆盖「拒绝之后继续喂」**。
这是**状态转换缺口**，不是输入空间缺口——补再多畸形输入也测不出来。

**建议改法**（`not implemented` 草图）

把「扫描游标」从**跨调用缓存**改成**状态机的一部分**，让错误态显式：

```rust
enum DecoderState {
    /// 正常：scanned 是「上次扫过的字节数」。
    Ready { scanned: usize },
    /// 已中毒：一旦畸形，后续 push 永远返回同一个错误。
    Poisoned(FrameError),
}

pub struct FrameDecoder {
    buf: Vec<u8>,
    limit: usize,
    state: DecoderState,   // ← 不再有独立的 `scanned` 字段
}

impl FrameDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        // TODO: Poisoned → 立刻返回同一错误，不动缓冲区
        // TODO: Ready → 扫描；遇畸形帧 → Poisoned(err) 并返回 err
        // TODO: 任何路径都不得让游标与 buf.len() 失同步
        not implemented
    }
}
```

要点：**`scanned` 失效的根因是「游标与缓冲区长度之间没有不变量」。**
状态机把「畸形帧之后解码器进入显式错误态」变成**类型可见**的事实——
调用方不必知道内部发生过 drain。

**验收**

| 项 | 要求 |
|---|---|
| 正向 | 上述三步序列**不 panic**，第 3 步返回 `Err` |
| 状态语义 | 畸形帧之后，后续 `push` 返回**同一个**错误（幂等） |
| 边界 | 缓冲区不再增长；`limit` 仍生效 |
| **负控** | 把游标重置移回 `parse_frame` 之后（恢复旧行为）→ 该测试**必须红** |

**falsifier**：在 `c37d61e` 上按上述三步执行而**不 panic** → 本条不成立。

---

#### F-02 · 三份手抄的注释剥离器，各自失效方式不同

- **类型**：缺陷 ｜ **严重度**：critical ｜ **证据级别**：L1 可复现
- **落点**：
  - `crates/nexus-api/tests/architecture.rs:39-103`
  - `crates/nexus-agent/tests/events.rs:264-294`
  - `crates/nexus-core/tests/architecture.rs:451-500`

**现象**

同一份「剥离注释」的逻辑手抄了 **3 份**，且**三份的行为不一样**。
把它们抽出来跑同一组对抗输入，实测结果：

| 对抗输入 | api 副本 | agent 副本 | core 副本 |
|---|---|---|---|
| **假阳性**：注释里含违规标记 | ✅ 正确剥离 | ✅ 正确剥离 | ✅ 正确剥离 |
| **假阳性**：`&'static` **之后**的注释 | ❌ **漏剥**（标记残留 → 断言误红） | ✅ 正确剥离 | ✅ 正确剥离 |
| **漏报**：字符串 `"http://x"` 后的真实代码 | ✅ 保留 | ❌ **吞掉整行**（真实违规消失） | ✅ 保留 |
| **假阳性**：字符字面量 `'"'` 之后的注释 | ✅ 正确剥离 | ✅ 正确剥离 | ❌ **漏剥**（标记残留 → 断言误红） |

**三个副本的机制各不相同**

- **api 副本**：有 `in_char` 状态，但 `&'static str` 的生命周期 `'` **只开不关**
  （源码里生命周期后面没有配对的 `'`）→ 从该点起**几乎全部文本不再被剥注释**。
  实测：`bridge.rs` 首个 `'` 在偏移 2274，其后 8800 字符里 **67 处注释标记原样保留**。
  在那段区域写一句注释「参考 `axum::response::sse` 会自动转义 data:」，
  `the_api_crate_does_not_use_the_axum_sse_type` **立刻变红**（实测复现）。

- **agent 副本**：**完全没有字符串状态**。`"http://x"` 里的 `//` 被当成行注释，
  **该行之后的真实代码被丢弃**。实测：`fn f() { let u = "http://x"; REAL_VIOLATION(); }`
  → 剥离后 `REAL_VIOLATION` **消失**。
  ⇒ **这是真漏报**：一个真实的违规可以靠「在字符串里放个 `//`」藏起来。

- **core 副本**：没有 `in_char` 状态 → 字符字面量 `'"'` 会**打开**字符串状态，
  其后的注释不再被剥。

**为什么这比「手抄三份」本身更严重**

这三份是**结构层断言的底层工具**。结构层断言是本仓库「让非法状态不可表示」的
主要防线之一（架构文档 §5.3 列了 16 项）。底层扫描器漏报，**上层断言全部失效**，
而门禁**照样绿**。

**建议改法**

**不要修这三个手写词法器**——删掉它们。

```rust
// crates/nexus-core/tests/support/strip.rs（示意）
// TODO: 用 syn 解析成 token 流，只扫 token，不扫原始文本。
// TODO: 注释与字符串是 token，不是「看起来像注释的文本」——
//       这正是手写词法器会错的地方。
pub fn code_tokens(source: &str) -> Vec<syn::TokenStream> { not implemented }

// TODO: 在三个 crate 的架构测试里共用这一份实现。
```

`syn` 是 dev-dependency，不影响生产依赖方向。
若评审不接受新增 dev-dependency，退回方案是**单一实现 + 对抗语料**，
但必须补齐 raw string / 字符字面量 / 嵌套块注释 / 字符串含 `/*` 四类用例。

**验收**

| 项 | 要求 |
|---|---|
| 反例 1 | `&'static str` 之后的注释被正确剥离（api 副本今天的失败点） |
| 反例 2 | `"http://x"` 之后的真实代码**保留**（agent 副本今天的失败点） |
| 反例 3 | 字符字面量 `'"'` 之后的注释被正确剥离（core 副本今天的失败点） |
| 反例 4 | 字符串内含 `/*` 不被当成块注释开始 |
| **负控** | 任一反例写错时该测试**必须红** |
| 全仓 | 修正后**全仓复扫**：每条新暴露的违规**要么修、要么登记为新 finding**，**不得**靠放宽断言变绿 |

**falsifier**：三份副本在上述四组输入上的行为**一致且正确** → 本条不成立。

---

#### F-03 · `check-specs.sh` 规则 5 用 `sed` 剥注释，会吞掉真实代码

- **类型**：缺陷 ｜ **严重度**：critical ｜ **证据级别**：L1 可复现
- **落点**：`scripts/check-specs.sh:338`、`scripts/check-deps.sh:155`

**现象**

两处门禁都用同一条命令剥注释：

```bash
find "$crate/src" -name '*.rs' -type f -print0 | xargs -0 sed -e 's://.*::' | grep -ohE 'ErrorCode::[A-Za-z]+'
```

`sed` 的 `s://.*::` 是**纯文本**替换，**不理解字符串**。
一行里有 `"http://x"` 时，`//` 之后的内容**整行被丢弃**。

实测：

```console
$ printf 'let u = "http://x"; ErrorCode::Forbidden;\n' | sed -e 's://.*::'
let u = "http:
```

**在真实源码上的影响**

| 文件 | 真实 `ErrorCode::` 变体数 | `sed` 之后 | 差 |
|---|---|---|---|
| `crates/nexus-core/src/error.rs` | 43 | 42 | 1 被吞 |
| `crates/nexus-llm/src/provider.rs` | 1 | **0** | **全部被吞** |
| `crates/nexus-api/src/response.rs` | 2 | 2 | 0 |

被吞的具体内容（`provider.rs`）：

```rust
//! - 响应体解析失败 → `Malformed` → `ErrorCode::Internal`（**Terminal**，不重试）
```

—— 这三行**恰好都在注释里**，所以**目前**没有真实违规被吞。
但这是**运气**，不是防线：**一行真实代码只要在同一行前面出现过 `//`（哪怕在字符串里），
它使用的错误码就完全不进门禁的视野**。

**为什么这条是 critical**

规则 5 的职责是「新增码值 = 破坏性变更，必须被拦住」。
它是**契约破坏的唯一防线**——业务码表已定稿（架构文档 §6.3），
一旦有人绕过它新增 `ErrorCode` 变体，**没有任何东西会红**。

**建议改法**

用 `syn` 做同样的事（与 F-02 同一个工具，同一份实现）：

```rust
// TODO: 遍历 token 流，取出 ErrorCode::Xxx 的标识符路径。
// TODO: 字符串与注释在 token 层是**不同的 token**——
//       「看起来像注释的文本」在 token 层不存在。
pub fn used_error_codes(crate_src: &Path) -> BTreeSet<String> { not implemented }
```

**验收**

| 项 | 要求 |
|---|---|
| 反例 | 一行内含 `"http://x"` **且**使用 `ErrorCode::Forbidden` → 该变体**必须**被检出 |
| 等价性 | `syn` 版与 `sed` 版在**当前**全部真实源码上给出**相同**的变体集合（迁移期可比对） |
| **负控** | 往任一 Spec 的 crate 加一个白名单外的 `ErrorCode::Forbidden` → 门禁**必须红** |

**falsifier**：在真实源码上，`sed` 版与正确实现给出的变体集合**完全一致**，
且构造「同一行含 `//` 与白名单外码」的样本能让门禁变红 → 本条不成立。

---

#### F-04 · `nexus-api` 声明 9 条内部依赖，6 条一次都没用到

- **类型**：缺陷 ｜ **严重度**：high ｜ **证据级别**：L1 可复现
- **落点**：`crates/nexus-api/Cargo.toml`、`crates/nexus-api/tests/architecture.rs:170-212`

**事实**（逐 crate grep `crates/nexus-api/src`）：

| 依赖 | 在 `src/` 中的使用 |
|---|---|
| `nexus-core` | ✅ 4 个文件 |
| `nexus-observability` | ✅ 1 个文件（`metrics.rs`） |
| `nexus-agent` | ✅ 2 个文件（`bridge.rs` 借事件流**类型**） |
| `nexus-config` | ❌ **0** |
| `nexus-model` | ❌ **0** |
| `nexus-security` | ❌ **0** |
| `nexus-service` | ❌ **0** |
| `nexus-execution` | ❌ **0** |
| `nexus-llm` | ❌ **0** |

**为什么「没用」是问题**

架构文档 §4.1.1（2026-11-05 修正）确立的判据是「**类型可以借，语义不能借**」——
协议层依赖执行层的**只读数据类型**是允许的。
但「**一个类型都没借**，只留下一条依赖边」不是「借了类型」，是**纯粹的依赖噪音**。

更要紧的是 `nexus-service`：架构测试 `the_api_crate_does_not_use_the_service_crate`
只扫 `src/` 的**标识符**，**不查 manifest**。依赖已在，闸门只是没检查。
一旦有人写 `use nexus_service::...`，那条断言会红——但那时已经晚了一步。

**建议改法**（**分两类，不能一刀切**）

| 依赖 | 处置 | 依据 |
|---|---|---|
| `nexus-config` / `nexus-model` / `nexus-security` / `nexus-llm` | **删除** | 零引用，且不在 §4.1 crate 图的 `API →` 出边上 |
| `nexus-service` | **保留** | §4.1 crate 图第 1327 行明确 `API --> SVC`；`check-deps.sh:42` 的 `deps_of nexus-api` 也列了它。这是**已授权边** |
| `nexus-execution` | **保留** | §4.1.1 的修正明确承认这条边（`nexus-api` 依赖 `nexus-execution`），且 `check-deps.sh` 一直允许 |

> ⚠️ **删除 `nexus-service` / `nexus-execution` 会与已定稿目标态冲突。**
> 本轮评估中曾有一个候选方案主张「删掉全部 6 条」——
> 那是错的，会让协议层永久失去接业务编排的能力。

同时**收紧** `the_api_crate_adds_no_internal_dependencies` 的 allowlist：
现在它是 9 条全放行，**每条都没有条款依据**。修法是给每条依赖标注
「依据架构文档哪一条」，未标注的不得放行。

**验收**

| 项 | 要求 |
|---|---|
| 依赖面 | `deps_of nexus-api` 与 `Cargo.toml` 一致（`check-deps.sh` 规则 7 已覆盖） |
| allowlist | 每条允许的依赖都有**条款依据**；新增依赖未标注依据 → 门禁红 |
| **负控** | 加一条无依据的依赖 → 门禁**必须红** |

**falsifier**：`nexus-api/src` 里能找到 `nexus_config` / `nexus_model` /
`nexus_security` / `nexus_service` / `nexus_execution` / `nexus_llm` 的**实际引用** → 本条不成立。

---

#### F-05 · P6 契约的 `crate` 字段单值，错误码扫描漏扫 `nexus-workflow`

- **类型**：缺陷 ｜ **严重度**：high ｜ **证据级别**：L1 可复现
- **落点**：`docs/specs/p6/contracts/invariants.json`、`scripts/check-specs.sh:298-320`

**事实**

`invariants.json` 的 `crate` 字段是**单值**：

```json
"crate": "nexus-execution",
```

而同一份契约里有 **23 处** `nexus-workflow` 的路径引用。
规则 5 读 `crate` 字段，然后**只扫那一个 crate** 的源码：

```bash
crate_name=$(python3 - ... json.load(fh).get("crate", "") ...)
crate="crates/$crate_name"
[ -d "$crate/src" ] || continue
used=$(find "$crate/src" -name '*.rs' ... )
```

⇒ **`nexus-workflow/src` 里使用白名单外错误码，不会让门禁变红。**

**诚实说明：这一点已被契约文档部分承认**

`invariants.json` 的 `$comment` 里明确写了：

> 「`crate` 字段是**逐 Spec** 判定 —— P6 的主 crate 是 `nexus-execution`。
> `nexus-workflow` 的不变量**也在本文件里**声明（同一份 P6 契约），
> 但 `check-specs.sh` 规则 5 只扫描 `crate` 字段指向的那个 crate 的源码」

**但那条注释解释的是「结构性断言由 T-003 / T-004 自己在 `crates/nexus-workflow/tests/` 里落地」**，
**没有覆盖「错误码扫描」这一项**。
契约声明了「`nexus-workflow` 的**结构性**断言另有归属」，
但没说「`nexus-workflow` 的**错误码**使用不受检查」。

**为什么现在修最便宜**

P6 **尚未开工**（`nexus-workflow` 与 `nexus-execution` 都是 7 行空壳）。
现在改规则 5 的多 crate 支持，是**改一个脚本**；
等 P6 落地、两个 crate 各有几百行代码之后改，就是**改一个已经有一堆"已知违规"要豁免的东西**。

**建议改法**

规则 5 支持**多个** crate：

```json
"crates": ["nexus-execution", "nexus-workflow"],   // ← 取代单值 "crate"
```

脚本侧：`crates` 字段接受字符串或数组；**逐个**扫描并**逐个**用本 Spec 的白名单判定
（保持「不能用并集」的既有纪律）。保留单值 `crate` 的向后兼容，
或一次性迁移并让旧写法**硬失败**（更好——避免静默跳过）。

**验收**

| 项 | 要求 |
|---|---|
| 覆盖 | `nexus-workflow/src` 里的白名单外 `ErrorCode::` 被检出 |
| 纪律保持 | 「逐 Spec 判定、不用并集」的行为**不变**（P3 的 crate 不得用 P4 的码） |
| **负控** | 在 `nexus-workflow/src` 加一个白名单外的 `ErrorCode::Forbidden` → 门禁**必须红** |

**falsifier**：`nexus-workflow/src` 里的白名单外错误码**能让门禁变红** → 本条不成立。

---

#### F-06 · 依赖策略在 bash 与 Rust 两处实现

- **类型**：缺陷 ｜ **严重度**：medium ｜ **证据级别**：L2 可核对
- **落点**：`scripts/check-deps.sh:20-46`、`crates/nexus-core/tests/architecture.rs:103-210`

`deps_of`（bash 的 `case` 表）与 Rust 侧的 `CAPABILITY` / `ALLOWED` 表
描述的是**同一套依赖策略**，但写在两个地方、两种语言里。
`check-deps.sh` 规则 7 校验「`deps_of` ↔ `Cargo.toml` 一致」，
却**没有**校验「bash 表 ↔ Rust 表一致」。

⇒ 新增 crate 时，可能出现「Cargo.toml 改了、bash 表改了、Rust 表没改」的窗口。

**建议改法**：把策略表收敛成**一份 JSON**，`bash` 与 Rust 两侧都从它读。
或（更轻）加一条门禁断言两侧表**逐条相等**。

**验收**：新增一条依赖边 → **只**改 JSON 一处 → 两端判定同时生效；
若只改了一处 → 门禁红。

**falsifier**：两侧表已经有自动的一致性校验 → 本条不成立。

---

#### F-07 · `nexus-api` 的 crate 级文档严重过期

- **类型**：缺陷 ｜ **严重度**：medium ｜ **证据级别**：L2 可核对
- **落点**：`crates/nexus-api/src/lib.rs:25-36`

文档仍写着：

> 「**P5 尚未完成。**」
> 「P5 收口（架构文档同步 + 契约升格）属 NEX-36。」

而 NEX-36 **已完成并归档**（`c37d61e`）。

**为什么这不只是措辞问题**

架构文档 §5.4 的不变量表把「文档同步」列为 **advisory** 门禁。
而本仓库的读者模型是「**先读 crate 文档，再读代码**」——
crate 文档说「尚未完成」，读者会按未完成的心智模型去读那 7 个模块，
**并跳过已被验证过的部分**。

**建议改法**：更新为完成态，并把「模块 ↔ 任务 ↔ 契约不变量」的对应关系写进去。

**falsifier**：`lib.rs` 已声明 P5 完成 → 本条不成立。

---

#### F-08 · `nexus-api` 有两套响应构造器、三套鉴权中间件入口

- **类型**：缺陷 ｜ **严重度**：low ｜ **证据级别**：L2 可核对
- **落点**：`crates/nexus-api/src/response.rs`、`crates/nexus-api/src/middleware.rs`、`crates/nexus-api/src/router.rs`

| 重复 | 详情 |
|---|---|
| 响应构造器 | `ApiResponse::ok()` / `ok_with()` / `failure()` / `failure_cow()` / `from_facing()` **加上** 5 个自由函数 `ok_response()` / `ok_with()` / `failure_response()` —— 自由函数版**零调用方** |
| 鉴权中间件 | `middleware::identity_middleware`（泛型）、`router::identity_middleware_erased`（擦除）、`tests/middleware.rs::identity_middleware_dyn`（测试里又写一遍） |

**建议改法**：删掉零调用方的自由函数；三个鉴权入口里，测试那份应复用生产代码
（测试自己写一份实现 = **断言的是测试自己的副本**，同义反复）。

**falsifier**：自由函数版本有真实调用方 → 本条不成立。

---

#### F-09 · `Cargo.toml` 声称 CI 用 `cargo-deny`，仓库里没有

- **类型**：缺陷 ｜ **严重度**：medium ｜ **证据级别**：L2 可核对
- **落点**：`Cargo.toml:34`

```toml
# 禁止在成员 crate 里写具体的版本号（CI 用 cargo-deny 校验）。
```

但：`deny.toml` **不存在**；`.github/workflows/ci.yml` 与 `scripts/` 里**没有任何** `cargo deny`。

> 架构文档 §5.4 的设计原则之一是「**只做**可执行的断言」，
> 而仓库自己的脚本注释里写着：「**无法执行的判据比没有判据更危险**——
> 它看起来是覆盖了的。」
> 这条注释说的正是本条。

**建议改法**：二选一——① 补 `deny.toml` + CI 步骤；② 删掉那句注释。
**建议选 ②**（`cargo-deny` 是独立工具链，接入成本高于收益，且 `check-lock.sh` 已覆盖版本纪律）。

**falsifier**：`deny.toml` 存在**且** CI 里有 `cargo deny` 步骤 → 本条不成立。

---

#### F-10 · `examples/` 的 61 个测试不在 CI 里

- **类型**：缺陷 ｜ **严重度**：medium ｜ **证据级别**：L2 可核对
- **落点**：`.github/workflows/ci.yml`

`examples/` 是**独立 workspace**（`exclude` 隔离），CI 里**没有**对应的 job。
架构文档 §5.5 明确说这些 demo 是「设计验证的**回归基准**，不是临时代码」。

⇒ 它们**可以**腐烂而不被发现。

**建议改法**：CI 加一个 `cd examples && cargo test --workspace` 的 job。
（成本极低：独立 workspace，无需数据库。）

**falsifier**：CI 里有跑 `examples/` 的步骤 → 本条不成立。

---

#### F-11 · CI 的集成测试套件清单是手工维护的

- **类型**：缺陷 ｜ **严重度**：medium ｜ **证据级别**：L2 可核对
- **落点**：`.github/workflows/ci.yml:152-177`

CI 里有 6 处 `check_suite "tests/xxx.rs" <阈值>` 硬编码。
新增一个集成测试文件而**忘记**加进清单 → 那个文件**不会被编进 CI**，
CI 照样绿。

NEX-14 已经建立过「集成测试在 CI 里**真的跑**」的防线，
但那防的是「套件被跳过」，**没防**「套件被漏登记」。

**建议改法**：从 `crates/*/tests/*.rs` 自动发现 + 用「测试数 ≥ 1」作为阈值，
或至少加一条断言「每个 `tests/*.rs` 都出现在清单里」。

**falsifier**：清单由脚本自动生成 → 本条不成立。

---

### 4.2 待决策（**不由代理决定**）

以下四条需要**操作者**裁决。代理**不得**自行决定，也**不得**在代码里隐式解决。

---

#### D-01 · 引擎侧持久化的 crate 归属（OQ-1）

- **状态**：✅ **已由操作者裁决**（2026-11-06）—— **选项 B**。见 `docs/specs/p6-tiering.md` OQ-1。
- **时间盒截止**：**2026-11-13**（距今 7 天）
- **严重度**：high

**冲突的两条已定稿条款**

| 条款 | 内容 |
|---|---|
| 架构文档 §4.1 crate 图 | 画着 `EXE --> REPO`（`nexus-execution` 依赖 `nexus-repo`） |
| 架构文档 §4.2 差异 1 / §4.3 表格 | 「`nexus-execution` **不**依赖 `nexus-repo`」 |

**现状**：`nexus-repo/src/leases.rs` 里写着「表：`execution_runs`（不是 `workflow_runs`）」，
围栏实现**已经在 `nexus-repo`**。所以现在是**「代码在 repo，图和条款说在 execution」**。

**P6 的 D12 决策**：**选项 B** —— **不迁移**；
`nexus-execution` **新增**依赖 `nexus-repo`，引擎侧持久化能力**留在** `nexus-repo`。
时间盒到 2026-11-13。

**落地影响**：该决策**修订了架构文档 §4.2 差异 1 / §4.3 目标态表 / §4.3.2 约束 2 /
§4.4（新增规则 4）/ §5.3 / §6.3** —— 属**已定稿条款的修订**，
T-001（`NEX-37`）需**人工评审** + **额外一名评审**。

**判定手段的同步变更**：「`workflow_runs` 写入权唯一」原先靠
「`nexus-repo` 不得引用该表的**类型**」判定 —— 选项 B 下**推不出**该结论，
改为「该表的**写操作**只允许出现在 `nexus-execution`」（强度**不降**）。

**本轮评估补充的事实**：`NEX-46`（P6 收口）的 AC #3 已经写明：

> 「架构文档 §4.1 的 crate 图与 §4.2 / §4.3 的**不一致**被**消解**（要么图改，要么条款改 —— **不得留矛盾**）」

⇒ 修订走的是**条款**方向（改 §4.2 / §4.3），§4.1 的图**本就**与修订后一致。
**§4.1 的矛盾已在 T-001（`NEX-37`）里消解**，NEX-46 只**核对七处的一致性**。

---

#### D-02 · OQ-1 的载体层级：Spec 内时间盒决策 vs RFC

- **严重度**：high ｜ **需要**：操作者裁决

**冲突**

| 文件 | 规定 |
|---|---|
| `docs/rfcs/README.md:7-15` | 「决策触及架构文档 §6.3 的**已定稿条款**」→ 写 RFC。§6.3 第 5 项正是「**crate 边界与 trait 归属**」 |
| `docs/specs/p6-tiering.md:96` | 「这条**不升 RFC**，而是在 Spec 里登记为**带时间盒的决策**（类别 b）」 |

⇒ 两条规则对**同一个决策**给出**相反结论**。

**选项**

| | 动作 | 代价 | 可逆性 |
|---|---|---|---|
| **A** | 承认 OQ-1 属 RFC 级，补 `NEX-RFC-1` 后再开 NEX-37 | 中：P6 主线被一个文档拖住 | 高 |
| **B** | 修订 `docs/rfcs/README.md`：明确「**权威文档内部矛盾**」不走 RFC（它是**消解**矛盾，不是**新增**决策） | 低 | 高 |

**建议**：**B**。

**依据**：OQ-1 的性质是「**权威文档自相矛盾**」，不是「工程上有多种正确做法」。
`docs/rfcs/README.md` 列的四条触发条件里，「存在合理工程师会就正确做法产生分歧」
**不成立**——两边都说得通，但矛盾的是**文档不是做法**。
RFC 的门槛是「决策有分歧」，这里**没有分歧，只有矛盾**。

**这不是代理的决定权范围**——本条**只登记**。

---

#### D-03 · `nexus-api` 的 6 条未使用依赖：删 4 条还是全留

- **严重度**：medium ｜ **需要**：操作者确认

见 F-04。代理的判断是「删 4 留 2」（保留 `nexus-service` / `nexus-execution`，
因为 §4.1 crate 图与 §4.1.1 的修正明确承认它们）。

**但「保留未使用的边」是否是想要的**，属于**架构意图**，不是机械问题：
如果 §4.1 的 `API --> SVC` 是**目标态**而非现状描述，那这条边**应该**留着等 P6 接线；
如果它只是**历史遗留的图**，那它该删。

**建议**：保留（目标态语义），并在 `check-deps.sh` 的 `deps_of` 旁边注明
「此边为 §4.1 目标态预留，待 P6 接线」。

---

#### D-04 · 12 个空壳 crate 的去留

- **严重度**：medium ｜ **需要**：操作者裁决

22 个成员 crate 里 **12 个是 7 行空壳**。它们在 Cargo 依赖图上**存在**，
但零实现。

**问题不是「空壳」本身**——P0 建骨架时就预置了它们，这是正常的分阶段策略。
**问题是**：依赖图**看起来**比实现**完整得多**。
`nexus-service` 依赖 16 个 crate 却只有 7 行；
`check-deps.sh` 对这 12 个 crate 的依赖方向检查**全部通过**——因为它们确实没有反向依赖。

⇒ 门禁给出的是「**依赖方向正确**」的结论，而读者容易读成「**这些 crate 实现了**」。

**选项**

| | 动作 |
|---|---|
| **A** | 保留骨架，给每个空壳的 `lib.rs` 写**明确的** crate 级文档（「本 crate 负责 X，当前状态：P7 起实施」） |
| **B** | 删掉尚未启动阶段的空壳，等该阶段再建 |

**建议**：**A**。理由：本仓库的分阶段策略（P0 骨架 → P1..P9）依赖这些 crate 存在；
删了会让 `check-deps.sh` 的 `deps_of` 表、架构文档 §4.1 的图、以及 P6 已在飞的
`nexus-workflow` / `nexus-execution` 全部要改。**A 的成本低得多**——
只改 12 个文件的文档。

**关键**：`A` 必须真的写出「当前状态」，否则空壳与「已实现」在读者眼里没有区别。

---

### 4.3 债务与重构（已认可，不需决策）

| ID | 债务 | 严重度 | 说明 |
|---|---|---|---|
| **C-01** | `strip_comments` 三份副本 | high | 见 F-02，**修法本身**就是这项债务 |
| **C-02** | 依赖策略两份实现 | medium | 见 F-06 |
| **C-03** | `nexus-model` 只有 220 行（session / user） | low | 架构文档 §4.2 差异 2 说它是「跨 crate 共享的纯 DTO crate」，但领域类型大量散在 `nexus-core`。**不建议现在动**——会触及已定稿的 ID 类型归属 |
| **C-04** | `nexus-core/src/error.rs` 900 行 | low | 生产代码与 `#[cfg(test)]` 交织。**不建议现在拆**——§6.3 的 42 码表是契约，拆文件有「改动契约载体」的风险 |
| **C-05** | `sse.rs` 851 行 / `router.rs` 526 行 | low | 同上。测试与实现同文件在本仓库是**刻意的**（本仓库注释密度高，注释承载「为什么」） |
| **C-06** | 架构文档单文件 2,422 行 | low | §5.4 不变量表已 150+ 行。拆分需谨慎——它是**唯一权威**，拆分有「权威分散」风险 |

> **关于 C-03 ~ C-06 的统一处置意见**：**现在都不动。**
> 理由：这四项都会触碰「已定稿条款的载体」（42 码表 / 18 个 ID 类型 / 唯一权威），
> 而它们的收益纯属「文件大小」美学。**在本仓库的规则下，「低风险高收益」比「低风险低收益」更值得做**。

---

### 4.4 想法（**明确不建议现在做**）

| ID | 想法 | 为什么现在不做 | 触发条件 |
|---|---|---|---|
| **H-01** | 其他 crate 是否也有未使用依赖？ | 本轮**未逐个验证** —— 只能列为猜想。`cargo machete` 可一次性查清 | 跑一次 `cargo machete`，输出为空则证伪 |
| **H-02** | 变异测试脚本化（`scripts/check-mutants.sh`） | 全量变异测试成本高。规则文件里有「变异测试」的要求，但**没有脚本也没有 CI 步骤** —— 先用**手工 patch + 记录** | 负控条目累计 ≥ 5 条，或故障注入重复出现 |
| **H-03** | 引入 `cargo machete` 检测未使用依赖 | H-01 的解法。**在 H-01 证伪前引入是浪费** | H-01 被证实 |
| **H-04** | 把 `nexus-api` 的 `Arc<dyn IdentityExtractor>` 换成 enum | §2.3.2 的 enum 优于 dyn 是**已定稿**的设计原则，但那条原则的适用场景是「**热路径**」与「**配置驱动**」。鉴权提取器既不在热路径，也不配置驱动 | 出现真实的「运行时替换鉴权实现」需求 |
| **H-05** | 拆分架构文档 | 唯一权威文档拆分的风险见 C-06 | 文档超过 3,000 行**且**出现多人并发编辑冲突 |

---

## 5. 改进波次

### 5.1 波次划分

按**风险面**切，不按文件切。理由：改门禁的失败模式是「门禁变红」，
改生产代码的失败模式是「行为变了」——两者的回归面完全不同，混在一起会让验证失效。

```
W0 止血          —— 恢复门禁的有效性（不改任何生产行为）
 │                 F-01（panic）· F-02（扫描器）· F-03（sed）
 │                 F-04（依赖噪音）· F-05（P6 契约覆盖）
 │
W1 契约与文档    —— 让文档与实现对上
 │                 F-07（crate 文档过期）· F-09（cargo-deny 幽灵门禁）
 │                 F-11（CI 套件清单）· F-10（examples 不在 CI）
 │                 D-04（空壳 crate 文档化）· C-02（依赖策略两份实现）
 │
W2 清理          —— 删掉零调用方与重复
                   F-08（响应构造器 / 鉴权入口）
 │
W3 决策落地      —— 等操作者裁决后执行
                   D-01（crate 归属）✅ 已裁决 2026-11-06（选项 B）→ T-001
                   D-02（OQ-1 载体层级）
                   D-03（依赖边去留）
```

### 5.2 每波的进入 / 退出条件

| 波次 | 进入条件 | 退出条件 |
|---|---|---|
| **W0** | 本文档被接受 | ① F-01 的复现序列**不再 panic**；② 三份扫描器**统一为一份**，四组对抗输入全部正确；③ `sed` 剥注释**全部替换**为 token 级扫描；④ `nexus-api` 只剩 3 条被授权依赖；⑤ `nexus-workflow` 的错误码**进入**规则 5 视野；⑥ **全仓复扫后无未处置的新违规** |
| **W1** | W0 全绿 | ① crate 文档与实现一致；② 12 个空壳写明「当前状态」；③ `examples/` 进 CI；④ CI 套件清单自动化或加完整性断言；⑤ 依赖策略单源 |
| **W2** | W1 全绿 | 零调用方的自由函数与重复实现被删除，且**门禁仍绿** |
| **W3** | **操作者已裁决** D-01 / D-02 / D-03 | 对应条款**修订落盘**，且 `check-deps.sh` / `check-rules.sh` / `check-docs.sh` 全绿 |

### 5.3 与 P6 的关系

**P6 正在开工（`docs/specs/p6/`，NEX-37 ~ NEX-46 已建、均未开始）。**

| 关系 | 说明 |
|---|---|
| **W0 与 P6 可并行** | W0 改的是 `nexus-api` / `scripts/`，P6 改的是 `nexus-workflow` / `nexus-execution`。**唯一的交集**是 F-05（规则 5 支持多 crate）——**建议 W0 先做 F-05**，因为 P6 落地后改规则 5 成本陡增 |
| **W0 不得碰 `nexus-execution` / `nexus-workflow` 的 `src/`** | 那是 P6 的工作面。W0 只改**脚本**与**契约文件**，不动 P6 的实现面 |
| **W1 的 D-04 与 P6 有交集** | `nexus-workflow` / `nexus-execution` 是 P6 的**主 crate**，D-04 给它们写「当前状态」文档时**必须**写「P6 实施中」，不能写「未开始」 |
| **D-01 的时间盒 (2026-11-13) 早于 P6 的实现完成** | 若不裁决，NEX-37（T-001 迁移）按 D12 的自主回退执行——**本计划不改变这一点**，只是把它提前暴露 |

### 5.4 关键路径

```
F-01（panic）───┐
F-02（扫描器）──┼──> W0 退出 ──> W1 退出 ──> W2 ──┐
F-03（sed）────┤                                   ├──> 全部完成
F-04（依赖）───┤                                   │
F-05（P6 契约）─┘                                   │
                                                    │
D-01 / D-02 / D-03（操作者裁决）─────────────────────┘
```

**关键路径上的单点**：W0 依赖**扫描器统一**这一件事。
F-02 与 F-03 是**同一份工具**（token 级扫描）的两个应用点——
**应该一起做**，而不是分两个任务。分开做会出现「两次改同一个脚本」的冲突。

### 5.5 建议的任务分解（**未落盘**）

> ⚠️ **这些任务号是建议**。按 AI-SDLC 规则，任务文件需要走 S1 → S2 → S3（DoR）→
> 授权后才落盘。本文档**不**代为登记，也**不**代为编号。

| 建议任务 | 内容 | 依赖 | 产出 |
|---|---|---|---|
| `NEX-47` | F-01：`FrameDecoder` 状态机改造 + 负控 | — | `sse.rs` + 变异测试 |
| `NEX-48` | F-02/F-03：token 级扫描器（`syn`），替换 3 份手写剥离器 + `sed` 剥注释 | — | `nexus-core/tests/support/` + 三处调用点 |
| `NEX-49` | F-02/F-03 后续：全仓复扫 + 逐条分诊 | NEX-48 | 新 finding 登记 / 修复 |
| `NEX-50` | F-04 + D-03：`nexus-api` 依赖面收敛 + allowlist 标注条款依据 | —（D-03 需先裁决） | `Cargo.toml` + `check-deps.sh` |
| `NEX-51` | F-05：规则 5 支持多 crate | — | `check-specs.sh` + P6 契约 |
| `NEX-52` | F-07/F-09：文档与幽灵门禁修正 | — | `lib.rs` + `Cargo.toml` |
| `NEX-53` | F-10/F-11：CI 补 `examples/` + 套件清单自动化 | — | `ci.yml` |
| `NEX-54` | F-08 + D-04：零调用方清理 + 空壳 crate 文档化 | — | `response.rs` / `middleware.rs` / 12 个 `lib.rs` |
| `NEX-55` | F-06：依赖策略单源 | NEX-48 | 策略文件 + 两侧读取 |

> **关于编号**：P6 已占用 `NEX-37 ~ NEX-46`。本表从 `NEX-47` 起，
> 但**须在任务真正落盘时由 DoR 流程确认无冲突**——`backlog/` 里已登记的
> P6 任务文件尚未提交，本文档的编号建议**不构成占位**。

---

## 6. 待决策清单（等操作者回答）

| ID | 问题 | 选项 | 建议 | 阻塞 |
|---|---|---|---|---|
| **D-01** | 引擎侧持久化归 `nexus-execution` 还是 `nexus-repo`？ | ✅ **已裁决：B**（2026-11-06）—— 不迁移，`nexus-execution` 新增 `nexus-repo` 依赖 + 修订 §4.2 / §4.3 / §4.3.2 / §4.4 | **B** | 已落地为 T-001（`NEX-37`）｜ 时间盒 **2026-11-13** |
| **D-02** | OQ-1 走 Spec 内时间盒决策，还是升 RFC？ | A：升 RFC ｜ B：修订 `docs/rfcs/README.md`，明确「权威文档内部矛盾」不走 RFC | **B** | P6 主线的文档层级 |
| **D-03** | `nexus-api` 的 `nexus-service` / `nexus-execution` 两条边留还是删？ | A：留（§4.1 目标态预留）｜ B：删 | **A** | NEX-50 |
| **D-04** | 12 个空壳 crate：留骨架并写明状态，还是删掉？ | A：留 + 写「当前状态」｜ B：删 | **A** | NEX-54 |

> **Away rule**：操作者不在场时，**不发起阻塞性提问**。
> 上述四条**只登记**；D-01 已由操作者裁决（选项 B），
> 其余三条**不阻塞**任何已开始的任务。

---

## 7. 风险登记

### 7.1 这份计划本身的风险

| 风险 | 后果 | 缓解 |
|---|---|---|
| **修复扫描器会暴露大量新违规** | 全仓复扫后可能出现几十条「漏网」 | **逐条分诊**：修 / 登记 / 豁免——**不得**靠放宽断言让它变绿（NEX-49） |
| **F-01 修复引入新的状态语义** | 畸形帧后解码器进入 `Poisoned` 是**行为变更**（原来会 panic） | 变更需在 P5 的契约里补一条不变量；`Poisoned` 的幂等语义要单独测 |
| **W0 与 P6 并行产生冲突** | 同一个 `check-specs.sh` 被两边改 | W0 只改脚本与契约文件，**不碰** P6 的 `src/`；F-05 建议先做 |
| **严重度基线会漂移** | P6 落地后 F-01 的「无生产调用方」前提消失 | §0.4 已把「P6 完成」列为复审触发条件 |
| **本计划被当成决议** | 读者以为 D-01 已经定了 | §0.2 明确「不是决议」；§6 明确「等操作者回答」 |

### 7.2 明确不做（out of scope）

| 项 | 理由 |
|---|---|
| 改 42 个业务码 / 12 个事件名 / 18 个 ID 底层类型 | 架构文档 §6.3 **已定稿** |
| 改 crate 依赖方向 | §6.3 已定稿；F-04 的处置**不涉及**方向 |
| 新增 `ErrorCode` 变体 | 同上 |
| 修 `nexus-api` 的**架构设计**（enum vs dyn 等） | P5 已定稿；相关想法见 H-04 |
| 拆分架构文档 | C-06；唯一权威文档的拆分风险高于收益 |
| 迁移 `nexus-model` 的领域类型 | C-03；会触及已定稿的 ID 类型归属 |
| 引入 `cargo machete` / `cargo-deny` | H-03 / F-09 —— 前者待 H-01 证实，后者建议先删注释 |
| 任何 `unsafe` | workspace 已 `forbid` |
| 在本文档里**直接修代码** | 本文档是**计划**；实现走 S2 → S3 → S4 |

---

## 8. 验收标准（这份计划怎么算做完）

| # | 判据 | 可判定性 |
|---|---|---|
| 1 | F-01 的复现序列不再 panic，且畸形后 `push` 幂等返回同一错误 | 自动化测试 |
| 2 | 三份扫描器统一为一份，四组对抗输入（生命周期 / 字符串含 `//` / 字符字面量 / 字符串含 `/*`）全部正确 | 自动化测试 + 负控 |
| 3 | `sed` 剥注释从两个脚本里**完全移除**，token 级扫描等价 | 源码断言 + 负控 |
| 4 | `nexus-api` 依赖面收敛，且每条允许的依赖都有条款依据 | `check-deps.sh` + allowlist 标注 |
| 5 | `nexus-workflow` 的错误码进入规则 5 视野 | 负控：加白名单外码 → 门禁红 |
| 6 | 12 个空壳 crate 的 `lib.rs` 写明「负责什么 + 当前状态」 | 人工核对 + 抽样 |
| 7 | `examples/` 的 61 个测试在 CI 里跑 | CI 步骤存在性断言 |
| 8 | CI 套件清单自动生成或加完整性断言 | 负控：新增 `tests/x.rs` 不登记 → 门禁红 |
| 9 | D-01 / D-02 / D-03 / D-04 **已裁决**并落盘 | 操作者确认 + 条款修订 |
| 10 | 文档进 `docs/README.md` 索引 | `check-docs.sh` 规则 2 |

> ⚠️ **第 10 条现在就适用**——本文档落盘后**必须**在 `docs/README.md` 登记，
> 否则 `bash scripts/check-docs.sh` 直接红。
> 这是「文档层一致性」门禁的既有行为，不是本文档新增的要求。

---

## 附录 A · 复现命令

```bash
# A-1  F-01：SSE 解码器 panic
#     需要一个临时测试文件（仓库内不保留探针）
cat > crates/nexus-api/tests/zz_repro.rs <<'EOF'
use nexus_api::sse::FrameDecoder;
#[test]
fn panic_when_scan_cursor_is_stale_after_drain() {
    let mut d = FrameDecoder::new();
    d.push(&[b'x'; 100]).unwrap();
    assert!(d.push(b"ab\n\n").is_err());
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.push(b"z")));
    assert!(r.is_err(), "PANIC REPRODUCED");
}
EOF
cargo test -p nexus-api --test zz_repro
rm crates/nexus-api/tests/zz_repro.rs
# 期望：panicked at crates/nexus-api/src/sse.rs:259: range start index 97 out of range

# A-2  F-02：扫描器假阳性
#     在 bridge.rs 的 &'static 之后加一句纯注释
#     → the_api_crate_does_not_use_the_axum_sse_type 应当变红
printf '\n// 参考：axum::response::sse 会自动转义 data:\n' >> crates/nexus-api/src/bridge.rs
cargo test -p nexus-api --test architecture
git checkout crates/nexus-api/src/bridge.rs

# A-3  F-03：sed 剥注释吞掉真实代码
printf 'let u = "http://x"; ErrorCode::Forbidden;\n' | sed -e 's://.*::'
# 期望输出：let u = "http:    ← ErrorCode::Forbidden 消失

# A-4  F-04：未使用依赖
for d in nexus_config nexus_model nexus_security nexus_service nexus_execution nexus_llm; do
  printf '%-18s %s\n' "$d" "$(grep -rl "\b$d\b" crates/nexus-api/src | wc -l | tr -d ' ')"
done
# 期望：全部为 0

# A-5  F-05：P6 契约的 crate 字段
python3 -c "import json;print(json.load(open('docs/specs/p6/contracts/invariants.json'))['crate'])"
# 期望：nexus-execution（单值 → 漏扫 nexus-workflow）

# A-6  F-07：crate 文档过期
grep -n "尚未完成\|NEX-36" crates/nexus-api/src/lib.rs

# A-7  F-09：cargo-deny 幽灵门禁
grep -n "cargo-deny" Cargo.toml; ls deny.toml 2>&1; grep -c "cargo deny" .github/workflows/ci.yml
```

## 附录 B · 本轮评估的证据来源

| 来源 | 用途 |
|---|---|
| `cargo test -p nexus-api`（48 条） | F-01 复现；确认既有测试全绿 |
| `cargo test -p nexus-api --test architecture`（6 条） | F-02 复现（注入注释 / 注入违规） |
| 三份 `strip_comments` 的**逐分支重放** | F-02 的四种对抗输入行为表 |
| `sed -e 's://.*::'` 在真实源码上的对比 | F-03 的变体数对比（43→42、1→0、2→2） |
| 逐 crate grep `crates/nexus-api/src` | F-04 的 9 条依赖使用情况 |
| `check-specs.sh:298-340` + `p6/contracts/invariants.json` | F-05 的单值 `crate` |
| `.github/workflows/ci.yml` | F-10 / F-11 |
| `docs/specs/p6-tiering.md`、`docs/rfcs/README.md`、`backlog/tasks/NEX-46*` | D-01 / D-02 的条款冲突与在飞任务 |

> **诚实声明**：本轮**未运行** PostgreSQL 集成测试、`examples/` workspace、
> `cargo deny` / `cargo audit`。涉及这些范围的结论仅作静态核对，已在 §3.3 列明。

---

## 相关文件

| 文件 | 作用 |
|---|---|
| [`2026-11-04-nexusagent-rust-architecture-design.md`](2026-11-04-nexusagent-rust-architecture-design.md) | **唯一权威** —— 本文档不与它冲突，只在其未定稿处提建议 |
| [`2026-11-04-nexusagent-eino-rust-reference-architecture.md`](2026-11-04-nexusagent-eino-rust-reference-architecture.md) | 参考实现对照 —— 本文档的事实核查基线之一 |
| [`../specs/p6/spec.md`](../specs/p6/spec.md) | P6 契约 —— F-05 与 D-01 的落点 |
| [`../specs/p6-tiering.md`](../specs/p6-tiering.md) | P6 分层判定 —— OQ-1 的登记处 |
| [`../../backlog/README.md`](../../backlog/README.md) | 任务载体格式 —— §5.5 的落点 |
| [`../../.cursor/rules/ai-sdlc-decision-authority.mdc`](../../.cursor/rules/ai-sdlc-decision-authority.mdc) | 决策三分类与 OQ 禁令 —— §4.2 的依据 |
