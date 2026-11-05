---
id: NEX-34
title: "feat(api): 落地协议层中间件与路由骨架"
status: Done
assignee: []
labels: [api, middleware, p5]
priority: high
dependencies:
  - NEX-32
  - NEX-33
references:
  - "docs/specs/p5/spec.md §1.1、§3.2、§3.3、§3.5、§3.6"
  - "docs/specs/p5/tasks.md T-004"
  - "docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §1.4"
---

## Description

决策 D10 的落地：鉴权的**机制**在 P5，**策略**在上层（trait 接缝）。

路由可**增量注册**；交付**一个**真实可跑的 SSE 端点（数据源是注入的 trait）。

### 为什么要 trait 而不是直接调 `nexus-security`

`nexus-security` 是 **7 行空壳** —— JWT 校验的具体形态**还没定**。

直接调它会让 P5 **阻塞于**一个还没做的 crate，而 P5 与 P8
（`nexus-security` 落地）**没有顺序依赖**。trait 让两者**并行**。

参考架构 §0.4 的 Go 版反例 → Rust 落点明确写着
「**权限由 service 注入**的 `SetAccessCheck` 回调完成」。

这与 P4 的 D4（`nexus-agent` 自定义 trait）是**同一条纪律**。

### 必须交付一个**真实可跑**的端点

决策 D9 的反方论点成立：「只交付骨架，端点就是**空壳**」。

所以本任务交付**一个** SSE 端点 —— 它的数据源是一个**注入的**事件流提供器
（trait），所以它**真实可跑**（用本地 mock 就能看到完整 SSE 帧流）。

## 范围边界

### 做

- `crates/nexus-api/src/middleware.rs`：`IdentityExtractor` trait + 中间件
- `crates/nexus-api/src/router.rs`：可增量注册的路由构建器 + 一个 SSE 端点
- `crates/nexus-api/tests/middleware.rs` + `tests/architecture.rs`

### 不做

| 项 | 理由 |
|---|---|
| 具体的 JWT 校验 | `nexus-security` 是空壳；属 P8 |
| 业务端点（KB / 工作流 / 记忆） | 属 P6/P7/P8 |
| `nexus-service` 的实现 | 决策 D9 —— P5 是**传输层** |

## Acceptance Criteria

- [x] #1 `crates/nexus-api/src/middleware.rs` 定义 `IdentityExtractor` trait —— `nexus-api` **不实现**具体的 JWT 校验
- [x] #2 提取失败 → `401` / `403` + 统一响应包
- [x] #3 提取成功 → 身份放进请求 `Extension`，供 handler 取用
- [x] #4 `crates/nexus-api/src/router.rs` 提供**可增量注册**的路由构建器
- [x] #5 交付**一个**真实可跑的 SSE 端点 —— 数据源是**注入的** trait（不依赖 `nexus-service` 的具体实现）
- [x] #6 `nexus-api` **不引用** `nexus-service` 的**具体类型**（同 P4 的 D4 纪律）
- [x] #7 `crates/nexus-api/tests/architecture.rs` 断言 `nexus-api` 不依赖 `nexus-repo` / `nexus-infra`
- [x] #8 `crates/nexus-api/Cargo.toml` **不新增**任何内部 crate 依赖
- [x] #9 路由层注入**心跳** —— 空闲连接按配置间隔写出 `: ping\n\n` 注释帧；桥接**不自带**定时器（spec §3.2）
- [x] #10 **请求 ID 透传** —— 中间件为每个请求分配或透传请求 ID，放进请求 `Extension` 与响应头（spec §1.1）
- [x] #11 `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

## DoR 判定

> **判定日期**：2026-11-05（AI-SDLC S3）
> **判定方式**：Stage A（确定性）+ Stage B（语义）逐项手工核对。

七项门禁逐项判定（判据见 `.cursor/rules/ai-sdlc-dor-gate.mdc` §3）。

### #1 验收条件可二元测试

11 条 AC 全部可二元判定 —— 每条都指明「trait / 函数存在 / 行为可观测 / 命令退出码」。

