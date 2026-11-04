//! Milvus 客户端构造。
//!
//! `nexus-infra` 负责 PG / Redis / S3 / Milvus 的**客户端构造**，不含任何业务语义
//! （架构文档 §4.3.3）。本模块是其中的 Milvus 部分。
//!
//! # 边界：连接 vs schema 构造
//!
//! 架构文档 §4.3.3 把参考实现的 `internal/infra/milvus.go` 拆成两个 crate：
//!
//! ```text
//! nexus-infra    （数据层）    Milvus 连接、schema/index 构造、PG、Redis、S3
//! nexus-vector   （能力层）    Store trait + ScopedStore trait + Scope 类型 + Milvus 实现
//! ```
//!
//! 依赖方向 `vector → infra`。本模块负责**连接**；
//! collection / schema / index 的构造属于 `nexus-vector`。
//!
//! # 为什么 Milvus 客户端是**异步**构造
//!
//! `milvus-sdk-rust 3.0.3` 的 `Client::new` 是 `async` ——
//! 它内部会 `dst.connect().await`（实测 `milvus-sdk-rust-3.0.3/src/v1/client.rs:203`）
//! 真的建一条 gRPC 通道。所以**构造失败 = 服务不可达**，
//! 必须返回 `Result`，不能 panic。
//!
//! # ⚠️ 「构造成功」**不等于**「Milvus 可用」
//!
//! 实测（`tonic-0.13.1/src/transport/channel/service/connection.rs:90`）：
//! `Connection::connect` 走的是 `ready_oneshot()` ——
//! 那只等**连接器**就绪（**TCP 建连**），HTTP/2 协议握手是**延迟**的。
//!
//! 后果：对端「接受 TCP 但不回应」时，`connect()` **会成功**，
//! 错误要到**第一次真正的 RPC** 才暴露。
//!
//! 这不是 bug，是 gRPC 客户端的常态。但调用方必须知道 ——
//! **不能**把「`connect()` 返回 `Ok`」当成「Milvus 已就绪」。
//! 需要真正的就绪判据时，应调用一个轻量的 RPC（如 `list_collections`）。
//!
//! # 配置来自 `nexus-config`
//!
//! 变量名由 `nexus-config` 的映射清单统一定义（`MILVUS__URL` → `milvus.url`）。
//! 本模块**不自己读环境变量** —— 否则会出现两套变量名约定，清单与实现脱节。

use std::time::Duration;

use milvus_sdk::client::ClientBuilder;
use nexus_config::ConfigStack;
use nexus_core::error::{AppError, ErrorCode};

/// Milvus URL 的配置路径。
const PATH_URL: &str = "milvus.url";

/// RPC 超时（秒）的默认值。
///
/// 与 `milvus-sdk-rust` 内部的 `RPC_TIMEOUT` 一致
/// （实测 `milvus-sdk-rust-3.0.3/src/v1/config.rs`：`Duration::new(10, 0)`）。
///
/// **本模块不新增配置项** —— `nexus-config` 的映射清单里没有 `milvus.timeout_secs`。
/// Milvus 是**纯派生 · 可重建**（§1.3），
/// 为它开一个必须配准的超时旋钮，只会让部署多一个失败点。
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// **整体**连接预算（秒）。
///
/// ⚠️ 这个值**必须存在**，而且必须作用在**我们这一侧**。
///
/// milvus 的 `ClientBuilder::timeout()` 只设置 tonic 的
/// **`Endpoint::timeout`** —— 那是**请求**超时，不是**建连**超时
/// （实测 `milvus-sdk-rust-3.0.3/src/v1/client.rs:180-183`：
/// `dst = dst.timeout(timeout)`；建连走 `dst.connect().await`，
/// 其连接超时是 tonic 的 `connect_timeout`，**未被设置**）。
///
/// 结果：连一个「接受 TCP 但不回应」的地址时，
/// `ClientBuilder::build()` 会**一直挂着**，直到操作系统的 TCP 超时（数分钟）。
///
/// 对一个「纯派生 · 可重建」的依赖来说，那不可接受 ——
/// §1.3 说 Milvus 不可用时应降级为「仅关键词」检索，而不是让进程卡死。
///
/// 所以：用 `tokio::time::timeout` 给**急切构造**套一个总预算。
/// 超了就返回错误让调用方降级。
pub const DEFAULT_CONNECT_BUDGET_SECS: u64 = 3;

/// Milvus 客户端的配置。
#[derive(Debug, Clone)]
pub struct MilvusSettings {
    /// 服务地址（`http://host:port` 或 `https://host:port`）。
    url: String,
    /// RPC 超时。
    timeout: Duration,
}

impl MilvusSettings {
    /// 从配置栈读取。
    ///
    /// # 错误
    ///
    /// `milvus.url` **缺失或为空**时返回 [`ErrorCode::InvalidParam`]。
    ///
    /// Milvus 是**纯派生 · 可重建**（§1.3）——
    /// 缺它不是致命错误，但「配置里说要用 Milvus 却没配地址」是**明确的配置错误**。
    pub fn from_config(config: &ConfigStack) -> Result<Self, AppError> {
        let url = config
            .get_str(PATH_URL)
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| {
                AppError::new(ErrorCode::InvalidParam, "缺少 Milvus 连接配置")
                    .with_cause_msg(format!("配置路径 {PATH_URL} 缺失或为空"))
            })?;

