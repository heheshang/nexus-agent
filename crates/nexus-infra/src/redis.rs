//! Redis 客户端构造。
//!
//! `nexus-infra` 负责 PG / Redis / S3 / Milvus 的**客户端构造**，不含任何业务语义
//! （架构文档 §4.3.3）。本模块是其中的 Redis 部分。
//!
//! # Redis 的定位：纯派生，可丢弃
//!
//! 架构文档 §1.3 的存储职责表：
//!
//! | 存储 | 权威性 | 不可用时 |
//! |---|---|---|
//! | PostgreSQL | **唯一权威** | 业务不可用 |
//! | Redis | 纯派生 · 可丢弃 | **降级但可用** |
//!
//! §1.3.1 把它变成**可执行断言**：
//!
//! > 如果移除 Redis，系统必须照常工作（性能下降，功能不变）。
//!
//! 这条断言决定了本模块的设计：**Redis 客户端的构造失败绝不能阻断 PG 侧**。
//! 调用方拿到 `Err` 后应当**降级**（退回 PG 轮询），而不是让整个进程起不来。
//!
//! # 为什么用 `ConnectionManager` 而不是裸 `Client`
//!
//! `redis::Client` 本身**不持有连接** —— 每次 `get_connection()` 都新建一条。
//! `ConnectionManager` 持有一条**可自动重连**的连接，断线后自行恢复。
//!
//! redis 2.0 的关键 API 变化（实测 `redis-2.0.0-rc.2/src/aio/connection_manager.rs:434`）：
//!
//! ```text
//! pub async fn new(client: Client) -> RedisResult<Self>
//! ```
//!
//! 1.x 是**同步**构造。2.0 改成 async —— 因为它内部会**真的建一次连接**
//! （`new_with_config` 里 `load().await`），
//! 以便失败时立刻返回错误而不是留下一个坏的管理器。
//!
//! # 配置来自 `nexus-config`
//!
//! 变量名由 `nexus-config` 的映射清单统一定义（`REDIS__URL` → `redis.url`）。
//! 本模块**不自己读环境变量** —— 否则会出现两套变量名约定，清单与实现脱节。
//!
//! **本模块不新增配置项** —— 只消费 `redis.url`。
//! 连接超时用 [`ConnectionManagerConfig`] 的默认值，
//! 不走配置栈：Redis 是**可丢弃**的派生存储，
//! 为它引入一个必须配准的调优项，只会让部署多一个失败点。

use std::time::Duration;

use nexus_config::ConfigStack;
use nexus_core::error::{AppError, ErrorCode};
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use secrecy::{ExposeSecret, SecretString};

/// Redis URL 的配置路径。
const PATH_URL: &str = "redis.url";

/// **单次**连接尝试的超时（秒）。
///
/// redis 的 `AsyncConnectionConfig` 默认是 1 秒
/// （实测 `redis-2.0.0-rc.2/src/client.rs:180`：
/// `DEFAULT_CONNECTION_TIMEOUT = Some(Duration::from_secs(1))`）。
/// 这里**显式**设上，而不是依赖上游的缺省 ——
/// 上游改默认值时，本仓库的行为不该跟着变。
const ATTEMPT_TIMEOUT_SECS: u64 = 1;

/// **整体**连接预算（秒）。
///
/// ⚠️ 这个值**必须存在**，而且必须作用在**我们这一侧**。
///
/// 实测（`redis-2.0.0-rc.2/src/aio/connection_manager.rs:481-489`）：
/// `ConnectionManager` 内部用 `ExponentialBuilder` 做重试，
/// 缺省 `number_of_retries = 6`、指数退避 ——
///
/// ```text
/// 单次尝试 1s × 6 次 + 退避 0.1+0.2+0.4+0.8+1.6+3.2 ≈ 6.3s
/// → 实测指向不可达地址时，`ConnectionManager::new` 耗时 30 秒
/// ```
///
/// 那 30 秒会让启动路径卡住 —— 对一个「纯派生 · 可丢弃」的依赖不可接受
/// （§1.3.1：Redis 不可用时系统必须照常工作）。
///
/// # 为什么不用 `set_number_of_retries` 来限流
///
/// `number_of_retries` 同时管着**断线后的自动重连**。
/// 把它调到 0 会让「Redis 短暂抖动后自动恢复」这个能力消失 ——
/// 那是 `ConnectionManager` 存在的理由。
///
/// 所以：**不动 redis 的重试策略**，而是用 `tokio::time::timeout`
/// 给**我们自己的急切构造调用**套一个总预算。
/// 超了就返回错误让调用方降级 —— 管理器反正会被丢弃。
///
/// **不**做成配置项：`nexus-config` 的映射清单里没有 `redis.connect_timeout_secs`，
/// 而本任务**不新增**配置项。Redis 是可丢弃的派生存储，
/// 为它开一个必须配准的旋钮只会让部署多一个失败点。
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 3;

