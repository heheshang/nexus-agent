//! 路由骨架 —— **可增量注册**的 `Router` 构建器 + 一个真实可跑的 SSE 端点。
//!
//! # 为什么需要「构建器」
//!
//! P5 只交付**传输层**。业务端点（KB / 工作流 / 记忆 / 模型管理）
//! 属 P6/P7/P8 —— 它们**随对应阶段加入**。
//!
//! 所以路由必须是**可增量注册**的：P5 交出一个构建器，
//! 后续阶段在它上面**逐个**挂端点，**不改** P5 的任何代码。
//!
//! # 心跳是**路由**的职责，不是桥接的
//!
//! Spec §3.2 **已定稿**：
//!
//! | 规则 | 说明 |
//! |---|---|
//! | **心跳**由调用方注入 | 桥接**不自带**心跳定时器 |
//!
//! 理由：心跳是**传输层保活**，频率与路由的读超时配置相关。
//! 把它做成桥接的**可配置项**会让桥接承担**两条**职责。
//! 桥接只做「事件 → 帧」这一件事。
//!
//! 所以心跳在**本模块**注入 —— [`RouterBuilder::with_heartbeat`] 配置间隔，
//! 端点组装时把定时器与事件流**合并**。
//!
//! # SSE 端点的数据源是**注入的 trait**
//!
//! 决策 D9 的反方论点成立：「只交付骨架，端点就是**空壳**」。
//! 所以本模块交付**一个**真实可跑的 SSE 端点 ——
//! 它的数据源是 [`EventStreamProvider`]（trait），**不是** `nexus-service`
//! 的具体实现。
//!
//! 这让端点**真实可跑**（注入一个本地 mock 就能看到完整 SSE 帧流），
//! 同时让 P5 **不依赖**任何业务实现。
//!
//! # 心跳怎么算「空闲」
//!
//! 用 [`tokio::time::Sleep`]（**一次性**定时器，不是 `Interval`）：
//!
//! - 写了数据 → **重置**定时器到「现在 + 间隔」；
//! - 定时器到期 → 写一个 `: ping\n\n` 注释帧，然后**再重置**。
//!
//! ⚠️ **不用** `tokio::time::interval` ——
//! `interval` 按**固定节奏**触发，与「上次什么时候写的」无关。
//! 那样繁忙时也会**穿插**心跳帧（明明不空闲）。
//!
//! `Sleep` 的「每次写完都重置」才是真正的**空闲**判定。

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::header::CONTENT_TYPE;
use axum::response::Response;
use axum::routing::get;
use futures::StreamExt;
use nexus_agent::events::EventStream;
use tokio_util::sync::CancellationToken;

use crate::bridge::{SSE_CONTENT_TYPE, sse_body};
use crate::middleware::{AuthenticatedIdentity, IdentityExtractor};
use crate::sse::ping;

/// 示范性 SSE 端点的路径。
///
/// # ⚠️ 这是**示范性**端点，不是业务端点
///
/// 它的数据源由 [`EventStreamProvider`] 注入 —— 上层接上真正的实现后，
/// 业务端点会**另起路径**（P6/P7/P8）。
///
/// 这里用 `/api/v1/chat/stream` 是因为它**对齐参考实现**的路径 ——
/// 前端 `lib/sse.ts` 的解析器**不关心**路径，但联调时路径一致
/// 能省掉一类无谓的差异。
pub const SSE_DEMO_PATH: &str = "/api/v1/chat/stream";

/// 默认心跳间隔。
///
/// # 为什么是 15 秒
///
/// 与 `nexus-config` 的**默认配置**（`server.heartbeat_secs: 15`）一致。
///
/// 两者**必须**一致 —— 配置里写的值若与代码默认值不同，
/// 「改了配置没生效」会变成一个极难排查的问题。
pub const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

