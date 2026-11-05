//! nexus-api —— 协议层（REST + SSE）。
//!
//! # 本 crate 负责什么
//!
//! | 能力 | 模块 |
//! |---|---|
//! | **协议层错误响应** —— 只序列化 `UserFacing`（`code` + `message`） | [`error`] |
//! | **`GET /metrics`** —— Prometheus 文本格式的指标端点 | [`metrics`] |
//!
//! # 本 crate **不**负责什么
//!
//! | 项 | 归属 |
//! |---|---|
//! | 业务用例编排 | `nexus-service` |
//! | 指标**产生**（埋点） | `nexus-observability` |
//! | 模型调用 / 工具执行 | `nexus-llm` / `nexus-tool` / `nexus-agent` |
//! | SSE 桥接 / 半帧处理 | P5 的其余任务（本 crate 尚未交付） |
//! | 危险工具的授权边界 | P8（`nexus-tool` / `nexus-mcp`） |
//!
//! # 当前进度
//!
//! **P5 尚未完成。** 本 crate 目前只交付了 `/metrics` 一个端点
//! （NEX-25），目的是让 NEX-23 产生的网关指标**先可见** ——
//! 指标产生却不可见，等于没产生。
//!
//! 完整的路由装配、SSE 桥接、鉴权中间件属 P5 的其余任务。

pub mod error;
pub mod metrics;

pub use error::ApiError;
pub use metrics::{METRICS_CONTENT_TYPE, METRICS_PATH, MetricsState, metrics_router};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
