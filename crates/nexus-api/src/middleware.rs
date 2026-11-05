//! 协议层中间件 —— 鉴权上下文的**提取机制**与请求 ID 透传。
//!
//! # 决策 D10：机制在 P5，策略在上层
//!
//! 本模块**只**提供「怎么提取身份」的**机制** ——
//! 一个 [`IdentityExtractor`] trait 与一个把它接到 axum 上的中间件。
//!
//! **不**提供任何具体的鉴权策略（JWT 校验、配额判定、归属校验）：
//!
//! - `nexus-security` 是 7 行空壳 —— JWT 校验的具体形态**还没定**；
//! - P5 与 P8（`nexus-security` 落地）**没有顺序依赖** ——
//!   trait 让两者**并行**，而不是让 P5 **阻塞于**一个还没做的 crate。
//!
//! # 为什么「提取失败」要变成 `401` / `403` 而不是 `500`
//!
//! 提取器返回 [`AppError`] —— 它的**业务码**决定 HTTP 状态：
//!
//! | 业务码 | HTTP | 语义 |
//! |---|---|---|
//! | `20101` `Unauthorized` | 401 | 缺少身份凭证 |
//! | `20201` `Forbidden` | 403 | 身份无权访问该资源 |
//!
//! 两者都是**客户端**的问题 —— 客户端改一下就能重试。
//! 映射成 500 会让客户端以为「服务端坏了」而盲目重试。
//!
//! # 统一响应包
//!
//! 拒绝响应的 body 是**统一响应包**（[`ApiResponse`]）——
//! 与成功响应**形状一致**（`{code, message}`）。
//! 前端**一个分支**就能处理成功与失败。
//!
//! # 请求 ID 透传
//!
//! 每个请求分配（或透传）一个请求 ID：
//!
//! - 放进请求的 `Extension` —— handler 与日志可取用；
//! - 放进**响应头** `x-request-id` —— 客户端报障时能直接给出这个值。
//!
//! 透传优先于生成：上游（网关 / 前置代理）已经分配了一个 ID 时，
//! **沿用它** —— 否则同一次请求在链路上会有两个不同的 ID，
//! 跨系统排查就断了。

use std::sync::Arc;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use nexus_core::AppError;
use nexus_core::id::UserId;
use uuid::Uuid;

use crate::response::ApiResponse;

/// 请求 ID 的响应头名（也是请求头名 —— 透传用同一个）。
///
/// 小写 —— HTTP 头名**不区分大小写**，但统一用小写更便于检索。
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// 已认证的身份。
///
/// # 为什么是**协议层自己的类型**
///
/// 身份是「HTTP 语义」的一部分（架构文档 §1.4：
/// 协议层能感知已认证的身份，但**不能**感知业务规则）。
///
/// 它**不**携带任何业务对象 —— 不带 Agent、不带工作流、不带权限表。
/// 那些是**上层**的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedIdentity {
    /// 用户 ID。
    ///
    /// `None` 表示「已认证但无关联用户」—— 例如机器对机器的调用。
    pub user_id: Option<UserId>,
    /// 主体标识（租户 / 服务账号 / 用户名）。
    ///
    /// **原样**取自提取器的结果 —— P5 **不**解析它，
    /// 也**不**假定它的格式（不强制它是邮箱 / 是不是数字）。
    pub subject: String,
}

impl AuthenticatedIdentity {
    /// 构造一个身份。
    pub fn new(user_id: Option<UserId>, subject: impl Into<String>) -> Self {
        Self {
            user_id,
            subject: subject.into(),
        }
    }
}

