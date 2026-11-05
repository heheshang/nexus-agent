//! `/metrics` 端点 —— 把网关的指标**暴露出来**。
//!
//! # 为什么需要这个端点
//!
//! `nexus-observability::LlmMetrics::render()` 能渲染 Prometheus 文本格式，
//! 但 NEX-23 交付之后**没有任何调用方** ——
//! 指标产生却不可见，等于**没产生**。
//!
//! `ai-sdlc-observability.mdc` §6 要求「可观测：关键路径有指标与结构化日志」。
//! 「有指标」指的是**能被采集的指标**，不是「代码里有个计数器」。
//!
//! # ⚠️ 为什么**不要求鉴权**
//!
//! 决策记录见 `backlog/completed/NEX-25`：
//!
//! - Prometheus 抓取器**通常**没有业务凭据 —— 加鉴权会让标准抓取配置复杂化；
//! - 指标**不含**凭据（标签只有 `provider` / `status` / `kind`）——
//!   所以未鉴权暴露的信息是**低敏**的。
//!
//! **代价**：该端点**只应在内网暴露**。这条约束写进
//! `docs/operations/p3-runbook.md`，不是靠调用方自觉。
//!
//! # ⚠️ 为什么**不会**泄露凭据
//!
//! 架构文档 §2.6.5 与 `nexus-observability` 的 crate 文档都强调：
//! **凭据不得进入埋点**。
//!
//! 这里的保证是**类型级**的 —— `LlmMetrics` 的写入方法
//! （`record_request` / `record_tokens`）**签名上就不接受凭据类型**。
//! 标签只有 `provider` / `status` / `kind` 三个。

use axum::Router;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use nexus_observability::LlmMetrics;

use crate::error::ApiError;

/// Prometheus 文本格式的 Content-Type。
///
/// 规范要求的最小形式是 `text/plain; version=0.0.4`。
/// `charset=utf-8` 是 Prometheus 官方推荐的补充 ——
/// 不加的话，客户端会按 HTTP 默认猜编码。
pub const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// 指标端点的路由路径。
pub const METRICS_PATH: &str = "/metrics";

/// 指标端点的共享状态。
///
/// # 为什么用 `Arc<LlmMetrics>` 而不是自己造单例
///
/// `LlmMetrics` 的计数器**本来就是全局共享的**（`nexus-observability` 内部用
/// `LazyLock`）—— `LlmMetrics::new()` 只是拿到句柄。
///
/// 所以这里**不新建**任何全局状态：状态里放的仍然是从 `nexus-observability`
/// 拿到的同一个句柄。`Arc` 只是为了让 handler 能廉价克隆。
///
/// # 为什么不新建一个 `nexus-api` 自己的注册表
///
/// 因为那会造成**两份互不相干的指标** ——
/// 网关写进 `nexus-observability` 的注册表，端点却渲染另一个注册表，
/// 结果就是「端点返回 200，但指标永远是空的」。
/// 那是**看起来正常、实际失效**的最坏形态。
#[derive(Clone, Debug)]
pub struct MetricsState {
    /// 指标句柄。
    metrics: std::sync::Arc<LlmMetrics>,
}

impl MetricsState {
    /// 构造状态。
    ///
    /// # 幂等
    ///
    /// `LlmMetrics::new()` 是**幂等**的 —— 计数器是全局共享的，
    /// 注册只在首次成功。重复调用不会产生第二份指标。
    pub fn new() -> Self {
        Self {
            metrics: std::sync::Arc::new(LlmMetrics::new()),
        }
    }
}

impl Default for MetricsState {
    fn default() -> Self {
        Self::new()
    }
}

/// 构造只含 `/metrics` 的路由。
///
/// # ⚠️ 这**不是** `nexus-api` 的完整路由
///
/// P5（协议层）整体尚未落地 —— 这里只交付**一个**端点，
/// 便于先让指标可见。完整的路由装配属 P5 的其余任务。
///
/// 保留 `State` 的设计是为了将来能往状态里加更多东西
/// （比如请求上下文、鉴权中间件的产物），而不用改端点签名。
pub fn metrics_router(state: MetricsState) -> Router {
    Router::new()
        .route(METRICS_PATH, get(metrics_handler))
        .with_state(state)
}

/// `GET /metrics` 的处理器。
///
/// # 错误
///
/// 渲染失败（编码器内部错误）→ `500` + `Internal`（90001）。
/// **内部原因绝不外泄**（架构文档 §2.6.5）。
async fn metrics_handler(State(state): State<MetricsState>) -> Result<Response, ApiError> {
    let body = state
        .metrics
        .render()
        .map_err(|err| ApiError::internal(format!("指标渲染失败: {err}")))?;

    Ok(([(CONTENT_TYPE, METRICS_CONTENT_TYPE)], body).into_response())
}
