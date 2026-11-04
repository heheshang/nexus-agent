//! demo-stream-fallback
//!
//! 验证设计文档 §3.3：**流式响应一旦开始向客户端发送，就不能回退到另一个模型**。
//!
//! 原因：客户端已经收到了一半内容，切换模型会产生内容拼接错乱。
//!
//! ## Rust 特有的陷阱
//!
//! `Stream` 是**惰性**的 —— 「已经发给客户端了」不是「调用了 `send`」，
//! 而是「下游 poll 到了这个 item」。
//!
//! 所以锁定点必须在**实际 yield 之前**：
//!
//! ```ignore
//! if let Some(chunk) = chunk {
//!     session.lock_provider(provider_id);   // ← 在 yield 之前
//!     yield Ok(chunk);
//! }
//! ```
//!
//! 这个时序点极易搞错 —— 编译器帮不上忙，只能靠测试兜底。

use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// 供应商标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderId {
    Primary,
    Fallback,
}

/// 回退错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FallbackError {
    #[error("流已开始发送，不能回退到其他供应商")]
    AlreadyStarted,
}

/// 流式会话。
///
/// 状态机保证「一旦发出首字节，就锁定 provider」。
///
/// **两个概念必须分开**：
/// - `provider` —— 当前选中的供应商（**开始前**可以改）
/// - `started` —— 是否已经发出过字节（一旦为 true 就**不可逆**）
///
/// 早期版本用 `locked_provider: Option<ProviderId>` 同时表达两者，
/// 导致 `fallback()` 之后 `started()` 竟变成 true —— 自相矛盾。
#[derive(Debug, Default)]
pub struct StreamSession {
    provider: Option<ProviderId>,
    started: bool,
}

impl StreamSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// 标记「已经开始发送」。
    ///
    /// **必须在 yield 之前调用** —— 见 `SessionStream::poll_next`。
    pub fn mark_started(&mut self) {
        self.started = true;
    }

    /// 选定供应商。
    ///
    /// **只在尚未选定时生效** —— 已经选过的（无论是 `fallback()` 选的
    /// 还是流开始时选的）不会被覆盖。
    /// 流开始后调用则完全无效。
    pub fn select_provider(&mut self, provider: ProviderId) {
        if !self.started && self.provider.is_none() {
            self.provider = Some(provider);
        }
    }

    /// 当前选中的供应商。
    pub fn provider(&self) -> Option<ProviderId> {
        self.provider
    }

    /// 是否已开始发送。
    pub fn started(&self) -> bool {
        self.started
    }

    /// 尝试回退到备用供应商。
    ///
    /// 流已开始则拒绝。
    pub fn fallback(&mut self, to: ProviderId) -> Result<(), FallbackError> {
        if self.started {
            return Err(FallbackError::AlreadyStarted);
        }
        self.provider = Some(to);
        Ok(())
    }
}

/// 一个模型输出的流片段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk(pub String);

/// 带会话状态的流。
///
/// 关键：锁定发生在 `poll_next` 内部 —— 也就是**下游真正收到这个 chunk 时**。
pub struct SessionStream {
    session: StreamSession,
    chunks: Vec<Chunk>,
    index: usize,
    provider: ProviderId,
}

impl SessionStream {
    pub fn new(provider: ProviderId, chunks: Vec<Chunk>) -> Self {
        Self {
            session: StreamSession::new(),
            chunks,
            index: 0,
            provider,
        }
    }

    pub fn session(&self) -> &StreamSession {
        &self.session
    }

    /// 可变访问 —— 用于在消费前决定是否回退。
    pub fn session_mut(&mut self) -> &mut StreamSession {
        &mut self.session
    }
}

impl Stream for SessionStream {
    type Item = Chunk;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.index >= this.chunks.len() {
            return Poll::Ready(None);
        }
        let chunk = this.chunks[this.index].clone();
        this.index += 1;

        // ★ 锁定发生在 yield 之前 —— 下游真正收到这个 chunk 的时刻
        this.session.select_provider(this.provider);
        this.session.mark_started();