/// Redis 客户端的配置。
///
/// **不**派生 `PartialEq` —— `SecretString` 没有实现它，
/// 而让 secret 参与相等性比较本身也是可疑的。
#[derive(Debug, Clone)]
pub struct RedisSettings {
    /// 连接 URL。**secret** —— 可能含口令。
    url: SecretString,
}

impl RedisSettings {
    /// 从配置栈读取。
    ///
    /// # 错误
    ///
    /// `redis.url` **缺失或为空**时返回 [`ErrorCode::InvalidParam`]。
    ///
    /// 缺 Redis **不是**致命错误（§1.3：Redis 可丢弃）——
    /// 但「配置里说要用 Redis 却没配地址」是**明确的配置错误**，
    /// 必须报出来，而不是悄悄降级 —— 那会让运维以为 Redis 在工作。
    pub fn from_config(config: &ConfigStack) -> Result<Self, AppError> {
        let url = config
            .get_str(PATH_URL)
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| {
                AppError::new(ErrorCode::InvalidParam, "缺少 Redis 连接配置")
                    .with_cause_msg(format!("配置路径 {PATH_URL} 缺失或为空"))
            })?;

        Ok(Self {
            url: SecretString::from(url),
        })
    }

    /// 连接 URL（**脱敏**）。
    ///
    /// 用于日志与错误信息。**永远不要**用这个值去建连接 ——
    /// 用 [`Self::expose_url`]。
    pub fn url_display(&self) -> String {
        redact_url(self.url.expose_secret())
    }

    /// 连接 URL（**明文**）。
    ///
    /// 刻意命名得啰嗦 —— 调用它意味着「我知道我在做什么」。
    pub fn expose_url(&self) -> &str {
        self.url.expose_secret()
    }
}

/// 把连接 URL 里的口令替换成 `***`。
///
/// 只做**保守**处理：把 `user:password@host` 形态的 `user:password` 部分替换掉。
/// 解析失败时返回**固定**的占位串 —— 宁可信息少，也不能泄露。
fn redact_url(url: &str) -> String {
    // 形如 scheme://user:password@host:port/path
    let Some(scheme_end) = url.find("://") else {
        return "***".to_string();
    };
    let rest = &url[scheme_end + 3..];
    let Some(at_pos) = rest.find('@') else {
        // 没有 userinfo —— URL 里没有口令，直接返回。
        return url.to_string();
    };
    let userinfo = &rest[..at_pos];
    let Some(colon_pos) = userinfo.find(':') else {
        // 只有用户名，没有口令。
        return url.to_string();
    };

    let mut out = String::with_capacity(url.len());
    out.push_str(&url[..scheme_end + 3]);
    out.push_str(&userinfo[..colon_pos]);
    out.push_str(":***@");
    out.push_str(&rest[at_pos + 1..]);
    out
}

/// Redis 客户端的句柄。
///
/// 包装 [`ConnectionManager`]，而不是直接导出它 ——
/// 这样「客户端从哪来」只有一个入口。
#[derive(Clone)]
pub struct RedisHandle {
    manager: ConnectionManager,
}

impl RedisHandle {
    /// 底层连接管理器。
    pub fn manager(&self) -> &ConnectionManager {
        &self.manager
    }

    /// 取出底层连接管理器（所有权）。
    pub fn into_manager(self) -> ConnectionManager {
        self.manager
    }
}

impl std::fmt::Debug for RedisHandle {
    /// **手写** `Debug` —— 只输出非敏感字段。
    ///
    /// `ConnectionManager` 派生 `Debug`（实测 `redis-2.0.0-rc.2/src/aio/connection_manager.rs:393`），
    /// 它内部持有 `Client` → `ConnectionInfo` → `RedisConnectionInfo`。
    ///
    /// redis 自己**已经**把手写的 `Debug` 做了脱敏
    /// （`src/connection.rs:470` 把 password 显示为 `<redacted>`），
    /// 但那依赖**上游版本的实现细节** —— 上游哪天改回派生就会开始泄露。
    /// 本仓库不把「安全」外包给依赖的某个版本行为。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisHandle").finish_non_exhaustive()
    }
}

