//! `nexus-llm` —— **LLM 网关层**：供应商接入、上下文预算、重试、熔断、失败转移。
//!
//! P3 阶段。后续子模块（预算 / 重试 / 熔断 / 网关主体 / provider）随 P3 的后续任务落地。
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
//! # 失败语义**不重写**
//!
//! 重试判据一律取 `nexus-core` 的 `RetryPolicy`（架构文档 §2.6.3）——
//! `nexus-llm` **不另写一套**「这个错误能不能重试」的判断。
//!
//! 参考实现的 `IsRetryable` 把 504（超时，可能已执行）与 502（明确拒绝）
//! **一视同仁** —— 504 下重试支付类工具会重复扣费。

pub mod budget;
pub mod error;
pub mod types;

pub use budget::{BoundedRequest, bound_chat_request};
pub use error::LlmError;
pub use types::{
    ChatRequest, ChatResponse, Message, ModelDescriptor, Role, StreamEvent, ToolCall, ToolDef,
    Usage,
};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
