//! `nexus-core` —— 跨 crate 共享的**纯类型**。
//!
//! # 职责边界
//!
//! 本 crate 只放**跨 crate 共享的纯数据类型与无状态工具**。
//!
//! # 明确**不做**的事
//!
//! - ❌ 不引入 sqlx / axum / reqwest 等重依赖
//! - ❌ 不放任何**有行为**的编排逻辑
//! - ❌ 不做业务判断
//!
//! # 子模块
//!
//! | 模块 | 内容 | 设计条款 |
//! |---|---|---|
//! | [`id`] | 类型化 ID（18 种） | 架构文档 §2.8.1 |
//! | [`cancel`] | 取消语义类型状态 | 架构文档 §2.1 |
//! | [`event`] | 事件负载（12 种）+ 单写者事件流 | 架构文档 §2.2 |
//! | [`error`] | 三层错误模型 + 42 个业务码 | 架构文档 §2.6 |
//! | [`canonical`] | 规范化 JSON + 定义哈希 | 架构文档 §3.5 |
//!
//! # 依赖位置
//!
//! 本 crate 是**依赖图的根**（架构文档 §4.4 规则 2：零内部依赖）。
//! 它的依赖只有外部 crate —— `serde` / `uuid` / `thiserror` / `tokio-util` / `futures`。

pub mod cancel;
pub mod canonical;
pub mod error;
pub mod event;
pub mod id;

pub use cancel::{DetachedScope, PersistError, RequestScope};
pub use canonical::{CanonicalizeError, canonical_hash, canonical_hash_hex, canonicalize};
pub use error::{ALL_CODES, AppError, ErrorCode, ModelError, RetryPolicy, UserFacing};
pub use event::{
    AgentEventKind, EVENT_NAMES, Event, EventChannel, EventPayload, EventSink, EventStream,
    EventStreamError, FinishReason, ToolStatus,
};
pub use id::{
    AgentId, CaseId, DatasetId, DocumentId, EmbeddingConfigId, ID_TYPE_NAMES, InterruptId, KbId,
    MessageId, ModelConfigId, ProviderId, RunId, SessionId, ToolCallId, ToolId, TraceId, UserId,
    VectorId, WorkflowId,
};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