/// 从配置栈构造 Redis 客户端并**建立连接**。
///
/// # 错误
///
/// - 配置缺失或非法 → [`ErrorCode::InvalidParam`]
/// - 连不上 Redis → [`ErrorCode::StorageUnavailable`]
///
/// **不 panic** —— Redis 不可用是**运行时故障**，不是编程错误。
///
/// ⚠️ **调用方必须处理这个错误而不是让它冒泡到启动路径** ——
/// §1.3.1：Redis 不可用时系统必须照常工作。正确做法是**记录告警并降级**
/// （退回 PG 轮询），而不是让整个进程起不来。
pub async fn connect(config: &ConfigStack) -> Result<RedisHandle, AppError> {
    let settings = RedisSettings::from_config(config)?;
    connect_with(settings).await
}

/// 用给定配置构造 Redis 客户端并建立连接。
pub async fn connect_with(settings: RedisSettings) -> Result<RedisHandle, AppError> {
    let client = redis::Client::open(settings.expose_url()).map_err(|e| {
        AppError::new(ErrorCode::InvalidParam, "Redis 连接配置非法")
            .with_cause(e)
            .with_cause_msg(format!("连接目标：{}", settings.url_display()))
    })?;

    // redis 2.0：`ConnectionManager::new` 是 **async**（1.x 是同步构造）。
    // 它内部会真的建一次连接，失败时立刻返回错误。
    //
    // ⚠️ 外面套一层 `tokio::time::timeout` ——
    // redis 内部会重试 6 次带指数退避，最坏要 30 秒（见
    // `DEFAULT_CONNECT_TIMEOUT_SECS` 的说明）。
    // 我们只给**急切构造**一个总预算；redis 自己的重连策略不动。
    let budget = Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS);
    let manager = tokio::time::timeout(
        budget,
        ConnectionManager::new_with_config(client, manager_config()),
    )
    .await
    .map_err(|_| {
        AppError::new(ErrorCode::StorageUnavailable, "Redis 暂不可用，请稍后再试").with_cause_msg(
            format!(
                "连接目标：{}（超过 {} 秒预算）",
                settings.url_display(),
                DEFAULT_CONNECT_TIMEOUT_SECS
            ),
        )
    })?
    .map_err(|e| {
        AppError::new(ErrorCode::StorageUnavailable, "Redis 暂不可用，请稍后再试")
            .with_cause(e)
            .with_cause_msg(format!("连接目标：{}", settings.url_display()))
    })?;

    Ok(RedisHandle { manager })
}

/// 构造 Redis 客户端，**不建立连接**（惰性）。
///
/// 连接在**首次使用**时建立。这让启动路径不必依赖 Redis 可达 ——
/// 与 §1.3.1 的「Redis 可丢弃」定位一致。
///
/// redis 2.0 **没有** `ConnectionManager::new_lazy` ——
/// 只有 `new_lazy_with_config(client, config)`
/// （实测 `redis-2.0.0-rc.2/src/aio/connection_manager.rs:471`）。
///
/// # 错误
///
/// 配置缺失或非法 → [`ErrorCode::InvalidParam`]。
///
/// # 前置条件
///
/// **必须在 Tokio 运行时上下文里调用** —— `new_lazy_with_config` 内部
/// 调 `Runtime::locate()` 并 spawn 后台重连任务，没有 runtime 会 panic。
pub fn connect_lazy(config: &ConfigStack) -> Result<RedisHandle, AppError> {
    let settings = RedisSettings::from_config(config)?;
    connect_lazy_with(settings)
}

/// 用给定配置构造 Redis 客户端，不建立连接（惰性）。
pub fn connect_lazy_with(settings: RedisSettings) -> Result<RedisHandle, AppError> {
    let client = redis::Client::open(settings.expose_url()).map_err(|e| {
        AppError::new(ErrorCode::InvalidParam, "Redis 连接配置非法")
            .with_cause(e)
            .with_cause_msg(format!("连接目标：{}", settings.url_display()))
    })?;

    // `new_lazy_with_config` 是**同步**的 —— 它不建连接，只起一个后台重连任务。
    let manager =
        ConnectionManager::new_lazy_with_config(client, manager_config()).map_err(|e| {
            AppError::new(ErrorCode::InvalidParam, "Redis 连接配置非法")
                .with_cause(e)
                .with_cause_msg(format!("连接目标：{}", settings.url_display()))
        })?;

    Ok(RedisHandle { manager })
}

