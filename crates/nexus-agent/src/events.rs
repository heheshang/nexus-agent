//! 事件 sink：**单点分配**序号 + **双通道**背压。
//!
//! # 为什么 P4 不复用 `nexus_core::EventSink`
//!
//! 两者形状不同，强行统一会让其中一个变形：
//!
//! | | `nexus_core::EventSink` | P4 需要的 |
//! |---|---|---|
//! | 写入方式 | `emit(&mut self)` —— **同步** | **异步**发送 |
//! | 写者 | **单写者**（`&mut self` 编译期串行化） | **多个并发生产者** |
//! | 背压 | 无 | **必须有**（消费者太慢 → 超时 → 主动取消） |
//! | 序号 | 调用方同步分配 | 需要**单点分配**（与写入原子） |
//!
//! `nexus_core` 的是 P1 定稿的**单写者**保证，改成异步多写者会**破坏**
//! 架构文档 §2.2.3 的核心不变量。
//!
//! **这是「并存」不是「替换」。**
//!
//! # 序号分配：单点分配，不用锁
//!
//! 参考架构 §2.2 给了三个方案，**推荐方案 C**：
//!
//! ```text
//! ingress mpsc（生产者只管 send）→ 唯一的序号分配 task → output mpsc（消费者读的流）
//! ```
//!
//! 方案 A（`AtomicU64` + `fetch_add`）**不够** —— 它保证序号唯一，
//! 但**不保证与通道写入的原子性**：事件 A 可能拿到 `seq=1` 却比 `seq=2`
//! 更晚到达通道。如果前端依赖严格有序，方案 A 不够。
//!
//! 本实现是方案 C 的强化版：**单一 ingress 通道**（全局 FIFO）
//! + **单一 output 通道**（保证消费端看到的序号严格递增）
//! + **按语义分开的信号量**（保证「可丢」的积压不会挤占「必达」的容量）。
//!
//! # 双通道
//!
//! 架构文档 §3.2 定稿：只有 `delta` / `reasoning` 是**可丢**的，其余全部**必达**。
//!
//! - **可丢**满了返回 [`Outcome::Discarded`]，**不阻塞**；
//! - **必达**满了**挂起等待**，直到送达或超时。
//!
//! 背压超时 → **主动取消**整个运行（不是无限阻塞）。
//!
//! # 为什么用两个信号量而不是两个 output 通道
//!
//! 两个 output 通道会导致消费端**合并**时序号乱序 —— 可丢通道里排着
//! `seq=5`，必达通道里排着 `seq=6`，消费端可能先读到 6。
//! 那就破坏了「序号严格单调」这条不变量。
//!
//! 两个信号量把「可丢 / 必达」的**容量语义**分开，但**序号与到达顺序**
//! 仍由单一 output 通道保证 —— 两边都拿到了。

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures::Stream;
use futures::stream::StreamExt;
use nexus_core::error::{AppError, ErrorCode};
use nexus_core::event::{Event, EventChannel, EventPayload};
use nexus_core::id::RunId;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

/// 写入结果。
///
/// 丢弃是**显式返回**的 —— 调用方可以计数上报（架构文档 §3.2.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 已入队。
    Delivered,
    /// **可丢**通道已满，事件被丢弃。
    ///
    /// 消费者会从后续增量或终态事件得知结论。
    Discarded,
}

/// 事件流的错误。
#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
    /// 通道已关闭。
    #[error("事件通道已关闭")]
    SourceClosed,
}

/// 事件流。
///
/// 消费侧拿到的是一个 `Stream` —— 与 `futures::StreamExt`、`tokio::select!`
/// 天然组合。
pub type EventStream =
    Pin<Box<dyn Stream<Item = Result<Event, EventStreamError>> + Send + 'static>>;

/// 事件写入器（发送端）。
///
/// **可克隆** —— 因为 ReAct 循环里多个并发生产者（多工具、模型调用）
/// 都要能写入同一个 sink。
///
/// 序号由**唯一的分配 task** 分配，所以本类型**不需要** `Mutex`。
#[derive(Debug, Clone)]
pub struct EventSink {
    run_id: RunId,
    /// 唯一的 ingress 通道 —— 生产者只管 send，**不分配序号**。
    ingress_tx: mpsc::Sender<Envelope>,
    /// 可丢事件的容量信号量。
    lossy_permits: Arc<Semaphore>,
    /// 必达事件的容量信号量。
    guaranteed_permits: Arc<Semaphore>,
    /// 背压超时时**主动取消**整个运行。
    cancel: CancellationToken,
    /// 背压超时时长。
    timeout: Duration,
}