### #2 无未决标记

无 `TBD` / `???` / 待定项。

「哪些端点挂鉴权」是**显式排除**（范围边界表），不是未决问题 ——
P5 交付**机制**，逐端点决定由上层做。

### #3 引用可解析

- `docs/specs/p5/spec.md §3.3、§3.5、§3.6` —— 存在
- `docs/specs/p5/tasks.md T-004` —— 存在
- `docs/architecture/2026-11-04-nexusagent-rust-architecture-design.md §1.4` —— 存在
- `scripts/check-deps.sh`（依赖方向表）—— 存在

### #4 范围有界

**单 crate 内部**（`nexus-api`）新增两个模块 + 测试。

**不触及**：42 个业务码、12 个事件名、18 个 ID 类型、crate 依赖方向、对外响应形状。

### #5 受影响面被指名

| 位置 | 影响 |
|---|---|
| `crates/nexus-api/src/middleware.rs` | **新增** |
| `crates/nexus-api/src/router.rs` | **新增** |
| `crates/nexus-api/src/lib.rs` | **修改** —— 导出模块 |
| `crates/nexus-api/tests/middleware.rs` | **新增** |
| `crates/nexus-api/tests/architecture.rs` | **新增/修改** |
| `crates/nexus-api/Cargo.toml` | **不修改** |

### #6 完成态可描述

`nexus-api` 有一个可增量注册的路由构建器、一个**真实可跑**的 SSE 端点，
以及一套**可替换**的鉴权机制（trait 接缝）。

### #7 无隐形依赖

- 前置任务 NEX-32 / NEX-33 均已登记在 `backlog/tasks/`
- `axum` / `tower` / `tokio` 已在 `nexus-api` 的依赖里
- `nexus-agent` / `nexus-core` 已在依赖里
- 无其他未言明的结构性假设

### 结论

**pass** —— 七项全过，但需 NEX-32 / NEX-33 完成后开工。

---

## 决策记录

### 类别：decide-and-proceed

- **决策**：SSE 端点的数据源用 **trait**（`EventStreamProvider` 之类）注入，
  而不是直接调 `nexus-service`。
- **依据**：决策 D9；架构文档 §1.4「协议层不能感知业务规则」；
  参考架构 §0.4「权限由 service 注入」的同构做法。
  同时这让 T-004 的测试能**不依赖任何业务实现**就跑起来。
- **可逆性**：高 —— trait 是接口，后续填实现**不改** `nexus-api`。
- **影响面**：`crates/nexus-api/src/router.rs`。

### 类别：decide-and-proceed

- **决策**：请求 ID 的**头名**取 `x-request-id`，**类型**取协议层**自有**的
  `RequestId` 新类型（`String` 底座），**不**复用 `nexus_core::id::TraceId`。
- **依据**：
  - 头名与类型**都未在架构文档 §6.3 定稿** —— 属于「合理工程师会有分歧」
    但**可低成本回滚**的范畴，按 `ai-sdlc-decision-authority.mdc` §2.2 归 (a)；
  - **不**复用 `TraceId`：`TraceId` 是**追踪** ID（跨系统），
    请求 ID 是**单跳**标识 —— 两者生命周期不同。
    混用会让「一次请求」与「一条追踪」在日志里对不上；
  - **不**用裸 `String`：裸 `String` 在 `Extension` 里会与其它字符串扩展**撞类型**。
- **可逆性**：高 —— 头名改名只需改一个常量；类型改名只影响本 crate。
- **影响面**：`crates/nexus-api/src/middleware.rs`。

### 类别：decide-and-proceed

- **决策**：透传进来的请求 ID **必须校验**（长度 ≤ 128 且字符限于
  ASCII 字母 / 数字 / `-` / `_` / `.`），否则**丢弃并重新生成**。
- **依据**：上游传来的头是**不可信输入**，而它会被**回显**到响应头。
  无校验就回显等于把攻击者控制的内容注入响应头 ——
  含 CR / LF 的值会造成**响应拆分**（response splitting）。
  这是**安全**判据，不是「格式偏好」。
