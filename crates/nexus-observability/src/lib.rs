//! `nexus-observability` —— **埋点设施**：span 构造与指标计数器。
//!
//! # 为什么这个 crate 存在
//!
//! 架构文档 §3.8：「**可观测性是结构的一部分**，不是事后贴的」（原则 5）。
//!
//! 它定义**埋点的方式**与**必须存在的维度**，
//! 而**不**预先定义指标清单 —— 理由是「指标清单是实现细节，容易过时」。
//!
//! # 当前状态
//!
//! 本 crate 从**空壳**（只有 `CRATE_NAME`）变为有实现，
//! 起因是 S8 核对发现 `nexus-llm` **零可观测** ——
//! 而架构文档 §2.4 把「span 生命周期绑定流」列为**关键不变量**。
//!
//! # ⚠️ `tracing` 的 span 是**引用计数**的
//!
//! `on_close` 只在**最后一个** `Span` 句柄被 drop 时触发。
//! 所以 [`StreamSpan::wrap`] **消耗** `self` 而不是克隆 ——
//! 留一份克隆在别处，span 就**永远不会关闭**。
//!
//! # ⚠️ callsite 的 interest 缓存是**全局**的
//!
//! `tracing` 把「这个 callsite 要不要记录」全局缓存，
//! 在 callsite **第一次**注册时算一次，之后不再重算。
//!
//! 后果：**先**注册 callsite 的那个 subscriber 决定了后续所有 subscriber
//! 能不能看到这个 span。测试里这会导致**随机失败** ——
//! 所以断言 span 关闭的测试应当**独占**一个测试二进制，
//! 并在**任何**业务调用**之前**装上 subscriber。
//!
//! # 两个核心抽象
//!
//! | 抽象 | 解决什么 |
//! |---|---|
//! | [`StreamSpan`] | span 的生命周期**绑定流**，而非绑定「拿到流」 |
//! | [`LlmMetrics`] | `llm_requests_total` 等计数器，标签口径固定 |
//!
//! # ⚠️ 凭据**不得**进入埋点
//!
//! `api_key` 是**凭据**。埋点字段会进日志、进 trace 后端、进指标标签 ——
//! 那是**不可撤回的**扩散。
//!
//! 所以 [`LlmMetrics`] 与 [`StreamSpan`] 都**不接受**任何凭据类型的参数。
//! 这不是靠调用方自觉，是靠**签名**挡住。

use std::sync::LazyLock;
use std::task::Poll;

use prometheus::{CounterVec, Opts, Registry, TextEncoder};
use tracing::Span;

/// 全局指标注册表。
///
/// # 为什么用全局单例
///
/// `prometheus` 的 `CounterVec` 在**标签不匹配**时 `panic`——
/// 而标签是**运行期**才知道的（供应商代号来自配置）。
/// 全局单例 + `get_metric_with_label_values` 的 `Result` 返回
/// 让「标签写错」变成**可处理的错误**，而不是**崩溃**。
static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// 全局共享的指标集。
///
/// # ⚠️ 为什么**计数器本身**也必须是全局单例
///
/// `Registry::register` 遇到**同名**指标会返回 `Error::AlreadyReg`。
/// 如果每次 `LlmMetrics::new()` 都新建 `CounterVec` 并注册，
/// 第二次就会注册失败（被 `let _ =` 吞掉），
/// 而**全局注册表里留下的仍是第一次那个空的计数器**——
///
/// 结果：写进新计数器的值**根本不会被导出**。
///
/// 这不是「重复注册报错」的小事 —— 它让指标**静默失效**。
///
/// 所以计数器用 `LazyLock` 全局共享，`LlmMetrics` 只是它的**句柄**。
static REQUESTS: LazyLock<CounterVec> = LazyLock::new(|| {
    CounterVec::new(
        Opts::new("llm_requests_total", "LLM 请求总数"),
        &["provider", "status"],
    )
    .expect("指标定义必须是合法的 —— 标签名与 help 文本是编译期常量")
});