/// 构造 `ConnectionManagerConfig`（**不建连**）。
///
/// 显式设上连接超时 —— 理由见 [`DEFAULT_CONNECT_TIMEOUT_SECS`]。
pub fn manager_config() -> ConnectionManagerConfig {
    ConnectionManagerConfig::new()
        .set_connection_timeout(Some(Duration::from_secs(ATTEMPT_TIMEOUT_SECS)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stack_with(redis: serde_json::Value) -> ConfigStack {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({ "redis": redis }));
        stack
    }

    fn full_stack() -> ConfigStack {
        stack_with(json!({
            "url": "redis://:supersecret@127.0.0.1:6379/0"
        }))
    }

    #[test]
    fn reads_settings_from_config() {
        let settings = RedisSettings::from_config(&full_stack()).unwrap();

        assert_eq!(
            settings.expose_url(),
            "redis://:supersecret@127.0.0.1:6379/0"
        );
    }

    #[test]
    fn missing_url_is_an_error() {
        let stack = stack_with(json!({}));
        let err = RedisSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn empty_url_is_an_error() {
        let stack = stack_with(json!({ "url": "   " }));
        let err = RedisSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn url_is_redacted_in_display() {
        let settings = RedisSettings::from_config(&full_stack()).unwrap();
        let shown = settings.url_display();

        assert!(
            !shown.contains("supersecret"),
            "口令绝不能出现在展示形式里：{shown}"
        );
    }

    #[test]
    fn debug_impl_does_not_leak_the_password() {
        let settings = RedisSettings::from_config(&full_stack()).unwrap();
        let debug = format!("{settings:?}");

        assert!(!debug.contains("supersecret"), "Debug 泄露了口令：{debug}");
    }

    #[test]
    fn manager_config_sets_a_bounded_attempt_timeout() {
        // 单次连接尝试必须有界 —— 否则一次挂起的 TCP 握手就能卡住整个构造。
        let config = manager_config();

        assert_eq!(
            config.connection_timeout(),
            Some(Duration::from_secs(ATTEMPT_TIMEOUT_SECS)),
            "单次连接超时必须有界"
        );
    }

    #[test]
    fn redaction_keeps_non_credential_urls_intact() {
        assert_eq!(
            redact_url("redis://127.0.0.1:6379/0"),
            "redis://127.0.0.1:6379/0"
        );
        assert_eq!(redact_url("not a url"), "***");
    }

    #[tokio::test]
    async fn connect_to_an_unreachable_redis_returns_error_not_panic() {
        // 端口 1 上不会有 Redis —— 连接必然失败。
        let stack = stack_with(json!({ "url": "redis://127.0.0.1:1/0" }));
        let err = connect(&stack).await.unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::StorageUnavailable);
        assert!(err.cause.is_some(), "内部原因应保留供日志使用");
    }

    #[tokio::test]
    async fn connect_lazy_does_not_panic_without_a_redis() {
        // 指向一个**不存在**的端口 —— 惰性构造不该去连它。
        let stack = stack_with(json!({ "url": "redis://127.0.0.1:1/0" }));
        let handle = connect_lazy(&stack);

        assert!(handle.is_ok(), "惰性构造不该失败");
    }

    #[tokio::test]
    async fn handle_debug_does_not_leak_the_password() {
        let settings = RedisSettings::from_config(&full_stack()).unwrap();
        let handle = connect_lazy_with(settings).unwrap();
        let debug = format!("{handle:?}");

        assert!(!debug.contains("supersecret"), "Debug 泄露了口令：{debug}");
    }

    #[tokio::test]
    async fn even_the_raw_upstream_manager_debug_does_not_leak_the_password() {
        // **纵深防御**的第二道防线。
        //
        // `RedisHandle` 的 `Debug` 是手写的，永远不打印任何东西 ——
        // 那意味着上面对它的断言**太容易通过**（哪怕实现有 bug 也可能碰巧不泄露）。
        //
        // 这一条断言的是**上游原始类型** `ConnectionManager` 的 `Debug`：
        // 调用方可以用 `into_manager()` 拿到它，然后 `{:?}` 打出来。
        // redis 自己手写了 `RedisConnectionInfo` 的 `Debug`
        // （实测 `redis-2.0.0-rc.2/src/connection.rs:455-475`，
        // 把 password 显示为 `<redacted>`）——
        // 本仓库**不依赖**这个行为来保证安全，
        // 但值得断言住：**一旦上游改成派生 `Debug`，这条测试会立刻红**，
        // 提醒我们「自己的手写 `Debug` 不是唯一防线了」。
        let settings = RedisSettings::from_config(&full_stack()).unwrap();
        let handle = connect_lazy_with(settings).unwrap();
        let raw = handle.into_manager();
        let debug = format!("{raw:?}");

        assert!(
            !debug.contains("supersecret"),
            "上游 ConnectionManager 的 Debug 泄露了口令：{debug}"
        );
    }
}