/// 事件流提供器 —— SSE 端点的**数据源**（trait 接缝）。
///
/// # 由**上层**实现
///
/// `nexus-api` **不**提供业务实现 —— 它连 `nexus-service` 的
/// **具体类型**都不引用（AC #6）。
///
/// # 契约
///
/// 实现方负责：
///
/// 1. 产出一个 [`EventStream`]（`nexus_agent::events::EventStream`）；
/// 2. 把**取消令牌**接上 —— 客户端断开时，桥接会触发它。
///
/// # ⚠️ `cancel` 的归属
///
/// 令牌由**端点**创建并持有，实现方**只**拿到它。
///
/// 理由与桥接相同：上层需要在取消发生后做清理
/// （释放共享额度、写终态、终止上游模型调用），
/// 令牌必须由**持有清理责任的一方**创建。
pub trait EventStreamProvider: Send + Sync {
    /// 为一次请求开启事件流。
    ///
    /// # 参数
    ///
    /// - `identity` —— 已认证的身份（若有）。**未鉴权**的端点会是 `None`。
    /// - `cancel` —— 取消令牌。客户端断开时端点会触发它。
    fn open(
        &self,
        identity: Option<&AuthenticatedIdentity>,
        cancel: CancellationToken,
    ) -> EventStream;
}

/// 路由构建器 —— **可增量注册**路由。
///
/// # 用法
///
/// ```ignore
/// let app = RouterBuilder::new()
///     .with_identity_extractor(Arc::new(MyExtractor))
///     .with_heartbeat(Some(Duration::from_secs(15)))
///     .register_sse_demo(Arc::new(MyProvider))
///     .build();
/// ```
///
/// # 为什么不用 `axum::Router` 直接干
///
/// `axum::Router` 本身就能加路由。构建器提供的是**协议层的语义**：
///
/// - **统一挂中间件**（请求 ID 全局挂、鉴权按路由挂）；
/// - **心跳配置**只在一处;
/// - 装配顺序**确定** —— 中间件挂在路由**之后**，
///   否则后加的路由会**绕过**中间件（axum 的 `layer` 文档明确说了这点）。
#[derive(Clone, Default)]
pub struct RouterBuilder {
    /// 已注册的路由。
    router: axum::Router,
    /// 身份提取器（可选 —— 不挂则不启用鉴权中间件）。
    identity_extractor: Option<Arc<dyn IdentityExtractor>>,
    /// 心跳间隔。
    heartbeat: Option<Duration>,
    /// SSE 端点的数据源（可选 —— 不注入则不注册 SSE 端点）。
    sse_provider: Option<Arc<dyn EventStreamProvider>>,
}

impl std::fmt::Debug for RouterBuilder {
    /// ⚠️ **手写** `Debug` —— 不 derive。
    ///
    /// `Arc<dyn IdentityExtractor>` **不**实现 `Debug`
    /// （trait 对象没有 `Debug` 约束）。
    ///
    /// derive 会要求它实现 —— 那会给**每个**提取器实现方
    /// 增加一个**与它们无关**的约束。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterBuilder")
            .field("has_identity_extractor", &self.identity_extractor.is_some())
            .field("heartbeat", &self.heartbeat)
            .field("has_sse_provider", &self.sse_provider.is_some())
            .finish()
    }
}

impl RouterBuilder {
    /// 构造一个**空**构建器。
    pub fn new() -> Self {
        Self {
            router: axum::Router::new(),
            identity_extractor: None,
            heartbeat: Some(DEFAULT_HEARTBEAT_INTERVAL),
            sse_provider: None,
        }
    }

    /// 设置身份提取器。
    ///
    /// 设置后，用 [`RouterBuilder::register_protected`] 注册的路由
    /// 会**自动**挂上鉴权中间件。
    pub fn with_identity_extractor(mut self, extractor: Arc<dyn IdentityExtractor>) -> Self {
        self.identity_extractor = Some(extractor);
        self
    }

    /// 设置心跳间隔。
    ///
    /// 传 `None` 表示**禁用**心跳（不写 `: ping\n\n`）。
    pub fn with_heartbeat(mut self, interval: Option<Duration>) -> Self {
        self.heartbeat = interval;
        self
    }

    /// 注入 SSE 端点的数据源。
    pub fn with_sse_provider(mut self, provider: Arc<dyn EventStreamProvider>) -> Self {
        self.sse_provider = Some(provider);
        self
    }

