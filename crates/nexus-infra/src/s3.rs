//! S3 / MinIO 客户端构造。
//!
//! `nexus-infra` 负责 PG / Redis / S3 / Milvus 的**客户端构造**，不含任何业务语义
//! （架构文档 §4.3.3）。本模块是其中的 S3 部分。
//!
//! # 为什么用 `object_store` 而不是 `aws-sdk-s3`
//!
//! `object_store` 是**抽象层** —— 同一份代码能对接 AWS S3、本地 MinIO、
//! 以及未来的其他 S3 兼容后端。直接用 `aws-sdk-s3` 就把实现焊死在 AWS 上了。
//!
//! 参考实现用的是 MinIO（Go 的 `minio-go`）—— 同样是 S3 协议。
//! 架构文档 §1.3 的存储职责表把 S3 / MinIO 列为**纯派生 · 可重建**：
//!
//! | 存储 | 权威性 | 不可用时 |
//! |---|---|---|
//! | S3 / MinIO | 纯派生 · 可重建 | 上传/下载失败，其余可用 |
//!
//! # 凭据绝不外泄
//!
//! `s3.access_key` / `s3.secret_key` 在 `nexus-config` 的映射表里标了 `.secret()`。
//! 本模块用 [`secrecy::SecretString`] 承载它们，
//! 并**手写** [`S3Handle`] 的 `Debug` ——
//! 依赖 `object_store` 内部那层脱敏不保险（见 [`S3Settings`] 的说明）。
//!
//! # 配置来自 `nexus-config`
//!
//! 变量名由 `nexus-config` 的映射清单统一定义
//! （`S3__BUCKET` / `S3__ENDPOINT` / `S3__ACCESS_KEY` / `S3__SECRET_KEY`）。
//! 本模块**不自己读环境变量** —— 否则会出现两套变量名约定，清单与实现脱节。

use nexus_config::ConfigStack;
use nexus_core::error::{AppError, ErrorCode};
use object_store::aws::AmazonS3;
use object_store::aws::AmazonS3Builder;
use secrecy::{ExposeSecret, SecretString};

/// bucket 名的配置路径。
const PATH_BUCKET: &str = "s3.bucket";

/// endpoint 的配置路径。
const PATH_ENDPOINT: &str = "s3.endpoint";

/// access key 的配置路径。
const PATH_ACCESS_KEY: &str = "s3.access_key";

/// secret key 的配置路径。
const PATH_SECRET_KEY: &str = "s3.secret_key";

/// 缺省 region。
///
/// AWS S3 的签名需要 region。缺省用 `us-east-1` ——
///
/// 这与 `object_store` 内部的缺省一致（实测 `object_store-0.14.2/src/aws/builder.rs:1162`：
/// `self.region.unwrap_or_else(|| "us-east-1".to_string())`）。
/// 对本地 MinIO 而言 region 只是签名参数，不影响连通性。
///
/// **本模块不新增配置项** —— `nexus-config` 的映射清单里没有 `s3.region`，
/// 而 region 只是签名参数不值得为它开一个必须配准的旋钮。
pub const DEFAULT_REGION: &str = "us-east-1";

/// S3 客户端的配置。
#[derive(Debug, Clone)]
pub struct S3Settings {
    /// bucket 名。
    bucket: String,
    /// endpoint（可空 —— 走 AWS 官方端点时不需要）。
    endpoint: Option<String>,
    /// access key。**secret**。
    access_key: SecretString,
    /// secret key。**secret**。
    secret_key: SecretString,
    /// region。
    region: String,
}

impl S3Settings {
    /// 从配置栈读取。
    ///
    /// # 错误
    ///
    /// 以下任一缺失或为空 → [`ErrorCode::InvalidParam`]：
    ///
    /// - `s3.bucket` —— **必需**。`object_store` 的 `build()` 也会检查
    ///   （`Error::MissingBucketName`，实测 `src/aws/builder.rs:1160`），
    ///   但本模块**提前**报错，好过让调用方看到一个来自第三方 crate 的英文错误。
    /// - `s3.access_key` / `s3.secret_key` —— **必需**。
    ///   `object_store` 在只配了其中一个时会返回 `MissingAccessKeyId` /
    ///   `MissingSecretAccessKey`（`src/aws/builder.rs:1180-1186`）——
    ///   同样是提前报错更清晰。
    ///
    /// `s3.endpoint` **可空** —— 走 AWS 官方 S3 时不需要指定。
    pub fn from_config(config: &ConfigStack) -> Result<Self, AppError> {
        let bucket = required(config, PATH_BUCKET, "缺少对象存储 bucket 配置")?;
        let access_key = required(config, PATH_ACCESS_KEY, "缺少对象存储访问密钥配置")?;
        let secret_key = required(config, PATH_SECRET_KEY, "缺少对象存储密钥配置")?;

        // endpoint 可空 —— AWS 官方端点由 SDK 自己推导。
        let endpoint = config
            .get_str(PATH_ENDPOINT)
            .filter(|e| !e.trim().is_empty());

        // region 用缺省值 —— 见 `DEFAULT_REGION` 的说明。
        let region = DEFAULT_REGION.to_string();

        Ok(Self {
            bucket,
            endpoint,
            access_key: SecretString::from(access_key),
            secret_key: SecretString::from(secret_key),
            region,
        })
    }

