---
name: nexus-rust
description: 在 nexus-agent 仓库里写 Rust 的执行手册。覆盖 crate 归属与依赖方向、类型化数据形状、三层失败语义、三层验证（trybuild 负向测试 / token 级结构扫描 / 契约表）与门禁命令。Use when adding or changing Rust code, a crate, an error path, an event, a type state, or tests in this repo, or when the request mentions Rust、crate、依赖方向、架构测试、trybuild、契约测试、错误码、事件负载.
---

# nexus-rust

本技能只讲**在这个仓库里**怎么写 Rust。通用 Rust 教程不在此。

## 权威顺序

1. `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md` 是契约的唯一权威。
2. `.cursor/rules/rust-code-standards.mdc` 是代码层判据。
3. `.cursor/rules/ai-sdlc-implementation.mdc` 是实现期的禁止事项。

已定稿条款（架构文档 §6.3）不得在实现里重新决定：错误码值、事件名与负载字段、ID 底层类型、crate 边界、MCP/A2A 归属、依赖形态。要改它们就停止，出 RFC 或升级。

## 开工前

1. 读架构文档里与本次改动相关的章节，确认所依赖的条款**已定稿**。
2. 定位改动落在哪个 crate。crate 图在架构文档 §4.1，`scripts/check-deps.sh` 的 `deps_of` 是它的**可执行**版本。
3. 答**契约影响五问**，逐项写「是」或「否」。错误码值、事件名与负载形状、ID 底层类型、crate 依赖方向、对外响应形状。触及任何一项就要更新契约测试与文档表格，并标注破坏性变更。

## 先命名数据形状

写逻辑之前先说清数据形状，以及它由什么结构承载。仓库已有四种可复用的形状。照抄形状，不要新造抽象。

| 需要表达的 | 形状 | 现成范本 |
|---|---|---|
| 区分不同实体的标识 | 独立 newtype，`#[serde(transparent)]` 输出裸值 | `crates/nexus-core/src/id.rs` 的 `define_i64_id!` / `define_uuid_id!` / `define_string_id!` |
| 区分可取消与不可取消这类状态 | 类型状态，非法状态对应**不存在**的 API | `crates/nexus-core/src/cancel.rs` 的 `RequestScope` 与 `DetachedScope` |
| 单写者且序号有序的事件流 | 不可 `Clone` 的 sink，`&mut self` 写入 | `crates/nexus-core/src/event.rs` 的 `EventSink` |
| 外部不可信输入 | 边界处解析成类型，内部信任类型 | `crates/nexus-core/src/canonical.rs` 的 `canonicalize` |

规则：状态用类型表达，不用 bool 字段组合。ID 不传裸 `i64` 与裸 `String`。同一个形状假设跨文件重复时，把它收进一个类型或一张表。

## 失败语义

三层错误。领域 enum → `AppError` → 传输层 `IntoResponse`。

- 领域错误是 enum，变体带结构化字段，`#[from]` 做自动转换，`match` 穷尽。
- `AppError` **不实现 `Serialize`**。对外只给 `UserFacing { code, message }`，内部原因只进日志与 `Display`。
- `ErrorCode` **手工实现** `Serialize` 与 `Deserialize`，输出数字码。derive 会输出变体名，破坏前端按码段处理。
- 每个错误都要能答出重试分类。`Retryable` 是明确失败、可安全重试。`UnknownOutcome` 是结果未知，转人工。`Terminal` 是明确失败、重试无意义。
- 超时默认映射到 `UnknownOutcome`。把 `UnknownOutcome` 当可重试会让已产生副作用的动作重复执行。

不要给 enum 变体加 `#[serde(flatten)]`。内部 tag 与 enum 级 flatten 不兼容。`flatten` 只能用在变体内部的 struct 字段上。

事件负载用 `#[serde(tag = "type")]` 做内部标签，序列化后 `type` 与其他字段同级。

## crate 归属与依赖

- 能力层不得依赖 `nexus-service` / `nexus-api` / `nexus-cli`。
- `nexus-core` 零内部依赖，是依赖图的根。
- `sqlx` 只出现在 `nexus-repo` / `nexus-infra` / `nexus-execution`。
- `nexus-execution → nexus-repo` 允许，反向禁止（决策 D12，选项 B）。
- `nexus-model` 是纯 DTO，不得出现 `FromRow` / `sqlx::` 痕迹。
- 依赖版本只能来自根 `Cargo.toml` 的 `workspace.dependencies`，成员 crate 不写具体版本号。
- 需要跨层能力时用**消费者定义 trait**，提供方 `impl Trait for Type`。
- 不声明零引用的内部依赖，除非架构文档预留了接缝并写明条款依据。

改依赖时同步改 `scripts/check-deps.sh` 的 `deps_of`，否则规则 7 会红。

## 验证三层

每条新代码至少落在一层。

**类型层.** 断言写错的代码编译不过。样例放 `crates/nexus-core/tests/compile_fail/`，每个样例配一个 `.stderr` 快照，用 `trybuild` 的 `compile_fail` 跑。快照入库，编译器行为变化会让测试立刻失败。先放一条框架自检样例，防止工具链坏了导致静默跳过全部用例。

**结构层.** 扫源码的断言**必须剥离注释**，判据要精确到完整路径。`axum::response::sse` 这种全路径算违规，「出现过 axum 和 sse」不算。扫描用 token 级扫描器。

```bash
cargo run -q --locked -p nexus-core --example token_scan -- db-traces crates/nexus-model/src
```

共用实现是 `crates/nexus-core/tests/support/token_scan.rs`。其他 crate 用 `#[path]` include 它，这样不产生依赖边。解析失败必须让测试红，不能把坏文件当空文件扫。

**行为层.** 断言用户能观察到的行为。契约表写成常量数组逐条比对，范本是 `crates/nexus-core/src/error.rs` 里的 `CONTRACT`。

## 门禁

```bash
bash scripts/dev.sh audit
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

`examples/` 是独立 workspace，验证命令是 `cd examples && cargo test --workspace`。

门禁拒绝时的动作是补齐 sanctioned path：登记任务、修产出物、改依赖声明表。不绕过 hook，不改门禁配置，不本地跳过后声称通过。

## 红线

- 不改错误码值、事件名、ID 底层类型。
- 不让内部 cause 进入对外响应或事件负载。
- 不把 `UnknownOutcome` 当可重试处理。
- 不对 enum 变体用 `#[serde(flatten)]`，不给 `ErrorCode` derive `Serialize`。
- 不引入 `unsafe`。release profile 保持 `panic = "unwind"`。
- 不在 `.await` 期间持有 `std::sync::MutexGuard`。
- 标识符只用 ASCII。中文只出现在注释、字符串字面量与文档注释里。
- 不用 `sed` 剥注释做源码扫描。
- 不把凭据写进仓库、日志或错误消息。

## 风格

edition 2024，rustfmt 默认配置，clippy 零告警。注释用中文解释「为什么」，不解释「做了什么」。不做过早抽象，相似三行好过错误的一个抽象。

## 提交

Conventional Commits，标题末尾带**真实存在**的任务号 `(NEX-nn)`，并带 `Co-Authored-By` 尾注。

## 相关文件

- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md`
- `.cursor/rules/rust-code-standards.mdc`
- `.cursor/rules/ai-sdlc-implementation.mdc`
- `.cursor/rules/ai-sdlc-workflow.mdc`
- `.cursor/skills/ai-sdlc/SKILL.md`
- `scripts/check-deps.sh`
- `scripts/check-lock.sh`
- `scripts/check-naming.sh`
- `scripts/dev.sh`
- `backlog/README.md`