/// 身份提取器 —— 鉴权**策略**的接缝。
///
/// # 由**上层**实现
///
/// `nexus-api` **不**提供任何具体实现 —— 特别是**不**实现 JWT 校验。
/// 实现由 `nexus-security` / `nexus-service` 提供（决策 D10）。
///
/// # 契约
///
/// | 情况 | 返回 |
/// |---|---|
/// | 凭证有效 | `Ok(身份)` |
/// | 缺少凭证 | `Err(AppError)`，业务码 `Unauthorized`（20101） |
/// | 凭证无效 / 过期 | `Err(AppError)`，业务码 `Unauthorized` 或等价于 401 的码 |
/// | 身份无权访问该资源 | `Err(AppError)`，业务码 `Forbidden`（20201） |
///
/// **业务码决定 HTTP 状态** —— 中间件不自己猜。
///
/// # ⚠️ 白名单边界
///
/// P5 的错误码白名单只含 `Unauthorized`（20101）与 `Forbidden`（20201）——
/// `nexus-api` **不引用**白名单外的码。
///
/// 但**实现方**（上层）可以用别的鉴权码 —— 它们同样映射到 401。
/// [`auth_rejection`] 按 **HTTP 状态**判定，不是按变体，
/// 所以上层的精确原因能**原样透出**给客户端。
pub trait IdentityExtractor: Send + Sync {
    /// 从请求头提取身份。
    ///
    /// # ⚠️ 实现方**必须**保证
    ///
    /// 1. **不把凭据写进错误消息** —— `cause` 只进日志（架构文档 §2.6.5）；
    /// 2. **失败即关闭**（fail closed）—— 无法确认身份时**必须**返回 `Err`，
    ///    绝不「猜一个身份放行」。
    fn extract(&self, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError>;
}

/// 让 `Arc<T>` 也能当提取器用。
///
/// # 为什么需要
///
/// 路由构建器存的是 `Arc<dyn IdentityExtractor>` ——
/// 它是**擦除类型**，不实现本 trait。
///
/// 但 `axum::middleware::from_fn_with_state` 的状态类型是**具体类型** ——
/// 有了这个 impl，中间件就能**直接**接受 `Arc<dyn IdentityExtractor>`，
/// **不需要**写第二个「擦除版」中间件。
///
/// # ⚠️ 为什么这样比「写两个中间件」好
///
/// 两个中间件意味着两处逻辑 —— 它们**迟早**会分叉
/// （改了一处忘了另一处）。
///
/// 一个中间件 + 一个 blanket impl：逻辑**只有一处**。
impl<T> IdentityExtractor for Arc<T>
where
    T: IdentityExtractor + ?Sized,
{
    fn extract(&self, headers: &HeaderMap) -> Result<AuthenticatedIdentity, AppError> {
        (**self).extract(headers)
    }
}

/// 鉴权中间件 —— 把身份提取器接到 axum。
///
/// # 行为
///
/// - 提取**成功** → 身份放进请求的 `Extension`（[`AuthenticatedIdentity`]），
///   供 handler 取用；请求**继续**往下走。
/// - 提取**失败** → 立即返回 `401` / `403` + **统一响应包**，
///   handler **不会**被执行。
///
/// # ⚠️ 失败即关闭
///
/// 提取器出错时**不**放行 —— 即使提取器自己「觉得应该继续」。
/// 鉴权中间件**没有**「失败但放行」这个选项。
///
/// # 挂在哪
///
/// **按路由挂**，不全局挂 —— 「哪些端点需要鉴权」是**上层**的决定
/// （P5 只交付机制）。用 [`axum::Router::route_layer`] 挂到指定路由上。
pub async fn identity_middleware<E>(
    State(extractor): State<E>,
    mut request: Request,
    next: Next,
) -> Response
where
    E: IdentityExtractor,
{
    match authenticate(&extractor, &mut request) {
        // 提取成功 → 继续往下走。
        None => next.run(request).await,
        // 提取失败 → 拒绝。
        Some(rejection) => rejection,
    }
}

/// 提取身份并放进请求的 `Extension`。
///
/// # 返回
///
/// - `None` —— 提取成功，身份已放进 `request` 的 `Extension`；
/// - `Some(响应)` —— 提取失败，响应已**按业务码**构造好，
///   调用方应当**直接返回**它。
///
/// # 为什么返回 `Option<Response>` 而不是 `Result<_, Response>`
///
/// `Result` 的 `Err` 变体是 `Response`（128 字节）——
/// clippy 的 `result_large_err` 会拒绝它（每个 `Err` 分支都拷贝一份）。
///
/// 用 `Option` 表达「有拒绝响应就返回它」是**同一个意思**，
/// 且没有那个开销。
///
/// # 为什么单独抽出来
///
/// 鉴权中间件有**两个**入口：
///
/// - [`identity_middleware`] —— 按路由挂（axum 中间件）；
/// - 路由构建器里那个**接受 `Arc<dyn>`** 的版本。
///
/// 如果两处各写一遍「提取 → 插入 `Extension` → 失败就拒绝」的逻辑，
/// 它们**迟早**会分叉 —— 改了一处忘了另一处。
///
/// 所以逻辑**只有这一处**，两个入口都调它。
pub fn authenticate<E>(extractor: &E, request: &mut Request) -> Option<Response>
where
    E: IdentityExtractor + ?Sized,
{
    match extractor.extract(request.headers()) {
        Ok(identity) => {
            request.extensions_mut().insert(identity);
            None
        }
        Err(err) => Some(auth_rejection(err)),
    }
}

/// 提取失败时的拒绝响应。
///
/// # 状态码由**业务码**决定
///
/// `AppError` 的业务码带 `http_status()` —— `Unauthorized` → 401，
/// `Forbidden` → 403。
///
/// ⚠️ **不**在这里硬编码「提取失败一定是 401」——
/// 「已认证但无权访问」是 **403**，两者语义不同。
///
/// # 为什么按 **HTTP 状态**而不是按**变体**判定
///
/// 鉴权段的业务码不止一个 —— 凭证过期、凭证无效、登录失败
/// **都是** 401。它们的具体区分是**上层**的事。
///
/// P5 的白名单只允许引用 `Unauthorized` 与 `Forbidden` ——
/// 若这里按变体逐个列举，就得引用白名单外的码（那是**契约越界**）。
///
/// 按 HTTP 状态判定则：
///
/// - 401 / 403 → 原样透出（**不**改业务码 —— 客户端拿到的仍是**精确**原因）；
/// - 其它状态 → 500（`Internal`）。
///
/// # ⚠️ 其它状态 → `500`
///
/// 提取器返回非鉴权段的码时，说明是**协议层自身**的内部问题 ——
/// 提取器不该返回那种码。
///
/// 映射成 500 比原样透出更诚实：
/// 原样透出会让「提取器内部崩了」看起来像「客户端没带凭证」，
/// 客户端会**盲目重试**。
pub fn auth_rejection(err: AppError) -> Response {
    let facing = err.user_facing();

    let status = match facing.code.http_status() {
        401 => StatusCode::UNAUTHORIZED,
        403 => StatusCode::FORBIDDEN,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };

    // ⚠️ 只序列化 `UserFacing` —— `AppError` 不实现 `Serialize`，
    // 所以「内部原因被发出去」在**类型上**就不可能。
    let body = ApiResponse::from_facing(facing).to_bytes();

    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

/// 请求 ID —— 放进请求 `Extension` 与响应头。
///
/// # 为什么是**字符串**
///
/// 请求 ID 是**技术标识**（不是实体 ID）—— 架构文档 §2.8.1 的
/// `TraceId` 那一档（`String` 底座）。
///
/// 但它**不是** `TraceId` —— `TraceId` 是**追踪** ID（跨系统追踪用），
/// 请求 ID 是**单跳**标识。两者生命周期不同，混用会让「一次请求」与
/// 「一条追踪」在日志里对不上。
///
/// 所以这里用**新类型**而不是裸 `String` ——
/// 裸 `String` 在 `Extension` 里会与其它字符串扩展撞类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    /// 构造。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 生成一个新的请求 ID（UUID v4）。
    fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// 取回底层值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 请求 ID 中间件 —— 分配或透传请求 ID。
///
/// # 行为
///
/// 1. 若请求头带 `x-request-id` 且**合法** → **透传**（沿用上游的值）；
/// 2. 否则 → 生成一个新的（UUID v4）；
/// 3. 放进请求的 `Extension`（[`RequestId`]）；
/// 4. 放进**响应头** `x-request-id`。
///
/// # ⚠️ 为什么透传的值要**校验**
///
/// 上游传来的头是**不可信输入** —— 它会被回显到响应头里。
///
/// 无校验就直接回显，等于把攻击者控制的内容**注入**响应头。
/// 所以只接受「短且只含安全字符」的值，其余**丢弃并重新生成**。
///
/// # 挂在哪
///
/// **全局挂** —— 请求 ID 对**每个**请求都有意义。
/// 用 [`axum::Router::layer`] 挂在最外层（这样 handler 失败时也有 ID）。
pub async fn request_id_middleware(mut request: Request, next: Next) -> Response {
    let request_id = match request.headers().get(REQUEST_ID_HEADER) {
        Some(value) => value
            .to_str()
            .ok()
            .filter(|raw| is_valid_request_id(raw))
            .map(RequestId::new)
            .unwrap_or_else(RequestId::generate),
        None => RequestId::generate(),
    };

    request.extensions_mut().insert(request_id.clone());

    let mut response = next.run(request).await;

    // 回显到响应头 —— 客户端报障时能直接给出这个值。
    // `RequestId` 的值是 UUID 或已校验的透传值 —— **一定**是合法头值。
    if let Ok(value) = HeaderValue::from_str(request_id.as_str()) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(REQUEST_ID_HEADER), value);
    }

