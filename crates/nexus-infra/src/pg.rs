//! PG 连接池与配置装配。
//!
//! `nexus-infra` 负责 PG / Redis / S3 / Milvus 的**客户端构造**，不含任何业务语义
//! （架构文档 §4.3.3）。本模块是其中的 PG 部分。
//!
//! # 为什么配置来自 `nexus-config` 而不是自己读环境变量
//!
//! 环境的「谁定义变量名」必须**唯一**。`nexus-config` 已经维护了一份
//! `NX_` 环境变量到配置路径的映射清单（`DATABASE__URL` → `database.url`），
//! 那份清单是**运维手册的来源**（`nexus config env` 直接打印它）。
//! 如果 `nexus-infra` 自己读 `DATABASE_URL`，就会出现**两个**变量名约定 ——
//! 清单与实现脱节。
//!
//! # URL 是 secret
//!
//! `database.url` 在映射表里标了 `.secret()` —— 它含数据库口令。
//! 本模块用 [`secrecy::SecretString`] 承载它：
//!
//! - `Debug` 输出自动脱敏（`SecretBox` 的 `Debug` 实现不打印内容）
//! - 需要真正使用时显式调 [`ExposeSecret::expose_secret`]
//!
//! 这比「自己记得别打印」可靠 —— **类型上**就不容易泄露。

use std::time::Duration;

use nexus_config::ConfigStack;
use nexus_core::error::{AppError, ErrorCode};
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};

/// 连接 URL 的配置路径。
const PATH_URL: &str = "database.url";

/// 最大连接数的配置路径。
const PATH_MAX_CONNECTIONS: &str = "database.max_connections";

/// 连接超时（秒）的配置路径。
const PATH_CONNECT_TIMEOUT_SECS: &str = "database.connect_timeout_secs";

/// `max_connections` 的默认值。
///
/// 10 是 PostgreSQL 官方建议的「连接数 ≈ 核心数 × 2 + 有效磁盘数」
/// 在中小规模下的落点。
pub const DEFAULT_MAX_CONNECTIONS: u32 = 10;

/// 连接超时（秒）的默认值。
///
/// 5 秒足够覆盖同机房 TCP 握手 + 认证。**更长的超时会让启动卡住** ——
/// 数据库不可达时应该快速失败，而不是让进程卡在启动阶段。
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 5;

/// PG 连接池的配置。
///
/// 与 [`PgPoolOptions`] 分开，是因为**「我们想连什么」**（配置）
/// 与 **「sqlx 怎么连」**（选项）是两件事。分开后，
/// 配置可以被独立测试 —— 不需要数据库。
///
/// **不**派生 `PartialEq` —— `SecretString`（`SecretBox<str>`）没有实现它，
/// 而让 secret 参与相等性比较本身也是可疑的（比较可能泄漏长度信息）。
#[derive(Debug, Clone)]
pub struct PgSettings {
    /// 连接 URL。**secret** —— 含口令。
    url: SecretString,
    /// 连接池最大连接数。
    max_connections: u32,
    /// 连接超时。
    connect_timeout: Duration,
}

impl PgSettings {
    /// 从配置栈读取。
    ///
    /// # 错误
    ///
    /// `database.url` **缺失或为空**时返回 [`ErrorCode::InvalidParam`] ——
    /// 没有 URL 就没法连库，这是配置错误，不是运行时故障。
    ///
    /// `max_connections` / `connect_timeout_secs` **缺失时用默认值** ——
    /// 这两项有合理的缺省，不该让部署因为漏配一个调优项就起不来。
    ///
    /// 但如果配了**非法值**（比如 `max_connections` 是负数或 `0`），
    /// 那是**明确的配置错误**，必须报错 —— 不能悄悄退回默认值，
    /// 那会让运维以为配的值生效了。
    pub fn from_config(config: &ConfigStack) -> Result<Self, AppError> {
        let url = config
            .get_str(PATH_URL)
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| {
                AppError::new(ErrorCode::InvalidParam, "缺少数据库连接配置")
                    .with_cause_msg(format!("配置路径 {PATH_URL} 缺失或为空"))
            })?;

