# P3 LLM 网关 — Tasks

> Spec 层产物（AI-SDLC 三层模型 Tier 2）。
> 契约见 `spec.md`；机器可读的不变量见 `contracts/`。
>
> 布局遵循 spec-kit `v0.8-headings`（每个任务一个 `### T-NNN — <title>` 标题）。
> `taskId` 形如 `T-NNN`，与 `backlog/tasks/` 下的 `NEX-nn` 一一对应。

## Tasks

### T-001 — 落地网关核心类型

`nexus-llm` 的领域类型与供应商描述符。后续所有 P3 任务都建立在这套类型上。

对应 backlog 任务：`backlog/tasks/NEX-16 - feat(llm) 落地网关核心类型.md`
依赖：无
对应设计条款：架构文档 §2.3.2、§2.6.2、§4.2 差异 2

- AC: `crates/nexus-llm/src/types.rs` 落地 `Message` / `Role` / `ToolCall` / `ToolDef` / `ChatRequest` / `Usage` / `ChatResponse` / `StreamEvent` / `ModelDescriptor` 九个类型
- AC: 所有公共类型实现 `Debug`（workspace lint `missing_debug_implementations` 通过）
- AC: `ModelDescriptor` 的 `api_key` 用 `secrecy::SecretString` 承载，`Debug` 输出不含明文口令
- AC: `StreamEvent` 的错误字段不参与序列化，且 `StreamEvent` 不实现 `Deserialize`
- AC: `Role` 是 `enum` 而非 `String`
- AC: `ToolDef` 的控制面字段（`dangerous` / `replay_policy` / `source_identity` / `binding_version` / `capability`）不序列化
- AC: `nexus-llm` 的 `Cargo.toml` 不依赖 `nexus-repo`
- AC: 有契约测试，逐字段对齐参考实现的 `internal/llm/types.go`
- AC: 有负向测试（trybuild 快照入库）：给 `StreamEvent` 序列化错误、给 `ToolDef` 序列化 `dangerous`、访问 `api_key` 私有字段，三者均编译不过
- AC: `cargo fmt --all --check` 通过；`cargo clippy -p nexus-llm --all-targets --locked -- -D warnings` 零告警；`cargo test -p nexus-llm` 通过

### T-002 — 落地上下文预算裁剪

**P3 完成判据的第一半：预算裁剪有测试。**

参考实现的 `boundChatDescriptor`（`internal/llm/context_budget.go:14`）在**每次模型尝试之前**
（含 fallback 与后续 tool_result 轮）重新执行预算裁剪。

对应 backlog 任务：`backlog/tasks/NEX-17 - llm-context-budget-bounding.md`
依赖：T-001
对应设计条款：架构文档 §3.1.1、参考架构 §3.1.1

- AC: 有纯函数 `bound_chat_request(desc, req) -> Result<BoundedRequest, AppError>`，不修改入参
- AC: 输出上限计算符合参考语义：`requested = min(req.max_tokens 或默认 1024, desc.max_output_tokens 或 desc.max_tokens, context_window / 4)`
- AC: framing 开销为 `64 + 16 * len(messages) + 32 * len(tools)`
- AC: 预算不足时返回 `ErrorCode::BudgetExceeded`，且不发起任何上游请求
- AC: 预算错误的 `retry_policy()` 是 `Terminal`
- AC: 边界值测试：恰好等于预算上限则通过，超出 1 字节则拒绝
- AC: 超预算被拒与不超预算通过两个方向的测试都有
- AC: `context_window == 0` 时不裁剪
- AC: 工具定义计入预算
- AC: 裁剪是纯函数：同样的输入产生同样的输出，且入参未被修改
- AC: 两个拒绝条件分别有测试：`requested >= context_window` 与 `body_bytes + framing > context_window - requested`
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-003 — 落地重试退避与熔断器

**失败语义的第一公民**（架构文档 §2.6.3）。