        // 超时用缺省值 —— 见 `DEFAULT_TIMEOUT_SECS` 的说明。
        let timeout = Duration::from_secs(DEFAULT_TIMEOUT_SECS);

        Ok(Self { url, timeout })
    }

    /// 服务地址。
    pub fn url(&self) -> &str {
        &self.url
    }

    /// RPC 超时。
    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

/// Milvus 客户端的句柄。
///
/// 包装 [`milvus_sdk::client::Client`]，而不是直接导出它 ——
/// 这样「客户端从哪来」只有一个入口。
#[derive(Clone)]
pub struct MilvusHandle {
    client: milvus_sdk::client::Client,
}

impl MilvusHandle {
    /// 底层 Milvus 客户端。
    pub fn client(&self) -> &milvus_sdk::client::Client {
        &self.client
    }

    /// 取出底层 Milvus 客户端（所有权）。
    pub fn into_client(self) -> milvus_sdk::client::Client {
        self.client
    }
}

impl std::fmt::Debug for MilvusHandle {
    /// **手写** `Debug` —— 只输出非敏感字段。
    ///
    /// `milvus_sdk::client::Client` 派生 `Debug`
    /// （实测 `milvus-sdk-rust-3.0.3/src/v1/client.rs:147`），
    /// 内部持有 `AuthInterceptor { token: Option<String> }`
    /// —— 那个 token 是 `base64(username:password)`（`src/v1/client.rs:187-192`），
    /// **派生 `Debug` 会把它打印出来**。
    ///
    /// 本仓库不把「安全」外包给依赖的某个版本实现。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MilvusHandle").finish_non_exhaustive()
    }
}

/// 从配置栈构造 Milvus 客户端并**建立连接**。
///
/// # 错误
///
/// - 配置缺失或非法 → [`ErrorCode::InvalidParam`]
/// - 连不上 Milvus → [`ErrorCode::StorageUnavailable`]
///
/// **不 panic** —— Milvus 不可达是**运行时故障**，不是编程错误。
///
/// ⚠️ Milvus 是**纯派生 · 可重建**（§1.3）——
/// 检索不可用时应降级为「仅关键词」（§1.3 的存储职责表），
/// 而不是让整个进程起不来。
pub async fn connect(config: &ConfigStack) -> Result<MilvusHandle, AppError> {
    let settings = MilvusSettings::from_config(config)?;
    connect_with(settings).await
}

/// 用给定配置构造 Milvus 客户端并建立连接。
pub async fn connect_with(settings: MilvusSettings) -> Result<MilvusHandle, AppError> {
    // `ClientBuilder::build()` 是 async —— 内部会真的建一条 gRPC 通道。
    // `ClientBuilder::new` 要求 `D: 'static`（`TryInto<Endpoint> + Clone`）——
    // 所以传**拥有所有权**的 `String`，不能传 `&str` 借用。
    //
    // ⚠️ 外面套一层 `tokio::time::timeout` ——
    // milvus 设置的 `Endpoint::timeout` 是**请求**超时，不是**建连**超时。
    // 连一个「接受 TCP 但不回应」的地址时，建连会挂到 OS 的 TCP 超时（数分钟）。
    let budget = Duration::from_secs(DEFAULT_CONNECT_BUDGET_SECS);
    let client = tokio::time::timeout(
        budget,
        ClientBuilder::new(settings.url().to_string())
            .timeout(settings.timeout())
            .build(),
    )
    .await
    .map_err(|_| {
        AppError::new(ErrorCode::StorageUnavailable, "向量库暂不可用，请稍后再试").with_cause_msg(
            format!(
                "连接目标：{}（超过 {} 秒预算）",
                settings.url(),
                DEFAULT_CONNECT_BUDGET_SECS
            ),
        )
    })?
    .map_err(|e| {
        AppError::new(ErrorCode::StorageUnavailable, "向量库暂不可用，请稍后再试")
            .with_cause(e)
            .with_cause_msg(format!("连接目标：{}", settings.url()))
    })?;

    Ok(MilvusHandle { client })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stack_with(milvus: serde_json::Value) -> ConfigStack {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({ "milvus": milvus }));
        stack
    }

    fn full_stack() -> ConfigStack {
        stack_with(json!({
            "url": "http://127.0.0.1:19530"
        }))
    }

    #[test]
    fn reads_all_settings_from_config() {
        let settings = MilvusSettings::from_config(&full_stack()).unwrap();

        assert_eq!(settings.url(), "http://127.0.0.1:19530");
    }

    #[test]
    fn timeout_uses_the_default() {
        let stack = stack_with(json!({ "url": "http://127.0.0.1:19530" }));
        let settings = MilvusSettings::from_config(&stack).unwrap();

        assert_eq!(
            settings.timeout(),
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        );
    }

    #[test]
    fn missing_url_is_an_error() {
        let stack = stack_with(json!({}));
        let err = MilvusSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn empty_url_is_an_error() {
        let stack = stack_with(json!({ "url": "   " }));
        let err = MilvusSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[tokio::test]
    async fn connect_to_an_unreachable_milvus_returns_error_not_panic() {
        // 端口 1 上不会有 Milvus —— 连接必然失败。
        let stack = stack_with(json!({ "url": "http://127.0.0.1:1" }));
        let err = connect(&stack).await.unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::StorageUnavailable);
        assert!(err.cause.is_some(), "内部原因应保留供日志使用");
    }
}