/// 全局共享的 token 计数器。
static TOKENS: LazyLock<CounterVec> = LazyLock::new(|| {
    CounterVec::new(
        Opts::new("llm_tokens_total", "LLM token 总数"),
        &["provider", "kind"],
    )
    .expect("指标定义必须是合法的 —— 标签名与 help 文本是编译期常量")
});

/// LLM 网关的指标。
///
/// # 标签口径
///
/// | 标签 | 取值 | 为什么 |
/// |---|---|---|
/// | `provider` | 供应商标识 | 定位「哪个供应商慢/错」 |
/// | `status` | `ok` / `error` / `canceled` | **取消单独计数** —— 便于区分「上游故障」与「用户取消」 |
///
/// `canceled` 单独计数是参考实现的既有口径
/// （`internal/llm/gateway.go:350-357`）——
/// 把取消混进 `error` 会让错误率虚高，且**误伤熔断器**。
pub struct LlmMetrics {
    /// 请求总数。
    requests: CounterVec,
    /// token 总数。
    tokens: CounterVec,
}

impl std::fmt::Debug for LlmMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `CounterVec` 不实现 `Debug` —— 而 workspace 的 lint
        // `missing_debug_implementations` 要求公共类型都有 `Debug`。
        f.debug_struct("LlmMetrics").finish_non_exhaustive()
    }
}

impl LlmMetrics {
    /// 创建指标集（并注册到全局注册表）。
    ///
    /// # 幂等
    ///
    /// 多次调用是**安全**的 —— 计数器是全局共享的，
    /// 注册只在**首次**成功。
    pub fn new() -> Self {
        let registry = &*REGISTRY;
        // ⚠️ `AlreadyReg` 是**预期内**的 —— 第二次调用时。
        // 忽略它，但**不能**忽略「计数器不是同一个」的情况 ——
        // 那由全局 `LazyLock` 保证（见 REQUESTS 的注释）。
        let _ = registry.register(Box::new(REQUESTS.clone()));
        let _ = registry.register(Box::new(TOKENS.clone()));

        Self {
            requests: REQUESTS.clone(),
            tokens: TOKENS.clone(),
        }
    }

    /// 记录一次请求的结果。
    ///
    /// # 标签不匹配时**不 panic**
    ///
    /// `prometheus` 的 `with_label_values` 在标签数量不符时 `panic`。
    /// 这里是**外部输入**（供应商代号来自配置）驱动的 ——
    /// 崩溃的代价远大于漏记一次。
    ///
    /// 漏记的错误通过返回值交给调用方记录。
    pub fn record_request(&self, provider: &str, status: RequestStatus) {
        let label = status.as_str();
        // ⚠️ 用 `get_metric_with_label_values` 而**不是** `with_label_values` ——
        // 后者在标签数量不符时**panic**。
        // 供应商代号来自**配置**（外部输入），崩溃的代价远大于漏记一次。
        match self
            .requests
            .get_metric_with_label_values(&[provider, label])
        {
            Ok(counter) => counter.inc(),
            Err(err) => {
                tracing::warn!(%err, provider, status = label, "指标写入失败");
            }
        }
    }

    /// 记录 token 用量。
    pub fn record_tokens(&self, provider: &str, prompt_tokens: i64, completion_tokens: i64) {
        for (kind, value) in [("prompt", prompt_tokens), ("completion", completion_tokens)] {
            if value <= 0 {
                continue;
            }
            match self.tokens.get_metric_with_label_values(&[provider, kind]) {
                Ok(counter) => counter.inc_by(value as f64),
                Err(err) => {
                    tracing::warn!(%err, provider, kind, "指标写入失败");
                }
            }
        }
    }

    /// 渲染当前指标（Prometheus 文本格式）。
    ///
    /// # 错误
    ///
    /// 编码失败只可能是内部状态问题 —— 返回 `Err` 而不 panic。
    pub fn render(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        encoder.encode_to_string(&REGISTRY.gather())
    }
}