/// 内部信封 —— 携带**可选**的确认通道与容量许可。
#[derive(Debug)]
struct Envelope {
    /// 事件负载。
    payload: EventPayload,
    /// 确认通道 —— 只有**必达**事件才等确认。
    ///
    /// 可丢事件**不等** —— 等确认会让「可丢」在下游整体卡住时也阻塞，
    /// 违背「可丢绝不阻塞」这条语义。
    ack: Option<oneshot::Sender<()>>,
    /// 容量许可 —— **随事件一路走到消费端**。
    ///
    /// ⚠️ 许可必须在**消费时**归还，而不是写入时。
    /// 若在写入时就归还，「已写入但还没被消费」的事件就**不占容量**了 ——
    /// 那「可丢」的积压就会挤占「必达」的容量，双通道语义就没了。
    permit: Option<OwnedSemaphorePermit>,
}

/// 已写入的事件 + 它的容量许可。
#[derive(Debug)]
struct Outbound {
    /// 事件。
    event: Event,
    /// 该事件占用的容量许可 —— 消费后归还。
    permit: Option<OwnedSemaphorePermit>,
}

/// sink 的配置。
#[derive(Debug, Clone)]
pub struct EventSinkConfig {
    /// 可丢通道容量。
    pub lossy_buffer: usize,
    /// 必达通道容量。
    pub guaranteed_buffer: usize,
    /// 背压超时。
    pub backpressure_timeout: Duration,
}

impl Default for EventSinkConfig {
    fn default() -> Self {
        Self {
            // 参考架构 §4.2 的 `EventBuffer=128`。
            lossy_buffer: 128,
            guaranteed_buffer: 128,
            // 与参考实现同量级 —— 足够慢的消费者能等到，
            // 足够快的消费者不会被误杀。
            backpressure_timeout: Duration::from_secs(5),
        }
    }
}

impl EventSink {
    /// 创建一个 sink，返回**发送端**与**接收流**。
    ///
    /// 内部会**启动一个 task** 作为唯一的序号分配者。
    ///
    /// ⚠️ 流在**所有** [`EventSink`] 克隆被丢弃后才结束 ——
    /// 因为 ingress 通道要等最后一个发送端关闭。
    pub fn new(run_id: RunId, config: EventSinkConfig) -> (Self, EventStream) {
        let EventSinkConfig {
            lossy_buffer,
            guaranteed_buffer,
            backpressure_timeout,
        } = config;

        // 单一 ingress —— 全局 FIFO，序号按**入队顺序**分配。
        let ingress_capacity = lossy_buffer.saturating_add(guaranteed_buffer).max(1);
        let (ingress_tx, mut ingress_rx) = mpsc::channel::<Envelope>(ingress_capacity);

        // 单一 output —— 消费端看到的序号严格递增。
        let (output_tx, output_rx) = mpsc::channel::<Outbound>(ingress_capacity);

        // 按语义分开的容量信号量。
        //
        // ⚠️ 这两个信号量把「可丢」与「必达」的**容量**分开 ——
        // 可丢事件的积压**不会**挤占必达事件的容量。
        //
        // 这比「一个通道 + 保留槽位」好：后者是**运行时约定**，
        // 靠计数维护，多个写入点时正确性靠人保证（架构文档 §3.2.3）。
        let lossy_permits = Arc::new(Semaphore::new(lossy_buffer));
        let guaranteed_permits = Arc::new(Semaphore::new(guaranteed_buffer));

        // 唯一的序号分配 task。
        tokio::spawn(async move {
            let mut seq: u64 = 0;

            while let Some(envelope) = ingress_rx.recv().await {
                let Envelope {
                    payload,
                    ack,
                    permit,
                } = envelope;

                seq += 1;
                let event = build_event(run_id, seq, payload);
                let outbound = Outbound { event, permit };
                if output_tx.send(outbound).await.is_err() {
                    break;
                }

                // ⚠️ 必达事件：确认「**已写出**」——
                // 调用方等到这个确认，才算「必达」兑现。
                //
                // ⚠️ 必须是 `send` 而不是 `drop` ——
                // `drop` 会**关闭** oneshot，接收方读到的是「通道关闭」
                // 而不是「已写出」。
                if let Some(ack) = ack {
                    let _ = ack.send(());
                }
            }
        });

        let sink = Self {
            run_id,
            ingress_tx,
            lossy_permits,
            guaranteed_permits,
            cancel: CancellationToken::new(),
            timeout: backpressure_timeout,
        };

        let stream = build_stream(output_rx);
        (sink, stream)
    }

    /// 所属的执行记录。
    pub fn run_id(&self) -> RunId {
        self.run_id
    }