    response
}

/// 透传的请求 ID 是否**合法**。
///
/// # 判据
///
/// - 长度在 `1..=128` —— 够放 UUID 与常见的链路 ID；
/// - 字符限于 ASCII 字母 / 数字 / `-` / `_` / `.` ——
///
///   ⚠️ 这条是**安全**判据，不是「格式偏好」：
///   请求头值会被**回显**到响应头。含 CR / LF 的值会造成
///   **响应拆分**（response splitting）。
fn is_valid_request_id(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 128
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// 从请求头提取请求 ID 的辅助函数。
///
/// # 为什么单独暴露
///
/// 中间件**只**把 ID 放进 `Extension`。
/// 但有些场景（如日志的**入口**埋点）需要在**进入中间件之前**就拿到
/// 上游透传的值 —— 那时 `Extension` 里还没有。
///
/// 这个函数给出**同一套**判据 —— 避免两处逻辑漂移。
pub fn request_id_from_headers(headers: &HeaderMap) -> Option<RequestId> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|raw| is_valid_request_id(raw))
        .map(RequestId::new)
}

/// 把 `RequestId` 从请求 `Extension` 里取出来。
///
/// # 为什么是 `Option`
///
/// 请求 ID **可能**不在 —— 例如该路由没挂请求 ID 中间件。
/// 那时返回 `None` 而不是 panic。
pub fn extract_request_id(parts: &mut axum::http::request::Parts) -> Option<RequestId> {
    parts.extensions.get::<RequestId>().cloned()
}

