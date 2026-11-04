//! demo-backpressure
//!
//! 验证设计文档 §3.2：用**两个不同类型的通道**区分「可丢」与「必达」。
//!
//! - `LossySender`：满时丢弃并**显式返回** `Outcome::Discarded`，**不阻塞**
//! - `GuaranteedSender`：挂起等待槽位，**保证送达**
//!
//! 关键点：这不是「一个通道 + 留一个槽位」的运行时约定，而是**类型上的区分**。

use std::time::Duration;

use tokio::sync::mpsc;

/// 投递结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// 被丢弃 —— 调用方可以计数上报。
    Discarded,
}

/// 可丢通道。
///
/// 满时丢弃，**永不阻塞**。用于高频增量事件。
#[derive(Debug)]
pub struct LossySender<T> {
    tx: mpsc::Sender<T>,
}

impl<T> LossySender<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            tx: mpsc::channel(capacity).0,
        }
    }

    /// 非阻塞投递。满则丢弃。
    pub fn emit(&self, item: T) -> Outcome {
        match self.tx.try_send(item) {
            Ok(()) => Outcome::Delivered,
            Err(_) => Outcome::Discarded,
        }
    }
}

/// 必达通道。
///
/// 挂起等待槽位，**保证送达**。用于终态事件。
#[derive(Debug, Clone)]
pub struct GuaranteedSender<T> {
    tx: mpsc::Sender<T>,
}

impl<T> GuaranteedSender<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            tx: mpsc::channel(capacity).0,
        }
    }

    /// 保证送达。会等待槽位。
    pub async fn emit(&self, item: T) -> Result<(), mpsc::error::SendError<T>> {
        self.tx.send(item).await
    }
}

#[tokio::main]
async fn main() {
    // ── 可丢通道 ──
    let (ltx, mut lrx) = mpsc::channel::<u32>(2);
    let lossy = LossySender { tx: ltx };
    println!("--- 可丢通道（容量 2）---");
    let mut discarded = 0;
    for i in 1..=4 {
        let o = lossy.emit(i);
        if o == Outcome::Discarded {
            discarded += 1;
        }
        println!("  发送 {i} → {o:?}");
    }
    let mut got = Vec::new();
    while let Ok(v) = lrx.try_recv() {
        got.push(v);
    }
    println!("  实际收到 = {got:?}，丢弃 {discarded} 条");
    assert_eq!(got, vec![1, 2]);
    assert_eq!(discarded, 2);

    // ── 必达通道 ──
    let (gtx, mut grx) = mpsc::channel::<u32>(2);
    let guaranteed = GuaranteedSender { tx: gtx };
    println!("\n--- 必达通道（容量 2，先填满）---");
    guaranteed.emit(1).await.unwrap();
    guaranteed.emit(2).await.unwrap();

    let sender = tokio::spawn(async move {
        guaranteed.emit(3).await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    println!("  第三个仍在等待（未被丢弃）= {}", !sender.is_finished());

    let first = grx.recv().await.unwrap();
    println!("  消费掉 {first} 后，第三个完成 = {}", sender.is_finished());
    sender.await.unwrap();

    let mut got2 = Vec::new();
    while let Ok(v) = grx.try_recv() {
        got2.push(v);
    }
    println!("  全部收到 = {got2:?}");
    assert_eq!(got2, vec![2, 3]);

    println!("\n✓ 两个通道的语义差异得到实证");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn 可丢通道满时丢弃且不阻塞() {
        let (tx, mut rx) = mpsc::channel::<u32>(2);
        let s = LossySender { tx };

        assert_eq!(s.emit(1), Outcome::Delivered);
        assert_eq!(s.emit(2), Outcome::Delivered);
        assert_eq!(s.emit(3), Outcome::Discarded);
        assert_eq!(s.emit(4), Outcome::Discarded);

        let mut got = Vec::new();
        while let Ok(v) = rx.try_recv() {
            got.push(v);
        }
        assert_eq!(got, vec![1, 2]);
    }

    #[tokio::test]
    async fn 必达通道挂起等待而非丢弃() {
        let (tx, mut rx) = mpsc::channel::<u32>(1);
        let s = GuaranteedSender { tx };

        s.emit(1).await.unwrap();

        let handle = tokio::spawn(async move {
            s.emit(2).await.unwrap();
        });

        // 通道满，发送方挂起
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!handle.is_finished(), "发送方不应完成");

        // 消费掉一个，发送方完成
        assert_eq!(rx.recv().await.unwrap(), 1);
        handle.await.unwrap();
        assert_eq!(rx.recv().await.unwrap(), 2);
    }
}