- **可逆性**：高 —— 判据是本 crate 内部的一个函数。
- **影响面**：`crates/nexus-api/src/middleware.rs`。

### 类别：decide-and-proceed

- **决策**：`nexus-api` **只引用** P5 白名单内的 `Unauthorized` / `Forbidden`；
  拒绝响应的状态码按 **`AppError` 的 HTTP 状态**判定（401 / 403），
  而**不是**按 `ErrorCode` 变体逐个列举。
- **依据**：`scripts/check-specs.sh` 规则 5 禁止本 Spec 的 crate
  使用白名单外的错误码。若按变体列举「凭证过期 / 凭证无效 / 登录失败」，
  就得引用 `TokenExpired` / `TokenInvalid` / `LoginFailed` —— **契约越界**。
  按 HTTP 状态判定则：401 / 403 原样透出（**不**改业务码，
  客户端拿到的仍是**精确**原因），其它状态 → 500。
- **可逆性**：高 —— 判定逻辑在本 crate 内部。
- **影响面**：`crates/nexus-api/src/middleware.rs`。

### 类别：decide-and-proceed

- **决策**：`RouterBuilder` 提供**两个** SSE 端点注册方法 ——
  `register_sse_demo`（公开）与 `register_sse_demo_protected`（受保护），
  而不是**一个**带 `protected: bool` 参数的方法。
- **依据**：让调用方在**调用点**就能看出意图。
  `protected: bool` 会让调用方有机会「传个 `false` 就忘了」——
  那正是「看起来完成了、实际没保护」的形态。
- **可逆性**：高 —— 加/减方法即可。
- **影响面**：`crates/nexus-api/src/router.rs`。

## 实现过程中的发现

### 发现 1 —— `Result<_, Response>` 触发 clippy 的 `result_large_err`

`Response` 是 128 字节。`Result<AuthenticatedIdentity, Response>` 的
`Err` 变体太大 —— clippy 的 `result_large_err` 直接拒绝
（`-D warnings` 下是**硬门禁**）。

**修正**：改成 `Option<Response>` ——
「有拒绝响应就返回它」是**同一个意思**，且没有那个开销。

**教训**：`Result` 的 `Err` 变体大小是个**真实的**门禁约束，
不只是风格问题。协议层里凡是「错误分支要产出一个响应」的地方，
都会撞上它。

### 发现 2 —— `from_fn_with_state` 的泛型 `E` 推断不出来

`axum::middleware::from_fn_with_state(state, f)` 需要 `f` 的**具体**状态类型。
构建器里存的是 `Arc<dyn IdentityExtractor>` —— 泛型 `E` **推不出来**。

最初想用 `impl<T> IdentityExtractor for Arc<T>` 的 blanket impl 解决，
但 `E` 仍然是**两个**候选（`Arc<dyn IdentityExtractor>` 与 `Arc<T>`），
推断失败。

**修正**：中间件函数写成**具体类型**的包装
（`State<Arc<dyn IdentityExtractor>>`），
逻辑委托给 `middleware::authenticate`（**唯一**实现处）。

**教训**：两处写一遍鉴权逻辑，**迟早**会分叉（改了一处忘了另一处）。
抽成单一入口 `authenticate` 后，中间件本体只有 3 行。

### 发现 3 —— `Sleep` 是 `!Unpin`

心跳定时器要放进 `unfold` 的状态里，又要被 `select!` 安全地借用。
`tokio::time::Sleep` 带 `PhantomPinned` —— **不能**直接放进状态。

**修正**：`Pin<Box<Sleep>>`。

### 发现 4 —— **心跳的「空闲」不能用 `tokio::time::interval`**

`Interval` 按**固定节奏**触发 —— 与「上次什么时候写的」无关。
那样**繁忙时也会穿插**心跳帧（明明不空闲）。

**修正**：用 `Sleep`（**一次性**定时器），每次写完都
`reset(now + interval)` —— 那才是真正的**空闲**判定。

### 发现 5 —— 提取失败**不能**硬编码成 401