    /// bucket 名。
    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// endpoint（`None` 表示走 AWS 官方端点）。
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// region。
    pub fn region(&self) -> &str {
        &self.region
    }

    /// access key（**明文**）。
    ///
    /// 刻意命名得啰嗦 —— 调用它意味着「我知道我在做什么」。
    pub fn expose_access_key(&self) -> &str {
        self.access_key.expose_secret()
    }

    /// secret key（**明文**）。
    ///
    /// 刻意命名得啰嗦 —— 调用它意味着「我知道我在做什么」。
    pub fn expose_secret_key(&self) -> &str {
        self.secret_key.expose_secret()
    }
}

/// 读一个**必需**的非空字符串配置项。
fn required(config: &ConfigStack, path: &str, message: &'static str) -> Result<String, AppError> {
    config
        .get_str(path)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::InvalidParam, message)
                .with_cause_msg(format!("配置路径 {path} 缺失或为空"))
        })
}

/// S3 客户端的句柄。
///
/// 包装 [`AmazonS3`]，而不是直接导出它 ——
/// 这样「客户端从哪来」只有一个入口。
#[derive(Clone)]
pub struct S3Handle {
    store: AmazonS3,
}

impl S3Handle {
    /// 底层对象存储客户端。
    pub fn store(&self) -> &AmazonS3 {
        &self.store
    }

    /// 取出底层对象存储客户端（所有权）。
    pub fn into_store(self) -> AmazonS3 {
        self.store
    }
}

impl std::fmt::Debug for S3Handle {
    /// **手写** `Debug` —— 只输出非敏感字段。
    ///
    /// `AmazonS3` 派生 `Debug`（实测 `object_store-0.14.2/src/aws/mod.rs:87`），
    /// 链路上最终会走到 `AwsCredential` 的 `Debug` ——
    /// 那份实现**确实**把 secret 显示成 `******`
    /// （`src/aws/credential.rs:83-92`）。
    ///
    /// 但那只是**当前版本**的行为。本仓库不把「安全」外包给依赖的某个版本实现 ——
    /// 一旦上游改成派生 `Debug`，泄露就是静默的。
    /// 手写一层，泄露就变成**不可能**而不是「大概率不会」。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Handle").finish_non_exhaustive()
    }
}

/// 从配置栈构造 S3 客户端。
///
/// # 错误
///
/// - 配置缺失或非法 → [`ErrorCode::InvalidParam`]
///
/// **不建连** —— `object_store` 的客户端构造是**纯本地**的（不发网络请求），
/// 真正的连通性在第一次上传/下载时才暴露。
/// 这让启动路径不被对象存储的可用性绑住 —— 与 §1.3 的「纯派生 · 可重建」一致。
pub fn connect(config: &ConfigStack) -> Result<S3Handle, AppError> {
    let settings = S3Settings::from_config(config)?;
    connect_with(settings)
}