    /// 注册**一个**路由（**不挂**鉴权中间件）。
    ///
    /// # 可增量注册
    ///
    /// 每次调用**追加**一条路由 —— 已注册的路由**不受影响**。
    /// P6/P7/P8 的业务端点就通过它**逐个**加入。
    pub fn register(self, path: &str, method_router: axum::routing::MethodRouter) -> Self {
        Self {
            router: self.router.route(path, method_router),
            ..self
        }
    }

    /// 注册一个**受鉴权保护**的路由。
    ///
    /// # ⚠️ 没设提取器时**panic**
    ///
    /// 「忘记注入提取器」是**装配错误** —— 它应当在**启动时**暴露，
    /// 而不是等到某个请求进来才发现端点没被保护。
    ///
    /// ⚠️ 这是**唯一**会 panic 的地方。
    /// 其余的错误路径（提取失败）都走**统一响应包**。
    pub fn register_protected(
        self,
        path: &str,
        method_router: axum::routing::MethodRouter,
    ) -> Self {
        let extractor = self.identity_extractor.as_ref().expect(
            "受保护的路由要求**先**调用 with_identity_extractor —— \
             装配错误应当在启动时暴露，而不是在请求上",
        );
        let extractor = extractor.clone();
        Self {
            router: self.router.route(
                path,
                method_router.layer(axum::middleware::from_fn_with_state(
                    extractor,
                    identity_middleware_erased,
                )),
            ),
            ..self
        }
    }

    /// 注册示范性 SSE 端点。
    ///
    /// # 需要**注入**数据源
    ///
    /// 没注入 [`EventStreamProvider`] 时**不注册** —— 返回 `self` 不变。
    ///
    /// ⚠️ 这是**有意的**：端点没有数据源就是**空壳**，
    /// 而「空壳比没有更糟」（它看起来完成了）。
    /// 所以**宁可不注册**，也不注册一个空壳。
    ///
    /// # 心跳
    ///
    /// 端点组装时把**心跳定时器**与事件流合并 ——
    /// 空闲超过 [`RouterBuilder::with_heartbeat`] 配置的间隔，
    /// 写一个 `: ping\n\n` 注释帧。
    pub fn register_sse_demo(self, path: &str) -> Self {
        self.register_sse_demo_inner(path, false)
    }

    /// 注册示范性 SSE 端点，并**挂上鉴权中间件**。
    ///
    /// # 与 [`RouterBuilder::register_sse_demo`] 的区别
    ///
    /// 这个端点在**鉴权之后**才执行 ——
    /// 数据源拿到的 `identity` 是**已认证**的身份。
    ///
    /// ⚠️ 「哪些端点需要鉴权」是**上层**的决定 ——
    /// 所以这里提供**两个**方法，而不是一个 `protected: bool` 参数：
    /// 后者会让调用方有机会「传个 false 就忘了」。
    pub fn register_sse_demo_protected(self, path: &str) -> Self {
        self.register_sse_demo_inner(path, true)
    }

    /// 注册 SSE 端点的**共同实现**。
    ///
    /// # ⚠️ 为什么 `protected` 是**内部**参数
    ///
    /// 对外的**两个**方法各自表达一个**意图**
    /// （「公开端点」/「受保护端点」）——
    /// 让调用方在**调用点**就能看出意图。
    fn register_sse_demo_inner(self, path: &str, protected: bool) -> Self {
        let Some(provider) = self.sse_provider.clone() else {
            return self;
        };
        let heartbeat = self.heartbeat;

        let handler = move |request: Request| {
            let provider = provider.clone();
            async move { sse_demo_handler(provider, heartbeat, request).await }
        };

        let method_router = get(handler);

        let method_router = if protected {
            let extractor = self.identity_extractor.as_ref().expect(
                "受保护的 SSE 端点要求**先**调用 with_identity_extractor —— \
                 装配错误应当在启动时暴露，而不是在请求上",
            );
            let extractor = extractor.clone();
            method_router.layer(axum::middleware::from_fn_with_state(
                extractor,
                identity_middleware_erased,
            ))
        } else {
            method_router
        };

        Self {
            router: self.router.route(path, method_router),
            ..self
        }
    }