「已认证但无权访问」是 **403**（`Forbidden`）——
与「缺少凭证」（401）语义**不同**。
硬编码 401 会让前端**无法区分**「登录一下就好」与「别试了」。

**修正**：状态码由 `AppError` 的业务码决定。

### 发现 6 —— 故障注入**实际发现**了一个**太弱**的断言

第 3 轮故障注入里，「心跳禁用被忽略」（把 `None` 变成默认间隔 15s）
这一项**未变红**。

查下来是断言**太弱**：`the_heartbeat_is_disabled_when_the_interval_is_absent`
只等 300ms，而默认间隔是 15s ——
「禁用心跳」与「心跳间隔恰好很长」在这个窗口里**表现一样**。

**修正**：变异改成「`None` → 50ms」—— 那样心跳会在 300ms 内到达，断言变红。
断言本身**保持** 300ms 窗口（它已足够短）。

**教训**：「未变红」有两种成因：变异无效，或**断言抓不住**那类变异。
两者都必须查清 —— 前者要换变异，后者要改断言。
**不分清就会把「覆盖盲区」误当成「变异无效」**（NEX-33 发现 3 的同一条教训）。

### 发现 7 —— `check-specs.sh` 规则 5 **真的拦住了**契约越界

最初按 `ErrorCode` **变体**列举鉴权错误，引用了
`TokenExpired` / `TokenInvalid` / `LoginFailed` ——
它们**不在** P5 的白名单里。

门禁报红：

```
✗ p5（crates/nexus-api）使用了本 Spec 白名单外的错误码：LoginFailed
```

**修正**：改按 **HTTP 状态**判定（见上面的决策记录）。

**教训**：这条门禁**不是**形式主义 ——
它拦住了一次真实的「顺手引用了别人的码」。
若没有它，P5 就会静默用上 P8 的错误码。

## 故障注入（13 项，最终全部**确实会红**）

在隔离工作树（`/tmp/nex34-verify`）里逐项注入变异，确认对应测试**失败**。

| # | 注入 | 结果 |
|---|---|---|
| 1 | 提取失败仍放行 | FAILED ✓ |
| 2 | 拒绝响应不用统一响应包 | FAILED ✓ |
| 3 | 403 被映射成 401 | FAILED ✓ |
| 4 | 身份不进 `Extension` | FAILED ✓ |
| 5 | 请求 ID 不生成（恒为固定值） | FAILED ✓ |
| 6 | 请求 ID 不透传（总是重新生成） | FAILED ✓ |
| 7 | 透传值不校验（直接回显任意内容） | FAILED ✓ |
| 8 | 请求 ID 不进响应头 | FAILED ✓ |
| 9 | 心跳不写（写空字节） | FAILED ✓ |
| 10 | 心跳禁用被忽略（`None` → 很短的间隔） | FAILED ✓ |
| 11 | 没有数据源也注册端点（空 provider 顶上） | FAILED ✓ |
| 12 | `nexus-api` 引用 `nexus-service` | FAILED ✓ |
| 13 | 桥接自带定时器 | FAILED ✓ |

⚠️ 其中 #10 **第一轮未变红** —— 查出是**断言太弱**（发现 6），
换有效变异后变红。

⚠️ #12 的第一版变异引用了**不存在的符号**（编译失败）——
那不算「断言生效」。改用**真实存在**的 `nexus_service::CRATE_NAME` 后变红。

## 验证结果

- `cargo test -p nexus-api`：84 个测试全绿
  （34 单元 + 6 结构 + 7 指标 + 13 中间件 + 7 响应包 + 8 路由 + 9 桥接）
- `cargo test --workspace --locked`：532 个测试全绿，0 失败
- `cargo fmt --all --check` 通过
- `cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警
- `scripts/check-lock.sh` / `check-deps.sh` / `check-naming.sh` /
  `check-backlog.sh` / `check-specs.sh` 通过
- ⚠️ `scripts/check-rules.sh` / `check-docs.sh` **失败**——
  但**与本次改动无关**：`docs/ai-sdlc/` 下的文件在工作区里被删除
  （未提交的工作区状态），与 NEX-34 无关。