        let max_connections = match config.get_u64(PATH_MAX_CONNECTIONS) {
            Some(0) => {
                return Err(
                    AppError::new(ErrorCode::InvalidParam, "数据库连接池大小配置非法")
                        .with_cause_msg(format!("{PATH_MAX_CONNECTIONS} 必须大于 0")),
                );
            }
            Some(n) => u32::try_from(n).map_err(|_| {
                AppError::new(ErrorCode::InvalidParam, "数据库连接池大小配置非法")
                    .with_cause_msg(format!("{PATH_MAX_CONNECTIONS} 超出 u32 范围：{n}"))
            })?,
            None => DEFAULT_MAX_CONNECTIONS,
        };

        let connect_timeout = match config.get_u64(PATH_CONNECT_TIMEOUT_SECS) {
            Some(0) => {
                return Err(
                    AppError::new(ErrorCode::InvalidParam, "数据库连接超时配置非法")
                        .with_cause_msg(format!("{PATH_CONNECT_TIMEOUT_SECS} 必须大于 0")),
                );
            }
            Some(n) => Duration::from_secs(n),
            None => Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS),
        };

        Ok(Self {
            url: SecretString::from(url),
            max_connections,
            connect_timeout,
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
    ///
    /// 只在**真正要建连**的地方用。不要把返回值打日志或存起来。
    pub fn expose_url(&self) -> &str {
        self.url.expose_secret()
    }

    /// 连接池最大连接数。
    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }

    /// 连接超时。
    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
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

/// PG 连接池的句柄。
///
/// 包装 [`PgPool`]，而不是直接导出它 ——
/// 这样「连接池从哪来」只有一个入口，将来加健康检查、
/// 加指标都只需改这一处。
#[derive(Debug, Clone)]
pub struct PgHandle {
    pool: PgPool,
}

impl PgHandle {
    /// 底层连接池。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 取出底层连接池（所有权）。
    pub fn into_pool(self) -> PgPool {
        self.pool
    }
}

/// 从配置栈构造连接池并**建立连接**。
///
/// # 错误
///
/// - 配置缺失或非法 → [`ErrorCode::InvalidParam`]
/// - 连不上数据库 → [`ErrorCode::StorageUnavailable`]
///
/// **不 panic** —— 数据库不可达是**运行时故障**，不是编程错误。
/// 这一点很重要：进程启动时数据库还没起来（容器编排里很常见），
/// 应该让调用方决定是重试还是退出，而不是让整个进程崩掉。
pub async fn connect(config: &ConfigStack) -> Result<PgHandle, AppError> {
    let settings = PgSettings::from_config(config)?;
    connect_with(settings).await
}

/// 用给定配置构造连接池并建立连接。
pub async fn connect_with(settings: PgSettings) -> Result<PgHandle, AppError> {
    let options = pool_options(&settings);

    let pool = options.connect(settings.expose_url()).await.map_err(|e| {
        AppError::new(ErrorCode::StorageUnavailable, "数据库暂不可用，请稍后再试")
            .with_cause(e)
            .with_cause_msg(format!("连接目标：{}", settings.url_display()))
    })?;

    Ok(PgHandle { pool })
}

/// 构造连接池选项（**不建连**）。
///
/// 拆出来是为了能**离线**测试配置到选项的映射 ——
/// 不需要数据库就能断言「配置确实落到了选项上」。
pub fn pool_options(settings: &PgSettings) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(settings.max_connections())
        // sqlx 0.9 起，`PgConnectOptions` **不再有** `connect_timeout` 字段 ——
        // 连接建立超时统一由池的 `acquire_timeout` 兜底
        // （`sqlx-core-0.9.0/src/pool/inner.rs:252` 以 `acquire_timeout` 为截止时间）。
        // 旧版本 sqlx 里的 `PgConnectOptions::connect_timeout` 已移除。
        .acquire_timeout(settings.connect_timeout())
        .min_connections(0)
}

/// 构造连接选项（**不建连**）。
///
/// ⚠️ **返回的 `PgConnectOptions` 的 `Debug` 会打印明文口令** ——
/// 那是 sqlx 派生 `Debug` 的行为（实测确认）。
/// 优先用 [`safe_connect_options`]；只有确实需要裸类型时才用这个。
pub fn connect_options(settings: &PgSettings) -> Result<PgConnectOptions, AppError> {
    settings
        .expose_url()
        .parse::<PgConnectOptions>()
        .map_err(|e| {
            AppError::new(ErrorCode::InvalidParam, "数据库连接配置非法")
                .with_cause(e)
                .with_cause_msg(format!("连接目标：{}", settings.url_display()))
        })
}

