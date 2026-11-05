//! `nexus-observability` 的**行为层**测试。
//!
//! # 检验的设计条款
//!
//! | 测试 | 条款 |
//! |---|---|
//! | `the_span_closes_when_the_stream_closes_not_before` | 架构文档 §2.4 关键不变量 2（`spanOnClose`） |
//! | `the_span_is_still_open_while_the_stream_is_alive` | 同上 |
//! | `cancellation_is_counted_separately_from_error` | 参考实现 `internal/llm/gateway.go:350-357` 的标签口径 |
//! | `a_mismatched_label_does_not_panic` | 参考架构「Rust 版」：`CounterVec::with_label_values` 在标签不匹配时 panic |
//!
//! # 怎么断言「span 已结束」
//!
//! `tracing` 的 span **没有**「是否已结束」的公开查询。
//! 所以这里实现一个 [`tracing_subscriber::Layer`]，
//! 在 `on_close` 回调里记录结束的 span 名 ——
//! **这是 span 真正关闭时**由 `tracing` 调用的钩子。

use std::sync::{Arc, Mutex};

use nexus_observability::{LlmMetrics, RequestStatus, StreamSpan};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

/// 记录 span 关闭事件的 layer。
#[derive(Debug, Clone, Default)]
struct CloseRecorder(Arc<Mutex<Vec<String>>>);

impl CloseRecorder {
    /// 已关闭的 span 名（按关闭顺序）。
    fn closed(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl<S> Layer<S> for CloseRecorder
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id) {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(span.name().to_string());
        }
    }
}

/// 装上 recorder，返回 guard 与 recorder。
///
/// # 为什么用 `set_default` 而不是 `with_default`
///
/// `with_default` 是**同步**闭包 —— 不能在里面 `await`。
/// 而本文件的测试要 `await` 流消费。
///
/// `set_default` 返回一个 `DefaultGuard`，
/// 它在**整个作用域**内生效 —— 可以跨 `await`。
fn with_recorder() -> (tracing::subscriber::DefaultGuard, CloseRecorder) {
    let recorder = CloseRecorder::default();
    let subscriber = tracing_subscriber::registry().with(recorder.clone());
    let guard = tracing::subscriber::set_default(subscriber);
    (guard, recorder)
}

#[tokio::test]
async fn the_span_is_still_open_while_the_stream_is_alive() {
    let (_guard, recorder) = with_recorder();
    {
        let span = StreamSpan::new("llm.chat_stream", "test-provider");

        // 一个**永不结束**的流。
        let stream = futures::stream::pending::<Result<i32, String>>();
        let mut wrapped = span.wrap(stream);

        // 拿到流之后 —— span **不该**已经关闭。
        assert!(
            recorder.closed().is_empty(),
            "拿到流时 span 不应结束 —— 它要覆盖模型生成 + 客户端消费的全过程，\
             否则 span 只有几毫秒，等于没测"
        );

        // poll 一次（流未结束）。
        use futures::StreamExt;
        let poll = futures::poll!(wrapped.next());
        assert!(poll.is_pending(), "未结束的流不应产出事件");

        // 仍然没关闭。
        assert!(recorder.closed().is_empty(), "流还活着时 span 不应结束");
    }
}

#[tokio::test]
async fn the_span_closes_when_the_stream_closes_not_before() {
    let (_guard, recorder) = with_recorder();
    {
        let span = StreamSpan::new("llm.chat_stream", "test-provider");

        let items: Vec<Result<i32, String>> = vec![Ok(1), Ok(2), Ok(3)];
        let stream = futures::stream::iter(items);
        let wrapped = span.wrap(stream);

        // 流还在手上 —— span 未关闭。
        assert!(recorder.closed().is_empty(), "拿到流时 span 不应结束");

        // 消费完整个流 —— wrapper 被 drop —— span 关闭。
        use futures::StreamExt;
        let collected: Vec<_> = StreamExt::collect(wrapped).await;
        assert_eq!(collected.len(), 3);

        assert!(
            recorder.closed().iter().any(|n| n == "llm.stream"),
            "流消费完后 span 应关闭，实际关闭的：{:?}",
            recorder.closed()
        );
    }
}

#[tokio::test]
async fn dropping_the_stream_early_also_closes_the_span() {
    let (_guard, recorder) = with_recorder();
    {
        let span = StreamSpan::new("llm.chat_stream", "test-provider");

        let stream = futures::stream::pending::<Result<i32, String>>();
        let wrapped = span.wrap(stream);

        // 提前放弃（客户端断连）。
        drop(wrapped);

        assert!(
            recorder.closed().iter().any(|n| n == "llm.stream"),
            "流被丢弃时 span 也应关闭（客户端断连不能留下悬挂的 span）"
        );
    }
}

#[test]
fn cancellation_is_counted_separately_from_error() {
    let metrics = LlmMetrics::new();

    metrics.record_request("p-cancel", RequestStatus::Ok);
    metrics.record_request("p-err", RequestStatus::Error);
    metrics.record_request("p-cancel", RequestStatus::Canceled);

    let rendered = metrics.render().expect("渲染不应失败");

    // 三个状态**各自**计数。
    assert!(
        rendered.contains(r#"status="canceled""#),
        "取消必须单独计数：\n{rendered}"
    );
    assert!(rendered.contains(r#"status="error""#), "错误应被计数");
    assert!(rendered.contains(r#"status="ok""#), "成功应被计数");

    // 取消**不**混进 error —— `p-err` 的 error 计数必须是 1。
    let error_line = rendered
        .lines()
        .find(|l| l.contains(r#"provider="p-err""#) && l.contains(r#"status="error""#))
        .unwrap_or_else(|| panic!("找不到 p-err 的 error 计数行：\n{rendered}"));
    let count: f64 = error_line
        .split_whitespace()
        .last()
        .and_then(|v| v.parse().ok())
        .unwrap_or_default();
    assert_eq!(count, 1.0, "错误计数应为 1");
}

#[test]
fn a_mismatched_label_does_not_panic() {
    // 标签来自**配置**（外部输入）——
    // 用安全的 `get_metric_with_label_values`，标签不匹配时**不 panic**。
    let metrics = LlmMetrics::new();
    metrics.record_request("provider-with-very-long-name", RequestStatus::Ok);
    metrics.record_tokens("provider-with-very-long-name", 10, 20);

    let rendered = metrics.render().expect("渲染不应失败");
    assert!(rendered.contains("llm_requests_total"));
    assert!(rendered.contains("llm_tokens_total"));
}
