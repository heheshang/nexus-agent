//! `nexus-llm` —— **LLM 网关层**：供应商接入、上下文预算、重试、熔断、失败转移。
//!
//! P3 **已完成**：协议类型、预算裁剪、重试、熔断、供应商客户端、网关回退边界。
//!
//! # 协议范围：**只做 OpenAI 兼容协议**
//!
//! deepseek / openai / qwen / zhipu / moonshot / doubao 都是同一套协议；
//! 本地 ollama 也走 `/v1` 兼容层。
//!
//! **不做** ollama 原生协议（`/api/chat`）—— 那是**另一套字段形状**，
//! 在只有一个供应商实现时引入第二套协议没有收益。
//! 契约载体：`docs/specs/p3/contracts/invariants.json` 的决策 **D2**。
//!
//! # 职责边界
//!
//! | 本 crate 负责 | 本 crate **不**负责 |
//! |---|---|
//! | 网关协议类型（消息 / 请求 / 响应 / 流事件） | 解析 `model_configs`（那是 `nexus-service` 的事） |
//! | 上下文预算裁剪 | 决定用哪个模型（配置驱动，上层决定） |
//! | 重试、熔断、并发控制 | 业务语义（怎么用模型输出） |
//! | 失败转移（fallback）编排 | 共享配额的跨进程协调（那是容量治理的事） |
//!
//! # 关键约束：**网关不查库**
//!
//! 参考实现的 `llm.Gateway` **不查库** —— `model_configs` → `ModelDescriptor`
//! （含解密后的 `api_key`）由 service 层解析后传入。
//!
//! Rust 版用 **Cargo 依赖图**保证这一点：
//! `nexus-llm` 的 `Cargo.toml` **不出现** `nexus-repo`。
//!
//! # 供应商多态：**enum 而非 `dyn`**
//!
//! 架构文档 §2.3.2 明确：供应商是「配置驱动」的，但用 **enum + match** 表达。
//!
//! | 理由 | 说明 |
//! |---|---|
//! | 穷尽匹配 | 新增供应商时编译器列出所有需要改的 `match` 点 |
//! | 无虚调用开销 | 热路径（每次模型调用） |
//! | 错误类型精确 | 每个变体可以有各自的错误 |
//!
//! 代价：新增供应商要改 enum —— 但那本来就是「新增一种模型接入方式」。
//!
//! # 熔断是**单副本粒度**
//!
//! 熔断状态是**进程内**的（`std::sync::Mutex` + 环形缓冲）。
//! 多副本部署时每个副本各有一套 —— 会出现「A 副本熔断、B 副本继续打」的不一致。
//!
//! 这是参考实现的已知局限（参考架构文档 §5.3 局限 #3），
//! 本阶段**明确接受**并声明 —— 外置到 Redis/PG 属后续特性，不在 P3 范围。
//!
//! # 回退边界（架构文档 §3.3）
//!
//! - **非流式**：primary 失败 → 可回退到 fallback。
//! - **流式**：**首事件之前**失败 → 可回退；**首事件之后** → **不可回退**。
//!
//! 原因：客户端已收到一半内容，切换模型会产生内容拼接错乱。
//!
//! 这个边界用**类型**表达（[`gateway::StreamOutcome`]）——
//! 回退的唯一入口是 `FailedBeforeStart`，编译期就没法在流已建立后回退。
//!
//! # 可观测
//!
//! 网关的 span 分三层：
//!
//! | span | 覆盖范围 | 生命周期 |
//! |---|---|---|
//! | `llm.chat` | 一次业务调用（含 fallback） | 调用结束 |
//! | `llm.attempt` | 一次供应商尝试 | 该次尝试结束（重试会产生多个） |
//! | `llm.stream` | 一次流式调用 | **流关闭时** |
//!
//! ⚠️ `llm.stream` 的生命周期**绑定流**，不是绑定「拿到流」。
//!
//! 参考实现的 `spanOnClose` 注释原文：
//! 「若放进 `runWithGuard`，拿到通道就返回，span 只有几毫秒，等于没测」。
//!
//! `nexus_observability::StreamSpan::wrap` **消耗** span（而不是克隆）——
//! 因为 `tracing` 的 span 是**引用计数**的，
//! 只有**最后一个**句柄被 drop 时才触发 `on_close`。
//! 留一份克隆在别处，span 就**永远不会关闭**。
//!
//! 指标：`llm_requests_total`（标签 `provider` / `status`）与
//! `llm_tokens_total`（标签 `provider` / `kind`）。
//! `status` 的 `canceled` **单独计数** —— 取消不是供应商的故障。
//!
//! # 配置接线
//!
//! [`ProviderConfig::from_config`] 与 [`GatewayConfig::from_config`]
//! 从 [`nexus_config::ConfigStack`] 读配置 ——
//! 映射表里**早就有** `NX_LLM__REQUEST_TIMEOUT_SECS` 等条目，
//! 但如果不读，运维设了环境变量**不生效**，
//! 而 `nexus config show` 会**显示**它已生效。那是**静默失效**。
//!
//! 配置缺失时**回退到缺省值**而不是报错 ——
//! 让「忘了配一项」变成「服务不可用」不合理。
//! 但 **0 值必须回退**：0 超时会让每个请求立即失败。
//!
//! # 失败语义**不重写**
//!
//! 重试判据一律取 `nexus-core` 的 `RetryPolicy`（架构文档 §2.6.3）——
//! `nexus-llm` **不另写一套**「这个错误能不能重试」的判断。
//!
//! 参考实现的 `IsRetryable` 把 504（超时，可能已执行）与 502（明确拒绝）
//! **一视同仁** —— 504 下重试支付类工具会重复扣费。

pub mod breaker;
pub mod budget;
pub mod convert;
pub mod error;
pub mod gateway;
pub mod protocol;
pub mod provider;
pub mod retry;
pub mod sse;
pub mod types;

pub use breaker::{BreakerConfig, BreakerRegistry, CircuitBreaker};
pub use budget::{BoundedRequest, bound_chat_request};
pub use convert::build_request_body;
pub use error::LlmError;
pub use gateway::{CallTarget, Gateway, GatewayConfig, StreamHandle, StreamOutcome};
pub use provider::{OpenAiProvider, ProviderConfig};
pub use retry::{CallOutcome, RetryConfig, RetryOutcome, is_retryable, record_outcome, retry_do};
pub use types::{
    ChatRequest, ChatResponse, Message, ModelDescriptor, Role, StreamEvent, ToolCall, ToolDef,
    Usage,
};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