/// **安全**的连接选项包装。
///
/// # 为什么需要它
///
/// `sqlx::postgres::PgConnectOptions` 的 `Debug` 是**派生**的，
/// 而它内部持有 `password: Option<String>` —— 于是：
///
/// ```text
/// PgConnectOptions { host: "127.0.0.1", ..., password: Some("supersecret"), ... }
/// ```
///
/// **明文口令直接出现在 `Debug` 输出里**（实测确认）。
/// 只要这个类型被 `{:?}` 打到日志里，口令就泄露了。
///
/// 本包装提供**手写**的 `Debug` —— 只输出非敏感字段，
/// 口令一律显示为 `[REDACTED]`。这样「不小心打印了连接选项」不再是事故。
///
/// # 为什么不用 `#[derive(Debug)]`
///
/// 派生会原样打印内部字段。**手写**才可能选择性地脱敏。
#[derive(Clone)]
pub struct SafeConnectOptions {
    inner: PgConnectOptions,
}

impl SafeConnectOptions {
    /// 取出内部的 sqlx 类型。
    ///
    /// 调用方拿到的 `PgConnectOptions` **仍然会泄露口令** ——
    /// 所以这个方法是 `pub(crate)` 级别谨慎使用的：
    /// 只在**真正要建连**的地方调用，不要把返回值存起来或打日志。
    pub fn into_inner(self) -> PgConnectOptions {
        self.inner
    }
}