        Poll::Ready(Some(chunk))
    }
}

#[tokio::main]
async fn main() {
    // ── 1. 流开始前可以回退 ──
    let mut session = StreamSession::new();
    println!("--- 流开始前 ---");
    println!("  已开始 = {}", session.started());
    session
        .fallback(ProviderId::Fallback)
        .expect("未开始时应允许回退");
    println!("  回退后供应商 = {:?}", session.provider());
    assert!(!session.started(), "回退不等于开始 —— 这两个概念必须分开");

    // ── 2. 流开始后拒绝回退 ──
    let mut session = StreamSession::new();
    session.select_provider(ProviderId::Primary);
    session.mark_started();
    println!("\n--- 流开始后 ---");
    println!("  已开始 = {}", session.started());
    let err = session.fallback(ProviderId::Fallback).unwrap_err();
    println!("  回退被拒绝：{err}");
    assert_eq!(err, FallbackError::AlreadyStarted);
    println!("  供应商仍为 = {:?}", session.provider());

    // ── 3. 真实流：锁定发生在下游 poll 的时刻 ──
    println!("\n--- 真实流 ---");
    let mut stream = SessionStream::new(
        ProviderId::Primary,
        vec![Chunk("第一段".into()), Chunk("第二段".into())],
    );

    // 在消费之前：未开始，允许回退
    assert!(!stream.session().started());
    stream
        .session_mut()
        .fallback(ProviderId::Fallback)
        .expect("应允许");
    println!(
        "  消费前：已开始 = {}，供应商 = {:?}",
        stream.session().started(),
        stream.session().provider()
    );

    // 消费第一个 chunk —— 锁定生效
    let first = futures::StreamExt::next(&mut stream).await;
    println!("  收到第一个 chunk = {first:?}");
    println!("  消费后：已开始 = {}", stream.session().started());
    println!("  供应商 = {:?}", stream.session().provider());
    assert!(stream.session().started());
    assert_eq!(stream.session().provider(), Some(ProviderId::Fallback));
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn fallback_allowed_before_stream_starts() {
        let mut s = StreamSession::new();
        assert!(s.fallback(ProviderId::Fallback).is_ok());
        assert_eq!(s.provider(), Some(ProviderId::Fallback));
        // 回退**不等于**开始
        assert!(!s.started());
    }

    #[tokio::test]
    async fn fallback_rejected_after_stream_starts() {
        let mut s = StreamSession::new();
        s.select_provider(ProviderId::Primary);
        s.mark_started();
        assert_eq!(
            s.fallback(ProviderId::Fallback),
            Err(FallbackError::AlreadyStarted)
        );
        // 供应商不变
        assert_eq!(s.provider(), Some(ProviderId::Primary));
    }

    #[tokio::test]
    async fn lock_happens_at_downstream_poll() {
        let mut stream = SessionStream::new(ProviderId::Primary, vec![Chunk("a".into())]);
        // 还没消费 —— 未开始
        assert!(!stream.session().started());
        // 消费 —— 锁定
        let _ = stream.next().await;
        assert!(stream.session().started());
        assert_eq!(stream.session().provider(), Some(ProviderId::Primary));
    }

    #[tokio::test]
    async fn empty_stream_does_not_lock() {
        let mut stream = SessionStream::new(ProviderId::Primary, vec![]);
        assert!(stream.next().await.is_none());
        assert!(!stream.session().started());
    }

    #[tokio::test]
    async fn select_provider_after_stream_start_is_invalid() {
        let mut s = StreamSession::new();
        s.select_provider(ProviderId::Primary);
        s.mark_started();
        s.select_provider(ProviderId::Fallback);
        assert_eq!(s.provider(), Some(ProviderId::Primary));
    }

    #[tokio::test]
    async fn selected_provider_is_not_overwritten() {
        let mut s = StreamSession::new();
        s.fallback(ProviderId::Fallback).unwrap();
        // 流开始时的 Primary 不应覆盖已选的 Fallback
        s.select_provider(ProviderId::Primary);
        assert_eq!(s.provider(), Some(ProviderId::Fallback));
    }
}