对应 backlog 任务：`backlog/tasks/NEX-18 - llm-retry-and-circuit-breaker.md`
依赖：T-001
对应设计条款：架构文档 §2.6.3、参考架构 §3.1.2 / §3.1.3

- AC: 有 `retry_do` 等价实现：指数退避 + 抖动 + 取消立即中止
- AC: 重试判据用 `nexus-core` 的 `RetryPolicy` —— `Retryable` 才重试，`UnknownOutcome` / `Terminal` 立即返回
- AC: 有测试断言 `UnknownOutcome`（如 `ModelTimeout`）不重试
- AC: 退避延迟有上限（默认 2s），且抖动存在
- AC: 等待退避期间取消能立即中止，不等到延迟结束
- AC: 熔断器用 `std::sync::Mutex` 而非 `tokio::sync::Mutex`
- AC: 熔断器有预热期：样本数不足时不评估错误率
- AC: 熔断打开后拒绝调用；冷却结束后允许试探（半开）
- AC: 打开熔断时窗口被重置（`filled = 0`）
- AC: 取消不计入熔断样本，熔断拒绝也不计入
- AC: 熔断状态的单副本粒度写进公开接口的文档注释与 crate 级文档
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-004 — 落地 OpenAI 兼容供应商客户端与网关回退边界

**P3 完成判据的第二半：回退边界有测试。**

本任务把前三个任务的机制装成网关，并实现 OpenAI 兼容的供应商客户端。

对应 backlog 任务：`backlog/tasks/NEX-19 - llm-openai-compatible-client-and-gateway.md`
依赖：T-001、T-002、T-003
对应设计条款：架构文档 §2.3.2、§3.3、参考架构 §3.1.5

- AC: `nexus-llm` 有 OpenAI 兼容客户端：`chat`（非流式）与 `chat_stream`（流式）
- AC: 客户端不查库 —— 描述符由调用方传入
- AC: 网关装配顺序为：预算裁剪 → 熔断 → 信号量 → 重试 → 供应商 → fallback
- AC: 预算裁剪在每次尝试都重跑（含 fallback 尝试）
- AC: 非流式失败时可回退到 fallback 供应商
- AC: 流式首事件前失败可回退
- AC: 流式首事件后不可回退
- AC: fallback 与 primary 是同一供应商时不切换
- AC: 两边都失败时，错误信息同时含 primary 与 fallback 的信息
- AC: 流式锁定的时序点正确：锁定发生在 yield 之前（下游真正 poll 到的时刻）
- AC: 客户端用本地 mock HTTP server 测试，不依赖真实模型服务
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过

### T-005 — P3 收口：架构文档同步与熔断粒度声明

**收口任务的价值在「核对判据是否真的成立」，不是「重做一遍」。**

逐条核对 AC，若某条已被前面的任务覆盖，就**标注已覆盖**而不是重复实现。

对应 backlog 任务：`backlog/tasks/NEX-20 - P3-closeout-docs-and-breaker-granularity.md`
依赖：T-001、T-002、T-003、T-004
对应设计条款：架构文档 §5.2、§5.3、§5.4、§6.1

- AC: 架构文档 §5.4 不变量表已补 P3 已验证项（逐条对应 10 条）
- AC: 架构文档 §5.3 架构测试表已补 P3 的三项结构层测试
- AC: 架构文档 §6.1 的 P3 行已标记为完成，并写明完成判据的达成方式
- AC: 架构文档明确声明「熔断是单副本粒度」
- AC: `docs/operations/p3-plan.md` 已更新为完成状态，并记录实际交付
- AC: `nexus-llm` 的 crate 级文档写明「不查库」「熔断单副本粒度」「只做 OpenAI 兼容协议」
- AC: 架构文档 §2.6.4.1 的业务码表未被改动（P3 不新增码值），有 diff 证据
- AC: `cargo fmt --all --check` 通过；`cargo clippy --workspace --all-targets --locked -- -D warnings` 零告警；`cargo test --workspace --locked` 通过
- AC: CI 全部门禁全绿