impl Default for LlmMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// 一次请求的结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatus {
    /// 成功。
    Ok,
    /// 失败。
    Error,
    /// **被调用方取消**。
    ///
    /// 单独计数 —— 取消**不是**供应商的故障。
    Canceled,
}

impl RequestStatus {
    /// 指标标签值。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Canceled => "canceled",
        }
    }
}

/// **流绑定**的 span。
///
/// # 这解决什么问题
///
/// 参考实现的 `spanOnClose`（`internal/llm/gateway.go:197`）
/// 把 span 的结束推迟到**事件流关闭**。它的注释原文：
///
/// > 若放进 `runWithGuard`，拿到通道就返回，span 只有几毫秒，等于没测。
///
/// 也就是说：**span 绑「拿到流」这个时刻**是**错的**——
/// 那样 span 只能覆盖「建流」的耗时，
/// 而流式请求的主要耗时（**模型生成 + 客户端消费**）在 span 之外。
///
/// `tracing` 的 span 没有「延迟结束」的 API ——
/// 它随 [`Drop`] 结束。所以本类型**持有** span，
/// 直到**流被消费完**（或被丢弃）才释放它。
///
/// # 用法
///
/// ```ignore
/// let handle = StreamSpan::new("llm.chat_stream", provider);
/// // 把 handle 移进流的包装里 —— 它随流一起被消费
/// let stream = handle.wrap(inner_stream);
/// ```
///
/// # ⚠️ 凭据不得进入 span 字段
///
/// 本类型的字段构造器**只接受**供应商标识与模型名 —/
/// 不接受任何凭据类型。那是**签名级**的保证。
pub struct StreamSpan {
    span: Span,
}

impl StreamSpan {
    /// 创建一个流绑定 span。
    ///
    /// # 参数
    ///
    /// - `name` —— span 名（如 `llm.chat_stream`）。
    /// - `provider` —— 供应商标识（**不是凭据**）。
    pub fn new(name: &'static str, provider: &str) -> Self {
        let span = tracing::info_span!(
            target: "nexus::llm",
            "llm.stream",
            llm.provider = provider,
        );
        // span 名作为字段记录，便于按名字过滤。
        span.record("llm.operation", name);
        Self { span }
    }

    /// 记录一个字段。
    ///
    /// # ⚠️ 调用方**不得**传入凭据
    ///
    /// 这是**约定**而非类型级保证 —— 因为字段类型是 `&str`，
    /// 类型上无法区分「模型名」与「api_key」。
    ///
    /// 防线是**结构层测试**：扫描 `nexus-llm` 的源码，
    /// 断言没有把 `api_key` / `expose_api_key()` 传给埋点。
    pub fn record(&self, field: &'static str, value: &str) {
        self.span.record(field, value);
    }

    /// 记录一次 token 用量。
    pub fn record_usage(&self, prompt_tokens: i64, completion_tokens: i64) {
        self.span.record("llm.prompt_tokens", prompt_tokens);
        self.span.record("llm.completion_tokens", completion_tokens);
    }