/// 用给定配置构造 S3 客户端。
pub fn connect_with(settings: S3Settings) -> Result<S3Handle, AppError> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(settings.bucket())
        .with_region(settings.region())
        .with_access_key_id(settings.expose_access_key())
        .with_secret_access_key(settings.expose_secret_key());

    if let Some(endpoint) = settings.endpoint() {
        builder = builder.with_endpoint(endpoint);
        // `object_store` 默认 `allow_http = false`（实测
        // `object_store-0.14.2/src/client/mod.rs:973`：`.https_only(!self.allow_http.get()?)`），
        // 即**只允许 https**。MinIO 这类本地端点走 http，不开这个开关根本连不上。
        //
        // ⚠️ 这是一个**取舍**：允许 http 意味着凭据可能以明文过网。
        // 但对 MinIO 这类**内网**部署，这是必需的。
        //
        // 关键：**按 endpoint 的 scheme 决定**，而不是无条件打开 ——
        // 配了 https 端点时不该顺手把 http 也放行（那等于留了一条降级通道）。
        if endpoint.starts_with("http://") {
            builder = builder.with_allow_http(true);
        }
    }

    let store = builder.build().map_err(|e| {
        AppError::new(ErrorCode::InvalidParam, "对象存储配置非法")
            .with_cause(e)
            .with_cause_msg(format!("bucket：{}", settings.bucket()))
    })?;

    Ok(S3Handle { store })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stack_with(s3: serde_json::Value) -> ConfigStack {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({ "s3": s3 }));
        stack
    }

    fn full_stack() -> ConfigStack {
        stack_with(json!({
            "bucket": "nexus-docs",
            "endpoint": "http://127.0.0.1:9000",
            "access_key": "AKIAEXAMPLE",
            "secret_key": "supersecret"
        }))
    }

    #[test]
    fn reads_all_settings_from_config() {
        let settings = S3Settings::from_config(&full_stack()).unwrap();

        assert_eq!(settings.bucket(), "nexus-docs");
        assert_eq!(settings.endpoint(), Some("http://127.0.0.1:9000"));
        assert_eq!(settings.expose_access_key(), "AKIAEXAMPLE");
        assert_eq!(settings.expose_secret_key(), "supersecret");
    }

    #[test]
    fn endpoint_is_optional() {
        let stack = stack_with(json!({
            "bucket": "b",
            "access_key": "a",
            "secret_key": "s"
        }));
        let settings = S3Settings::from_config(&stack).unwrap();

        assert_eq!(settings.endpoint(), None);
    }

    #[test]
    fn region_uses_the_default() {
        let stack = stack_with(json!({
            "bucket": "b",
            "access_key": "a",
            "secret_key": "s"
        }));
        let settings = S3Settings::from_config(&stack).unwrap();

        assert_eq!(settings.region(), DEFAULT_REGION);
    }

    #[test]
    fn missing_bucket_is_an_error() {
        let stack = stack_with(json!({
            "access_key": "a",
            "secret_key": "s"
        }));
        let err = S3Settings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn empty_bucket_is_an_error() {
        let stack = stack_with(json!({
            "bucket": "   ",
            "access_key": "a",
            "secret_key": "s"
        }));
        let err = S3Settings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn missing_access_key_is_an_error() {
        let stack = stack_with(json!({
            "bucket": "b",
            "secret_key": "s"
        }));
        let err = S3Settings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn missing_secret_key_is_an_error() {
        let stack = stack_with(json!({
            "bucket": "b",
            "access_key": "a"
        }));
        let err = S3Settings::from_config(&stack).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::InvalidParam);
    }

    #[test]
    fn settings_debug_does_not_leak_the_secret_key() {
        let settings = S3Settings::from_config(&full_stack()).unwrap();
        let debug = format!("{settings:?}");

        assert!(!debug.contains("supersecret"), "Debug 泄露了密钥：{debug}");
    }

    #[tokio::test]
    async fn connect_builds_a_client_without_network_access() {
        // 客户端构造是**纯本地**的 —— 不需要真的 S3 存在。
        let handle = connect(&full_stack());

        assert!(handle.is_ok(), "构造不应失败：{handle:?}");
    }

    #[tokio::test]
    async fn handle_debug_does_not_leak_the_secret_key() {
        let handle = connect(&full_stack()).unwrap();
        let debug = format!("{handle:?}");

        assert!(!debug.contains("supersecret"), "Debug 泄露了密钥：{debug}");
        assert!(
            !debug.contains("AKIAEXAMPLE"),
            "Debug 泄露了 access key：{debug}"
        );
    }

    #[tokio::test]
    async fn even_the_raw_upstream_store_debug_does_not_leak_the_secret_key() {
        // **纵深防御**的第二道防线。
        //
        // `S3Handle` 的 `Debug` 是手写的，永远不打印任何东西 ——
        // 那意味着上面对它的断言**太容易通过**。
        //
        // 这一条断言的是**上游原始类型** `AmazonS3` 的 `Debug`：
        // 调用方可以用 `into_store()` 拿到它，然后 `{:?}` 打出来。
        // `AmazonS3` 派生 `Debug`，链路上走到 `AwsCredential` 的**手写** `Debug`
        // （实测 `object_store-0.14.2/src/aws/credential.rs:83-92`，secret 显示为 `******`）。
        //
        // 断言住它：**上游哪天改成派生 `Debug`，这条测试会立刻红。**
        let handle = connect(&full_stack()).unwrap();
        let raw = handle.into_store();
        let debug = format!("{raw:?}");

        assert!(
            !debug.contains("supersecret"),
            "上游 AmazonS3 的 Debug 泄露了密钥：{debug}"
        );
    }
}