    /// 本 sink 的取消令牌 —— 供运行体在结束时取消上游工作。
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// 写入一个**可丢**事件（增量）。
    ///
    /// 通道满时**丢弃**并返回 [`Outcome::Discarded`] —— **绝不阻塞**。
    ///
    /// 丢弃增量是安全的：消费者会从后续增量或终态事件得知结论。
    pub async fn emit_lossy(&self, payload: EventPayload) -> Result<Outcome, AppError> {
        debug_assert!(payload.channel() == EventChannel::Lossy);

        // ⚠️ **先拿容量许可**，再入队。
        //
        // 顺序很重要：许可必须在**入队之前**拿到，
        // 否则「已入队但没容量」的增量会挤占必达事件的容量。
        //
        // `try_acquire` **不挂起** —— 满了立即失败。
        let Ok(permit) = self.lossy_permits.clone().try_acquire_owned() else {
            return Ok(Outcome::Discarded);
        };

        match self.ingress_tx.try_reserve() {
            Ok(reservation) => {
                reservation.send(Envelope {
                    payload,
                    ack: None,
                    permit: Some(permit),
                });
                Ok(Outcome::Delivered)
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                // ingress 满了 —— 增量丢弃。
                // ⚠️ 许可随 `permit` 一起被丢弃 —— **自动归还**。
                Ok(Outcome::Discarded)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(closed_error()),
        }
    }

    /// 写入一个**必达**事件。
    ///
    /// 通道满时**挂起等待** —— 保证送达。
    ///
    /// 等待超过**背压超时** → **主动取消**整个运行（不是无限阻塞）。
    pub async fn emit_guaranteed(&self, payload: EventPayload) -> Result<(), AppError> {
        debug_assert!(payload.channel() == EventChannel::Guaranteed);

        // ⚠️ 已被取消 —— **立即失败**，不再等一个超时。
        //
        // 背压超时后整个运行已被取消 —— 此时再等一个超时是**浪费**，
        // 而且会让调用方以为「还有机会送达」。
        if self.cancel.is_cancelled() {
            return Err(AppError::new(
                ErrorCode::ServiceUnavailable,
                "运行已被取消，事件不再投递",
            ));
        }

        // ⚠️ **先等容量**，再入队。
        //
        // 这一步挂起 —— 那正是背压的表现。
        let permit = match tokio::time::timeout(
            self.timeout,
            self.guaranteed_permits.clone().acquire_owned(),
        )
        .await
        {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return Err(closed_error()),
            Err(_) => return Err(self.backpressure_error()),
        };

        let (ack_tx, ack_rx) = oneshot::channel();

        // ⚠️ `reserve` 是**可取消安全**的 —— 超时取消它不会丢掉已发送的消息。
        // （`send` 不可取消安全 —— 超时会静默丢消息。）
        let reservation = match tokio::time::timeout(self.timeout, self.ingress_tx.reserve()).await
        {
            Ok(Ok(reservation)) => reservation,
            Ok(Err(_)) => return Err(closed_error()),
            Err(_) => return Err(self.backpressure_error()),
        };

        // ⚠️ 许可移交给信封 —— 随事件一路走到消费端才归还。
        // `Permit::send` 消耗 `reservation`，返回 `()` —— 许可已在信封里。
        reservation.send(Envelope {
            payload,
            ack: Some(ack_tx),
            permit: Some(permit),
        });

        // 等**真正写出** —— 这才是「必达」的含义。
        match tokio::time::timeout(self.timeout, ack_rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(closed_error()),
            Err(_) => Err(self.backpressure_error()),
        }
    }

    /// 按**语义**写入 —— 调用方不必自己判断通道。
    ///
    /// 负载的类型**决定**通道（架构文档 §3.2）：
    /// `delta` / `reasoning` 走可丢，其余走必达。
    pub async fn emit(&self, payload: EventPayload) -> Result<Outcome, AppError> {
        match payload.channel() {
            EventChannel::Lossy => self.emit_lossy(payload).await,
            EventChannel::Guaranteed => self
                .emit_guaranteed(payload)
                .await
                .map(|()| Outcome::Delivered),
        }
    }

    /// 背压超时 —— **主动取消**整个运行。
    fn backpressure_error(&self) -> AppError {
        // 取消**不区分**是谁造成的背压 —— 整个运行一起停。
        self.cancel.cancel();
        AppError::new(
            ErrorCode::ServiceUnavailable,
            "事件背压：消费者过慢，已取消运行",
        )
    }
}

/// 构造一个带序号的事件。
fn build_event(run_id: RunId, seq: u64, payload: EventPayload) -> Event {
    Event {
        run_id,
        seq,
        payload,
    }
}

/// 通道已关闭的错误。
fn closed_error() -> AppError {
    AppError::new(ErrorCode::ServiceUnavailable, "事件通道已关闭")
}

/// 把 output 通道包装成流，并在**消费**时归还对应语义的容量。
///
/// ⚠️ 容量许可随事件一路走到消费端 —— 消费时（`Outbound` 被 drop）才归还。
/// 这保证「已写入但还没被消费」的事件**仍然占着容量** ——
/// 所以「可丢」的积压**不会**挤占「必达」的容量。
fn build_stream(output_rx: mpsc::Receiver<Outbound>) -> EventStream {
    futures::stream::unfold(output_rx, |mut rx| async move {
        let outbound = rx.recv().await?;
        // ⚠️ 拆开信封：事件交给消费端，**许可随信封被 drop 时归还**。
        let Outbound { event, permit } = outbound;
        drop(permit);
        Some((Ok(event), rx))
    })
    .boxed()
}

/// 把 sink 包成 `Arc` 便于在多个 task 间共享。
pub type SharedEventSink = Arc<EventSink>;