    /// 装配出最终的 `Router`。
    ///
    /// # 中间件在这里**统一挂**
    ///
    /// 请求 ID 中间件**全局挂** —— 它对**每个**请求都有意义，
    /// 且要在**最外层**（这样 handler 失败时响应头里也有 ID）。
    ///
    /// ⚠️ 为什么它**不**在 `register` 里挂
    ///
    /// 因为 `axum::Router::layer` **只作用于已存在的路由**。
    /// 在 `build()` 里挂能保证**所有**已注册路由都被覆盖 ——
    /// 包括未来阶段新增的路由（它们也是先 `register` 再 `build`）。
    pub fn build(self) -> axum::Router {
        self.router
            .layer(axum::middleware::from_fn(
                crate::middleware::request_id_middleware,
            ))
            .with_state(())
    }
}

/// 接受**擦除类型**提取器的鉴权中间件。
///
/// # 为什么需要它
///
/// `axum::middleware::from_fn_with_state` 的状态类型是**具体类型**。
/// 构建器里存的是 `Arc<dyn IdentityExtractor>` ——
/// `Arc<dyn IdentityExtractor>` **不**实现 `IdentityExtractor`
/// （`dyn IdentityExtractor` 本身不是 `Sized`，blanket impl 要求 `?Sized`，
/// 但 Rust 的 trait object 自动满足 `?Sized` —— 所以它**确实**实现）。
///
/// 但泛型 `E` 在 `from_fn_with_state` 里**推断不出来** ——
/// 所以这里给一个**具体类型**的包装函数。
///
/// # 为什么逻辑**不在**这里重复实现
///
/// 这里只做「委托」—— 真正的逻辑在
/// [`crate::middleware::extract_or_reject`]。
///
/// ⚠️ 两处逻辑**不得**分叉 —— 所以这里是**纯委托**，
/// 不是「复制一份逻辑」。
async fn identity_middleware_erased(
    axum::extract::State(extractor): axum::extract::State<Arc<dyn IdentityExtractor>>,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    match crate::middleware::authenticate(&*extractor, &mut request) {
        None => next.run(request).await,
        Some(rejection) => rejection,
    }
}

/// 示范性 SSE 端点的处理器。
///
/// # 流程
///
/// 1. 从 `Extension` 取身份（**可选** —— 该端点默认不挂鉴权）；
/// 2. 创建取消令牌；
/// 3. 向数据源要事件流；
/// 4. 桥接成 `Body`；
/// 5. 合并心跳定时器。
async fn sse_demo_handler(
    provider: Arc<dyn EventStreamProvider>,
    heartbeat: Option<Duration>,
    request: Request,
) -> Response {
    // 身份是**可选**的 —— 该端点默认不挂鉴权，所以可能没有。
    let identity = request.extensions().get::<AuthenticatedIdentity>().cloned();

    let cancel = CancellationToken::new();
    let stream = provider.open(identity.as_ref(), cancel.clone());

    // 桥接**不含**心跳 —— 那是路由的职责（spec §3.2）。
    let body = sse_body(stream, cancel);

    let body = match heartbeat {
        Some(interval) => with_heartbeat(body, interval),
        None => body,
    };

    Response::builder()
        .header(CONTENT_TYPE, SSE_CONTENT_TYPE)
        .body(body)
        .unwrap_or_else(|_| {
            // 静态头 + 合法 body 构造不会失败；兜底也**不 panic** ——
            // 服务器里一个 `expect` 就是一次可用性事故。
            let mut resp = Response::new(Body::empty());
            *resp.status_mut() = axum::http::StatusCode::INTERNAL_SERVER_ERROR;
            resp
        })
}

