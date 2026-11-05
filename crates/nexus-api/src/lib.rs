//! nexus-api —— 协议层（REST + SSE）。
//!
//! # 本 crate 负责什么
//!
//! | 能力 | 模块 |
//! |---|---|
//! | **统一响应包** —— `{"code": 0, "message": "ok", "data"?}` | [`response`] |
//! | **协议层错误响应** —— 只序列化 `UserFacing`（`code` + `message`） | [`error`] |
//! | **`GET /metrics`** —— Prometheus 文本格式的指标端点 | [`metrics`] |
//! | **SSE 帧编解码** —— 逐字节对齐参考实现 + **半帧缓冲** + 帧大小上限 | [`sse`] |
//! | **SSE 桥接** —— 事件流 → 响应体（手写 `Body::from_stream`）+ 断开取消 | [`bridge`] |
//! | **鉴权中间件** —— 身份提取的**机制**（trait 接缝）+ 请求 ID 透传 | [`middleware`] |
//! | **路由骨架** —— 可增量注册的构建器 + 示范性 SSE 端点 + 心跳 | [`router`] |
//!
//! # 本 crate **不**负责什么
//!
//! | 项 | 归属 |
//! |---|---|
//! | 业务用例编排 | `nexus-service` |
//! | 指标**产生**（埋点） | `nexus-observability` |
//! | 模型调用 / 工具执行 | `nexus-llm` / `nexus-tool` |
//! | **鉴权的策略**（JWT 校验 / 配额判定） | `nexus-security`（决策 D10） |
//! | 危险工具的授权边界 | P8（`nexus-tool` / `nexus-mcp`） |
//!
//! # 当前进度
//!
//! **P5 尚未完成。** 本 crate 目前交付了：
//!
//! - `/metrics` 端点（NEX-25）—— 让 NEX-23 产生的网关指标**先可见**；
//! - SSE 帧编解码器（NEX-31）—— **半帧缓冲**是 SSE 协议里唯一真正会出错的地方；
//! - SSE 桥接（NEX-32）—— 12 个事件名映射 + 断开即取消；
//! - 统一响应包（NEX-33）—— 成功 `code == 0`，失败用业务码；
//! - 协议层中间件与路由骨架（NEX-34）—— 鉴权 trait 接缝 + 请求 ID + 心跳；
//! - 解码器帧大小上限（NEX-35）—— 缓冲**有界**，超限报错而非撑爆内存。
//!
//! P5 收口（架构文档同步 + 契约升格）属 NEX-36。

pub mod bridge;
pub mod error;
pub mod metrics;
pub mod middleware;
pub mod response;
pub mod router;
pub mod sse;

pub use bridge::{
    SSE_CONTENT_TYPE, encode_error_frame, encode_event, event_name, sse_body, sse_response,
};
pub use error::ApiError;
pub use metrics::{METRICS_CONTENT_TYPE, METRICS_PATH, MetricsState, metrics_router};
pub use middleware::{
    AuthenticatedIdentity, IdentityExtractor, REQUEST_ID_HEADER, RequestId, RequestIdExtractor,
    identity_middleware, request_id_middleware,
};
pub use response::{ApiResponse, SUCCESS_CODE, SUCCESS_MESSAGE};
pub use router::{DEFAULT_HEARTBEAT_INTERVAL, EventStreamProvider, RouterBuilder, SSE_DEMO_PATH};
pub use sse::{Frame, FrameDecoder, FrameError, MAX_FRAME_BYTES, encode_frame, ping};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
