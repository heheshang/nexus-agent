//! nexus-api —— 协议层（REST + SSE）。
//!
//! # 本 crate 负责什么
//!
//! | 能力 | 模块 |
//! |---|---|
//! | **统一响应包** —— `{"code": 0, "message": "ok", "data"?}` | [`response`] |
//! | **协议层错误响应** —— 只序列化 `UserFacing`（`code` + `message`） | [`error`] |
//! | **`GET /metrics`** —— Prometheus 文本格式的指标端点 | [`metrics`] |
//! | **SSE 帧编解码** —— 逐字节对齐参考实现 + **半帧缓冲** | [`sse`] |
//! | **SSE 桥接** —— 事件流 → 响应体（手写 `Body::from_stream`）+ 断开取消 | [`bridge`] |
//!
//! # 本 crate **不**负责什么
//!
//! | 项 | 归属 |
//! |---|---|
//! | 业务用例编排 | `nexus-service` |
//! | 指标**产生**（埋点） | `nexus-observability` |
//! | 模型调用 / 工具执行 | `nexus-llm` / `nexus-tool` / `nexus-agent` |
//! | 鉴权中间件 / 路由骨架 | P5 的其余任务 |
//! | 危险工具的授权边界 | P8（`nexus-tool` / `nexus-mcp`） |
//!
//! # 当前进度
//!
//! **P5 尚未完成。** 本 crate 目前交付了：
//!
//! - `/metrics` 端点（NEX-25）—— 让 NEX-23 产生的网关指标**先可见**；
//! - SSE 帧编解码器（NEX-31）—— **半帧缓冲**是 SSE 协议里唯一真正会出错的地方；
//! - SSE 桥接（NEX-32）—— 12 个事件名映射 + 断开即取消；
//! - 统一响应包（NEX-33）—— 成功 `code == 0`，失败用业务码。
//!
//! 鉴权中间件与路由骨架属 P5 的其余任务。

pub mod bridge;
pub mod error;
pub mod metrics;
pub mod response;
pub mod sse;

pub use bridge::{
    SSE_CONTENT_TYPE, encode_error_frame, encode_event, event_name, sse_body, sse_response,
};
pub use error::ApiError;
pub use metrics::{METRICS_CONTENT_TYPE, METRICS_PATH, MetricsState, metrics_router};
pub use response::{ApiResponse, SUCCESS_CODE, SUCCESS_MESSAGE};
pub use sse::{Frame, FrameDecoder, FrameError, encode_frame, ping};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