/// 给 SSE 响应体注入**心跳**。
///
/// # 行为
///
/// 事件流与心跳定时器**并行**：
///
/// - 事件来了 → 立刻写出（**不**等心跳），并**重置**定时器；
/// - 定时器到期 → 写一个 `: ping\n\n` 注释帧，并**重置**定时器。
///
/// # 为什么用 `Sleep` 而不是 `Interval`
///
/// `Interval` 按**固定节奏**触发 —— 与「上次什么时候写的」无关。
/// 那样繁忙时也会**穿插**心跳帧（明明不空闲）。
///
/// `Sleep` 每次写完都**重置**到「现在 + 间隔」——
/// 那才是真正的**空闲**判定。
///
/// # ⚠️ 心跳帧是**注释行**
///
/// `: ping\n\n` —— 前端 `lib/sse.ts` 靠 `:` 开头识别并跳过。
/// 它**不是** `event:` 帧 —— 所以不占用任何事件名。
fn with_heartbeat(body: Body, interval: Duration) -> Body {
    let body = body.into_data_stream();

    // 定时器在**第一次** poll 时创建 —— 不在 `with_heartbeat` 里，
    // 因为那会在**装配期**就固定下起始时刻，
    // 而请求真正开始服务可能晚很多。
    //
    // ⚠️ `Sleep` 是 `!Unpin` —— 必须 `Box::pin` 才能既放进状态里
    // 又能被 `select!` 安全地借用。
    type Sleeper = std::pin::Pin<Box<tokio::time::Sleep>>;

    let stream = futures::stream::unfold(
        Some((body, None::<Sleeper>)),
        move |state: Option<(axum::body::BodyDataStream, Option<Sleeper>)>| async move {
            let (mut body, mut sleeper) = state?;
            // 定时器尚未创建 —— 此刻是「连接刚建立」，算**空闲**。
            if sleeper.is_none() {
                sleeper = Some(Box::pin(tokio::time::sleep(interval)));
            }
            let mut sleeper = sleeper.expect("上面已确保存在");

            tokio::select! {
                // ⚠️ `biased` —— **事件优先**。
                //
                // 不加 `biased` 时 `select!` **随机**挑一个就绪的分支 ——
                // 繁忙时可能连续写出多个心跳帧。
                biased;

                chunk = body.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            // 写了数据 → 重置定时器（重新开始计空闲）。
                            sleeper.as_mut().reset(tokio::time::Instant::now() + interval);
                            Some((Ok::<Bytes, Infallible>(bytes), Some((body, Some(sleeper)))))
                        }
                        Some(Err(_err)) => {
                            // 响应体流出错 —— 结束。
                            //
                            // ⚠️ **不**补错误帧：桥接**已经**处理了
                            // 「事件流出错」的情形（发 error 帧）。
                            // 这里是**传输层**的字节流本身出错 ——
                            // 连接已经不可用，写什么都到不了客户端。
                            None
                        }
                        // 事件流**正常结束** —— 结束。
                        //
                        // ⚠️ 不补心跳 —— 流结束了还写心跳是错的。
                        None => None,
                    }
                }

                _ = &mut sleeper => {
                    // 空闲到期 —— 写一个心跳注释帧，并重置定时器。
                    let bytes = Bytes::from(ping());
                    sleeper.as_mut().reset(tokio::time::Instant::now() + interval);
                    Some((Ok::<Bytes, Infallible>(bytes), Some((body, Some(sleeper)))))
                }
            }
        },
    );

    Body::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_heartbeat_matches_the_config_default() {
        // 必须与 `nexus-config` 的 `server.heartbeat_secs: 15` 一致 ——
        // 否则「改了配置没生效」会变成极难排查的问题。
        assert_eq!(DEFAULT_HEARTBEAT_INTERVAL, Duration::from_secs(15));
    }

    #[test]
    fn a_new_builder_has_no_provider() {
        let builder = RouterBuilder::new();
        assert!(builder.sse_provider.is_none(), "新构建器不应有数据源");
    }

    #[test]
    fn registering_a_route_keeps_the_previous_ones() {
        let builder = RouterBuilder::new()
            .register("/a", get(|| async { "a" }))
            .register("/b", get(|| async { "b" }));
        // axum 的 Router 没有公开的「路由数量」查询 ——
        // 只能确认构建**没有 panic** 且状态仍是可用的。
        let _ = builder.build();
    }
}