impl std::fmt::Debug for SafeConnectOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SafeConnectOptions")
            .field("host", &self.inner.get_host())
            .field("port", &self.inner.get_port())
            .field("database", &self.inner.get_database())
            .field("password", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

/// 构造**安全**的连接选项（**不建连**）。
///
/// 返回 [`SafeConnectOptions`] 而不是裸的 `PgConnectOptions` ——
/// 这样调用方拿到的东西**在类型上**就不含「会被 `Debug` 泄露」的风险。
pub fn safe_connect_options(settings: &PgSettings) -> Result<SafeConnectOptions, AppError> {
    connect_options(settings).map(|inner| SafeConnectOptions { inner })
}

/// 从配置栈构造连接池，**不建立连接**（惰性）。
///
/// 连接在**首次使用**时建立。这让启动路径不必依赖数据库可达 ——
/// 但注意：**首次查询**仍会失败。是否用这个入口取决于调用方能否接受
/// 「启动成功但首次查询失败」。
///
/// # 前置条件
///
/// **必须在 Tokio 运行时上下文里调用** —— `connect_lazy_with` 会 spawn
/// 一个后台连接维护任务（`sqlx-core-0.9.0/src/pool/inner.rs:533`），
/// 没有 runtime 会 panic「this functionality requires a Tokio context」。
pub fn connect_lazy(config: &ConfigStack) -> Result<PgHandle, AppError> {
    let settings = PgSettings::from_config(config)?;
    let options = safe_connect_options(&settings)?;

    Ok(PgHandle {
        pool: pool_options(&settings).connect_lazy_with(options.into_inner()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_config::ConfigStack;
    use serde_json::json;

    fn stack_with(database: serde_json::Value) -> ConfigStack {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({ "database": database }));
        stack
    }

    fn full_stack() -> ConfigStack {
        stack_with(json!({
            "url": "postgres://nexus:secret@127.0.0.1:5432/nexus",
            "max_connections": 20,
            "connect_timeout_secs": 3
        }))
    }

    #[test]
    fn reads_all_settings_from_config() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();

        assert_eq!(settings.max_connections(), 20);
        assert_eq!(settings.connect_timeout(), Duration::from_secs(3));
        assert_eq!(
            settings.expose_url(),
            "postgres://nexus:secret@127.0.0.1:5432/nexus"
        );
    }

    #[test]
    fn missing_optional_settings_use_defaults() {
        let stack = stack_with(json!({ "url": "postgres://u:p@h:5432/d" }));
        let settings = PgSettings::from_config(&stack).unwrap();

        assert_eq!(settings.max_connections(), DEFAULT_MAX_CONNECTIONS);
        assert_eq!(
            settings.connect_timeout(),
            Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)
        );
    }

    #[test]
    fn missing_url_is_an_error() {
        let stack = stack_with(json!({}));
        let err = PgSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn empty_url_is_an_error() {
        let stack = stack_with(json!({ "url": "   " }));
        let err = PgSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn zero_max_connections_is_rejected_not_silently_defaulted() {
        // 配了 0 是**明确的错误** —— 悄悄退回默认值会让运维以为配的值生效了。
        let stack = stack_with(json!({ "url": "postgres://u:p@h/d", "max_connections": 0 }));
        let err = PgSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn zero_connect_timeout_is_rejected() {
        let stack = stack_with(json!({
            "url": "postgres://u:p@h/d",
            "connect_timeout_secs": 0
        }));
        let err = PgSettings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn url_is_redacted_in_display() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let shown = settings.url_display();

        assert!(
            !shown.contains("secret"),
            "口令绝不能出现在展示形式里：{shown}"
        );
        assert!(shown.contains("nexus"), "用户名应保留，便于定位：{shown}");
    }

    #[test]
    fn debug_impl_does_not_leak_the_password() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let debug = format!("{settings:?}");

        assert!(!debug.contains("secret"), "Debug 泄露了口令：{debug}");
    }

    #[test]
    fn redaction_keeps_non_credential_urls_intact() {
        assert_eq!(
            redact_url("postgres://127.0.0.1:5432/db"),
            "postgres://127.0.0.1:5432/db"
        );
        assert_eq!(
            redact_url("postgres://user@127.0.0.1:5432/db"),
            "postgres://user@127.0.0.1:5432/db"
        );
        assert_eq!(redact_url("not a url"), "***");
    }

    #[test]
    fn settings_land_on_pool_options() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let options = pool_options(&settings);

        assert_eq!(options.get_max_connections(), 20);
        assert_eq!(options.get_acquire_timeout(), Duration::from_secs(3));
    }

    #[test]
    fn connect_options_parse_a_valid_url() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let opts = connect_options(&settings).unwrap();

        assert_eq!(opts.get_host(), "127.0.0.1");
        assert_eq!(opts.get_port(), 5432);
    }

    #[test]
    fn connect_options_reject_a_malformed_url() {
        let stack = stack_with(json!({ "url": "not-a-postgres-url" }));
        let settings = PgSettings::from_config(&stack).unwrap();
        let err = connect_options(&settings).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn safe_connect_options_do_not_leak_the_password_in_debug() {
        // sqlx 的 `PgConnectOptions` 派生 `Debug`，会**明文**打印口令。
        // 本仓库的包装必须挡住这一点。
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let opts = safe_connect_options(&settings).unwrap();
        let debug = format!("{opts:?}");

        assert!(!debug.contains("secret"), "Debug 泄露了口令：{debug}");
        assert!(debug.contains("[REDACTED]"), "应显式标出已脱敏：{debug}");
        assert!(
            debug.contains("127.0.0.1"),
            "非敏感字段应保留，便于排查：{debug}"
        );
    }

    #[test]
    fn safe_connect_options_keep_the_usable_fields() {
        let settings = PgSettings::from_config(&full_stack()).unwrap();
        let opts = safe_connect_options(&settings).unwrap();
        let inner = opts.into_inner();

        assert_eq!(inner.get_host(), "127.0.0.1");
        assert_eq!(inner.get_port(), 5432);
        assert_eq!(inner.get_database(), Some("nexus"));
    }

    #[tokio::test]
    async fn connect_lazy_does_not_panic_without_a_database() {
        // 指向一个**不存在**的端口 —— 惰性构造不该去连它。
        //
        // ⚠️ 必须跑在 Tokio 上下文里：`connect_lazy_with` 会 spawn 一个
        // 后台维护任务（`sqlx-core-0.9.0/src/pool/inner.rs:533`），
        // 没有 runtime 会 panic「this functionality requires a Tokio context」。
        let stack = stack_with(json!({ "url": "postgres://u:p@127.0.0.1:1/nonexistent" }));
        let handle = connect_lazy(&stack);

        assert!(handle.is_ok(), "惰性构造不该失败");
    }

    #[tokio::test]
    async fn connect_to_an_unreachable_database_returns_error_not_panic() {
        // 端口 1 上不会有 PG —— 连接必然失败。
        let stack = stack_with(json!({
            "url": "postgres://u:p@127.0.0.1:1/nonexistent",
            "connect_timeout_secs": 1
        }));
        let err = connect(&stack).await.unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::StorageUnavailable);
        assert!(err.cause.is_some(), "内部原因应保留供日志使用");
    }
}