/// 便于在 handler 中**可选**地提取 `RequestId` 的提取器。
///
/// # 为什么需要它
///
/// `Extension<RequestId>` 已经能用。但 `Extension` 的**缺失**是
/// **rejection**（500）—— 对「可选」的请求 ID 来说太重。
///
/// 这个提取器**缺失时返回 `None`**，不报错。
///
/// # 用法
///
/// ```ignore
/// async fn handler(req_id: RequestIdExtractor) -> impl IntoResponse {
///     let req_id = req_id.0; // Option<RequestId>
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct RequestIdExtractor(pub Option<RequestId>);

impl<S> FromRequestParts<S> for RequestIdExtractor
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(extract_request_id(parts)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::ErrorCode;

    #[test]
    fn a_valid_request_id_is_accepted() {
        assert!(is_valid_request_id("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_request_id("req_123.abc"));
    }

    #[test]
    fn a_malformed_request_id_is_rejected() {
        // 空 —— 不能回显。
        assert!(!is_valid_request_id(""));
        // 超长 —— 不能回显。
        assert!(!is_valid_request_id(&"a".repeat(129)));
        // 含 CR / LF —— **响应拆分**风险。
        assert!(!is_valid_request_id("a\r\nX-Injected: 1"));
        assert!(!is_valid_request_id("a b"));
        assert!(!is_valid_request_id("a\nb"));
    }

    #[test]
    fn the_request_id_length_boundary_is_exact() {
        // 128 是**允许**的上界。
        assert!(is_valid_request_id(&"a".repeat(128)));
        assert!(!is_valid_request_id(&"a".repeat(129)));
    }

    #[test]
    fn the_generated_request_id_is_a_uuid() {
        let id = RequestId::generate();
        assert_eq!(id.as_str().len(), 36, "UUID v4 的文本长度应为 36");
        assert!(
            Uuid::parse_str(id.as_str()).is_ok(),
            "生成的请求 ID 应是合法 UUID：{}",
            id.as_str()
        );
    }

    #[test]
    fn an_auth_error_code_maps_to_its_own_status() {
        let resp = auth_rejection(AppError::new(ErrorCode::Unauthorized, "未认证"));
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = auth_rejection(AppError::new(ErrorCode::Forbidden, "无权访问"));
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn a_non_auth_error_code_maps_to_internal() {
        // 提取器返回非鉴权段的码 —— 那是**协议层自身**的问题。
        let resp = auth_rejection(AppError::new(ErrorCode::Internal, "boom"));
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