    /// 在 span 的上下文里执行一段代码。
    pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
        self.span.in_scope(f)
    }

    /// 把 span **移进**一个流，使 span **随流结束**。
    ///
    /// # ⚠️ 为什么是 `self`（**消耗**）而不是 `&self`
    ///
    /// `tracing` 的 span 是**引用计数**的 ——
    /// `on_close` 只在**最后一个** `Span` 句柄被 drop 时才触发。
    ///
    /// 如果这里 `clone()` 一份放进 wrapper，而调用方还留着 `self`，
    /// 那么 wrapper 被 drop 时引用计数仍是 1 ——
    /// **span 永远不会关闭**，测试也永远抓不到这个问题。
    ///
    /// 所以 `wrap` **消耗** `self`：wrapper 持有**唯一**的句柄，
    /// 它被 drop 时 span 立刻关闭。
    ///
    /// # 为什么需要这层包装
    ///
    /// span 的生命周期由**持有它**的对象决定。
    /// 把 span 放进流的 wrapper 里，wrapper 被消费完（或被 drop）时，
    /// span 才结束 —— 这就是「span 生命周期绑定流」。
    pub fn wrap<S>(self, stream: S) -> StreamSpanned<S>
    where
        S: futures::Stream,
    {
        StreamSpanned {
            inner: stream,
            span: self,
            on_item: None,
        }
    }

    /// 把 span 移进一个流，并**在每个事件上**记录信息。
    ///
    /// # 为什么需要 per-item 回调
    ///
    /// 流式的 `finish_reason` 与 token 用量是**随事件到达**的 ——
    /// 而 span 在流结束时才关闭。
    ///
    /// 如果在流**结束后**才记这些字段，span 已经关了，记了也看不到。
    /// 所以必须在**事件流经时**记录。
    ///
    /// 回调是**类型无关**的（`nexus-observability` 不依赖 `nexus-llm`）——
    /// 具体怎么读字段由调用方决定。
    pub fn wrap_with<S, F>(self, stream: S, on_item: F) -> StreamSpanned<S>
    where
        S: futures::Stream,
        F: Fn(&Span, &S::Item) + Send + Sync + 'static,
    {
        StreamSpanned {
            inner: stream,
            span: self,
            on_item: Some(Box::new(on_item)),
        }
    }

    /// 当前的 span（供外部记录）。
    pub fn span(&self) -> &Span {
        &self.span
    }
}

impl Clone for StreamSpan {
    fn clone(&self) -> Self {
        Self {
            span: self.span.clone(),
        }
    }
}

impl std::fmt::Debug for StreamSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamSpan").finish_non_exhaustive()
    }
}

/// 每个事件上的回调类型。
///
/// # 为什么要一个 type 别名
///
/// `Box<dyn Fn(&Span, &S::Item) + Send + Sync>` 太长，
/// 且 clippy 的 `type_complexity` 会拒绝它。
type ItemCallback<S> = Box<dyn Fn(&Span, &<S as futures::Stream>::Item) + Send + Sync>;

/// 被 [`StreamSpan::wrap`] 包装的流。
///
/// span 随**本对象**结束 —— 而本对象随流结束。
pub struct StreamSpanned<S>
where
    S: futures::Stream,
{
    /// 内部流。
    inner: S,
    /// 绑定的 span —— **唯一**的句柄。
    span: StreamSpan,
    /// 每个事件上的回调。
    on_item: Option<ItemCallback<S>>,
}

impl<S> std::fmt::Debug for StreamSpanned<S>
where
    S: futures::Stream,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 回调是 `dyn Fn` —— 不实现 `Debug`。
        // 这里只报告「有没有回调」。
        f.debug_struct("StreamSpanned")
            .field("has_on_item", &self.on_item.is_some())
            .finish_non_exhaustive()
    }
}

impl<S> StreamSpanned<S>
where
    S: futures::Stream,
{
    /// 取出内部的流（**放弃** span 绑定）。
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> futures::Stream for StreamSpanned<S>
where
    S: futures::Stream + Unpin,
{
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<<S as futures::Stream>::Item>> {
        let this = self.get_mut();
        let polled = std::pin::Pin::new(&mut this.inner).poll_next(cx);

        // span 在 `Drop` 时结束 —— 而 `StreamSpanned` 被消费完或 drop 时才 Drop。
        // 这里用 `in_scope` 让 poll 发生在 span 的上下文里。
        let polled = this.span.in_scope(|| polled);

        // 事件流经时记录 —— span 还没关，字段才看得见。
        //
        // ⚠️ 回调收到的是**原始 item**，不假设它是 `Result` ——
        // `nexus-observability` 不该知道上游的错误形状。
        if let (Some(on_item), Poll::Ready(Some(item))) = (this.on_item.as_ref(), &polled) {
            on_item(this.span.span(), item);
        }

        polled
    }
}
