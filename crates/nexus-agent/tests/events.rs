//! P4 事件 sink 的行为层测试。
//!
//! 覆盖的不变量：
//!
//! - **P4-I18**：事件序号严格单调、无重复、无空洞（并发工具调用下）
//! - **P4-I19**：终态事件**必达**、增量**可丢** —— 填满增量通道后终态仍然送达
//! - **P4-I20**：背压超时**主动取消**整个运行，而不是无限阻塞
//!
//! 以及 NEX-28 的验收条件：
//!
//! - AC #2 序号严格单调、无重复、无空洞，且与写入顺序一致
//! - AC #3 序号分配与写入天然原子 —— 不使用 `Mutex`
//! - AC #4 双通道：`delta` / `reasoning` 走可丢，其余走必达
//! - AC #5 填满可丢通道后，终态事件仍然送达
//! - AC #9 并发工具调用下序号仍严格单调

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use nexus_agent::events::{EventSink, EventSinkConfig, Outcome};
use nexus_core::event::{Event, EventPayload};
use nexus_core::id::{AgentId, RunId, SessionId};
use tokio::sync::Barrier;
use tokio_util::sync::CancellationToken;

// include 共享实现 —— `crates/nexus-core/tests/support/token_scan.rs`
//
// ⚠️ 为什么用 `#[path]` 而不是抽一个 crate
//
// 抽 crate 会**新增**一条依赖边，而本仓库的依赖方向是**已定稿**条款
// （架构文档 §6.3 第 5 项）。`#[path]` include 只需要**文件系统**上的相对路径，
// **不产生任何依赖边** —— `Cargo.toml` 完全不动。
#[path = "../../nexus-core/tests/support/token_scan.rs"]
mod token_scan;

// ═══════════════════════════════════════════════════════════
// 辅助
// ═══════════════════════════════════════════════════════════

fn run_id() -> RunId {
    RunId::new(1)
}

fn small_config(lossy: usize, guaranteed: usize) -> EventSinkConfig {
    EventSinkConfig {
        lossy_buffer: lossy,
        guaranteed_buffer: guaranteed,
        backpressure_timeout: Duration::from_millis(200),
    }
}

/// 收集流里的所有事件。
///
/// ⚠️ 调用方必须**先 drop 掉 sink** —— 否则 ingress 通道不会关闭，
/// 流永远不会结束，测试会挂住。
async fn collect(mut stream: nexus_agent::events::EventStream) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("不应有流错误"));
    }
    events
}

/// 断言序号严格单调、无重复、无空洞（从 1 开始）。
fn assert_strictly_monotonic(events: &[Event]) {
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();

    for pair in seqs.windows(2) {
        assert_eq!(
            pair[1],
            pair[0] + 1,
            "序号必须严格递增且无空洞 —— 实得 {pair:?}"
        );
    }

    let unique: HashSet<u64> = seqs.iter().copied().collect();
    assert_eq!(unique.len(), seqs.len(), "序号不得重复 —— 实得 {seqs:?}");
}

// ═══════════════════════════════════════════════════════════
// AC #2 / #3 / P4-I18：序号分配
// ═══════════════════════════════════════════════════════════

/// AC #2：序号从 1 开始，严格单调，无重复，无空洞。
#[tokio::test]
async fn sequence_starts_at_one_and_is_strictly_monotonic() {
    let (sink, stream) = EventSink::new(run_id(), small_config(16, 16));

    for i in 0..10 {
        sink.emit_lossy(EventPayload::Delta {
            content: format!("片段 {i}"),
        })
        .await
        .expect("应发送成功");
    }

    drop(sink);
    let events = collect(stream).await;
    assert_eq!(events.len(), 10);

    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs[0], 1, "序号必须从 1 开始");
    assert_strictly_monotonic(&events);
}

/// AC #2：序号与**写入顺序**一致 —— 必达通道内保序。
#[tokio::test]
async fn sequence_matches_write_order_within_the_guaranteed_channel() {
    let (sink, stream) = EventSink::new(run_id(), small_config(16, 16));

    // 必达事件：tool_call → tool_result 的配对顺序必须保持。
    for i in 0..5 {
        sink.emit_guaranteed(EventPayload::ToolCall {
            call_id: format!("call-{i}").into(),
            name: format!("tool-{i}"),
            arguments: serde_json::json!({"q": i}),
        })
        .await
        .expect("应发送成功");
    }

    drop(sink);
    let events = collect(stream).await;
    let names: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventPayload::ToolCall { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(names.len(), 5);
    assert_strictly_monotonic(&events);

    // 序号递增的方向必须与写入方向一致。
    for (i, name) in names.iter().enumerate() {
        let event = events
            .iter()
            .find(|e| matches!(&e.payload, EventPayload::ToolCall { name: n, .. } if n == name))
            .expect("应找到该 tool_call");
        assert_eq!(
            event.seq,
            (i + 1) as u64,
            "第 {i} 个写入的事件序号应为 {}",
            i + 1
        );
    }
}

/// AC #9 / P4-I18：**并发**生产者下序号仍严格单调、无重复、无空洞。
///
/// 这是「单点分配」相对方案 A（`fetch_add`）的核心区别：
/// 多个 task 同时发送，序号仍由**唯一**的分配 task 赋予。
///
/// ⚠️ **必须有并发消费者** —— 必达通道在写满时**挂起等待**
/// （那正是「必达」的语义）。所以这里一边写一边读。
#[tokio::test]
async fn concurrent_producers_still_yield_a_strictly_monotonic_sequence() {
    const PRODUCERS: usize = 8;
    const PER_PRODUCER: usize = 25;

    let (sink, mut stream) = EventSink::new(run_id(), small_config(64, 64));
    let sink = Arc::new(sink);

    // 并发消费者 —— 把收��的事件收集起来。
    let collected = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let consumer = {
        let collected = Arc::clone(&collected);
        tokio::spawn(async move {
            while let Some(item) = stream.next().await {
                collected.lock().await.push(item.expect("不应有流错误"));
            }
        })
    };

    // 用屏障让所有生产者**同时**起跑 —— 制造最大并发。
    let barrier = Arc::new(Barrier::new(PRODUCERS));

    let mut handles = Vec::new();
    for p in 0..PRODUCERS {
        let sink = Arc::clone(&sink);
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            for i in 0..PER_PRODUCER {
                sink.emit_guaranteed(EventPayload::AgentEvent {
                    kind: nexus_core::event::AgentEventKind::Progress,
                    expert: format!("expert-{p}"),
                    detail: format!("step-{i}"),
                })
                .await
                .expect("应发送成功");
            }
        }));
    }

    for handle in handles {
        handle.await.expect("task 不应 panic");
    }

    // 消费端在所有生产者结束后会自然结束（sink 被 drop）。
    drop(sink);
    consumer.await.expect("消费 task 不应 panic");

    let events = collected.lock().await;
    assert_eq!(
        events.len(),
        PRODUCERS * PER_PRODUCER,
        "所有事件都必须送达 —— 必达通道不丢事件"
    );
    assert_strictly_monotonic(&events);

    // 序号必须是 1..N 的**完整集合** —— 无空洞。
    let seqs: HashSet<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs.len(), events.len(), "序号不得重复");
    assert_eq!(*seqs.iter().min().unwrap(), 1, "最小序号必须是 1");
    assert_eq!(
        *seqs.iter().max().unwrap(),
        events.len() as u64,
        "最大序号必须是事件总数 —— 否则有空洞"
    );
}

/// AC #3：序号分配与写入**天然原子** —— 不用 `Mutex`。
///
/// 这条是**结构层**断言：源码里不得出现 `Mutex`。
#[test]
fn the_sink_does_not_use_a_mutex() {
    let source = include_str!("../src/events.rs");

    // ⚠️ token 级扫描 —— 注释里提到 `Mutex` 是**解释**，不是使用。
    //
    // 旧实现用「剥注释后搜文本」，而那份剥注释器**没有字符串状态** ——
    // `"http://x"` 里的 `//` 会启动行注释，把**该行之后**的真实代码整段丢弃
    // （那是**真漏报**）。详见 `crates/nexus-core/tests/support/token_scan.rs`。
    let scanned = token_scan::scan_code(source).expect("解析失败");

    assert!(
        !scanned.identifiers.contains("Mutex"),
        "序号分配必须用**单点分配 task**（方案 C），不得用 `Mutex`"
    );
    assert!(
        !scanned.calls.contains("fetch_add"),
        "方案 A（`fetch_add`）不保证序号与写入的原子性 —— 不得使用"
    );

    // ⚠️ **单点分配**的可判定形式 —— 源码里只允许**一处**推进序号。
    //
    // 这条比「不用 `Mutex` / 不用 `fetch_add`」更强：它不关心用什么同步原语，
    // 只要求「序号在**唯一**的地方推进」。
    //
    // 行为测试抓不住方案 A —— 并发下「序号与到达顺序脱节」需要极端重排才显现，
    // 概率太低。所以这条**结构层**断言才是方案 A 的真正防线。
    //
    // ⚠️ 判据要**足够宽** —— 任何对 `seq` 的**自增或赋值**都算「推进序号」，
    // 不管它叫什么名字、用什么形式写。
    //
    // ⚠️ 用 **AST** 判定而不是「逐行搜文本」——
    // 逐行搜会**假阳性**（注释里的 `// seq += 1` 被算成一处推进）
    // 也会**假阴性**（`self.seq += 1` 被漏掉）。
    let advancements = token_scan::find_advancements(source, "seq").expect("解析失败");
    assert_eq!(
        advancements.len(),
        1,
        "序号必须只在**唯一**的地方推进（单点分配）—— 实得 {} 处：{advancements:?}",
        advancements.len()
    );
}

// ═══════════════════════════════════════════════════════════
// AC #4 / #5 / P4-I19：双通道
// ═══════════════════════════════════════════════════════════

/// AC #4：只有 `delta` / `reasoning` 是可丢，其余全部必达。
#[test]
fn only_delta_and_reasoning_are_lossy() {
    let lossy: Vec<EventPayload> = vec![
        EventPayload::Delta {
            content: "x".into(),
        },
        EventPayload::Reasoning {
            content: "x".into(),
        },
    ];
    for payload in &lossy {
        assert_eq!(
            payload.channel(),
            nexus_core::event::EventChannel::Lossy,
            "增量必须是可丢的"
        );
    }

    let guaranteed: Vec<EventPayload> = vec![
        EventPayload::Meta {
            session_id: SessionId::new_v4(),
            agent_id: AgentId::new(1),
        },
        EventPayload::Plan {
            steps: vec!["a".into()],
            current: 0,
        },
        EventPayload::ToolCall {
            call_id: "c".into(),
            name: "n".into(),
            arguments: serde_json::json!({}),
        },
        EventPayload::ToolResult {
            call_id: "c".into(),
            result: "r".into(),
            elapsed_ms: 1,
            status: nexus_core::event::ToolStatus::Ok,
        },
        EventPayload::AgentEvent {
            kind: nexus_core::event::AgentEventKind::Delegate,
            expert: "e".into(),
            detail: "d".into(),
        },
        EventPayload::Refs { items: vec![] },
        EventPayload::Usage {
            prompt_tokens: 1,
            completion_tokens: 1,
            cost: None,
            model: "m".into(),
        },
        EventPayload::Approval {
            approval_id: "a".into(),
            kind: "k".into(),
            payload: serde_json::json!({}),
        },
        EventPayload::Done {
            finish_reason: nexus_core::event::FinishReason::Stop,
        },
        EventPayload::Error {
            code: 90001,
            message: "m".into(),
        },
    ];
    for payload in &guaranteed {
        assert_eq!(
            payload.channel(),
            nexus_core::event::EventChannel::Guaranteed,
            "非增量事件必须必达 —— 丢了就是语义丢失"
        );
    }
}

/// AC #5 / P4-I19：填满可丢通道后，**终态事件仍然送达**。
#[tokio::test]
async fn terminal_events_are_delivered_even_after_the_lossy_channel_is_full() {
    // 可丢通道容量 2 —— 故意很小。
    let (sink, stream) = EventSink::new(run_id(), small_config(2, 8));

    // 塞满可丢通道：容量 2，加上分配 task 中转，实际能多塞几条。
    // 发足够多，确保可丢通道**确定**被填满。
    let mut discarded = 0usize;
    for i in 0..50 {
        let outcome = sink
            .emit_lossy(EventPayload::Delta {
                content: format!("片段 {i}"),
            })
            .await
            .expect("可丢通道满时不应报错");
        if outcome == Outcome::Discarded {
            discarded += 1;
        }
    }
    assert!(
        discarded > 0,
        "可丢通道应当被填满并开始丢弃 —— 否则这条测试没测到东西"
    );

    // **终态**事件必须送达。
    sink.emit_guaranteed(EventPayload::Done {
        finish_reason: nexus_core::event::FinishReason::Stop,
    })
    .await
    .expect("终态事件必须送达");

    drop(sink);
    let events = collect(stream).await;
    assert!(
        events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::Done { .. })),
        "填满可丢通道后，终态事件仍然必须送达"
    );
}

/// AC #4：可丢通道满时**不阻塞** —— 立即返回 `Discarded`。
#[tokio::test]
async fn a_full_lossy_channel_never_blocks() {
    let (sink, _stream) = EventSink::new(run_id(), small_config(2, 8));

    // 用 `timeout` 包住 —— 如果阻塞了就会超时。
    let result = tokio::time::timeout(Duration::from_millis(200), async {
        for i in 0..50 {
            sink.emit_lossy(EventPayload::Delta {
                content: format!("片段 {i}"),
            })
            .await
            .expect("不应报错");
        }
    })
    .await;

    assert!(result.is_ok(), "可丢通道满了也**不得阻塞**");
}

/// 必达通道满时**挂起等待**，消费掉一个后完成 —— 保证送达。
#[tokio::test]
async fn a_full_guaranteed_channel_waits_instead_of_dropping() {
    let (sink, mut stream) = EventSink::new(run_id(), small_config(2, 2));

    // 先填满必达通道（ingress 容量 2）。
    for i in 0..2 {
        sink.emit_guaranteed(EventPayload::AgentEvent {
            kind: nexus_core::event::AgentEventKind::Progress,
            expert: "e".into(),
            detail: format!("{i}"),
        })
        .await
        .expect("应发送成功");
    }

    // 第三条会挂起 —— 因为必达通道满了。
    let pending = sink.emit_guaranteed(EventPayload::Done {
        finish_reason: nexus_core::event::FinishReason::Stop,
    });
    tokio::pin!(pending);

    // ⚠️ 断言它**挂起**而不是立即完成或立即丢弃。
    //
    // 用 `select!` 而不是 `is_finished()` —— 后者在 opaque future 上不可用。
    tokio::select! {
        result = &mut pending => {
            panic!("必达通道满时发送方应**挂起等待**，而不是丢弃 —— 但它返回了 {result:?}");
        }
        _ = tokio::time::sleep(Duration::from_millis(100)) => {}
    }

    // 消费掉一条 —— 挂起的发送应该完成。
    let first = stream.next().await.expect("应有事件").expect("不应有错");
    assert!(matches!(first.payload, EventPayload::AgentEvent { .. }));

    tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .expect("消费掉一个后应完成")
        .expect("应发送成功");
}

// ═══════════════════════════════════════════════════════════
// AC #6 / P4-I20：背压主动取消
// ═══════════════════════════════════════════════════════════

/// AC #6 / P4-I20：背压超时**主动取消**整个运行，而不是无限阻塞。
#[tokio::test]
async fn backpressure_timeout_cancels_the_run_instead_of_blocking_forever() {
    let config = EventSinkConfig {
        lossy_buffer: 2,
        guaranteed_buffer: 2,
        backpressure_timeout: Duration::from_millis(100),
    };
    let (sink, _stream) = EventSink::new(run_id(), config);
    let cancel = sink.cancel_token();

    // 填满必达通道。
    for i in 0..2 {
        sink.emit_guaranteed(EventPayload::AgentEvent {
            kind: nexus_core::event::AgentEventKind::Progress,
            expert: "e".into(),
            detail: format!("{i}"),
        })
        .await
        .expect("应发送成功");
    }

    // 第三条会挂起 —— 消费端**永不**消费。
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        sink.emit_guaranteed(EventPayload::Done {
            finish_reason: nexus_core::event::FinishReason::Stop,
        }),
    )
    .await;

    let err = result
        .expect("不应无限阻塞 —— 必须在超时后返回")
        .expect_err("应返回错误");
    assert_eq!(
        err.user_facing().code,
        nexus_core::error::ErrorCode::ServiceUnavailable,
        "背压超时应报 `ServiceUnavailable`（90003）"
    );

    // ⚠️ 关键：**主动取消**了。
    assert!(
        cancel.is_cancelled(),
        "背压超时必须**主动取消**整个运行，而不是无限阻塞"
    );
}

/// 背压超时后，后续的必达发送应立即失败 —— 不再无限等待。
#[tokio::test]
async fn after_backpressure_the_sink_stops_accepting_guaranteed_events() {
    let config = EventSinkConfig {
        lossy_buffer: 2,
        guaranteed_buffer: 2,
        backpressure_timeout: Duration::from_millis(50),
    };
    let (sink, _stream) = EventSink::new(run_id(), config);

    for i in 0..2 {
        sink.emit_guaranteed(EventPayload::AgentEvent {
            kind: nexus_core::event::AgentEventKind::Progress,
            expert: "e".into(),
            detail: format!("{i}"),
        })
        .await
        .expect("应发送成功");
    }

    // 触发背压。
    let _ = sink
        .emit_guaranteed(EventPayload::Done {
            finish_reason: nexus_core::event::FinishReason::Stop,
        })
        .await;

    // 之后的发送应快速失败，而不是再等一个超时。
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        sink.emit_guaranteed(EventPayload::Done {
            finish_reason: nexus_core::event::FinishReason::Stop,
        }),
    )
    .await;

    assert!(result.is_ok(), "第二次发送不应无限挂起");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "背压后应快速失败 —— 实际耗时 {:?}",
        started.elapsed()
    );
}

// ═══════════════════════════════════════════════════════════
// 复用既有契约
// ═══════════════════════════════════════════════════════════

/// P4 **不新增**事件类型 —— 事件名与负载形状复用 `nexus_core::EventPayload`。
#[tokio::test]
async fn the_sink_reuses_the_core_event_payload_contract() {
    let (sink, stream) = EventSink::new(run_id(), small_config(4, 4));

    // `Meta` 需要真实的 ID 类型。
    sink.emit_guaranteed(EventPayload::Meta {
        session_id: SessionId::new_v4(),
        agent_id: AgentId::new(1),
    })
    .await
    .expect("应发送成功");

    drop(sink);
    let events = collect(stream).await;
    assert_eq!(events.len(), 1);

    // 事件名必须是既定的 12 个之一 —— 不新增。
    let name = match &events[0].payload {
        EventPayload::Meta { .. } => "meta",
        _ => panic!("负载类型不对"),
    };
    assert!(
        nexus_core::event::EVENT_NAMES.contains(&name),
        "事件名必须在 `EVENT_NAMES` 内 —— 不得新增"
    );
    assert_eq!(
        nexus_core::event::EVENT_NAMES.len(),
        12,
        "12 个事件名是**已定稿**的契约 —— 不得增减"
    );
}

/// 事件必须带上 `run_id` —— 消费端按它分流。
#[tokio::test]
async fn every_event_carries_the_run_id() {
    let id = RunId::new(42);
    let (sink, stream) = EventSink::new(id, small_config(4, 4));

    sink.emit_lossy(EventPayload::Delta {
        content: "x".into(),
    })
    .await
    .expect("应发送成功");

    drop(sink);
    let events = collect(stream).await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].run_id, RunId::new(42));
}

/// 取消令牌可共享 —— 运行体用它取消上游工作。
#[tokio::test]
async fn the_cancel_token_is_shared_between_clones() {
    let (sink, _stream) = EventSink::new(run_id(), small_config(4, 4));
    let clone = sink.clone();

    let token = sink.cancel_token();
    assert!(!token.is_cancelled());

    clone.cancel_token().cancel();
    assert!(token.is_cancelled(), "克隆体取消应影响同一个 token");
}

/// 哨兵 —— 确保 `CancellationToken` 的导入被用到（避免未使用告警）。
#[test]
fn the_cancel_token_type_is_usable() {
    let token = CancellationToken::new();
    assert!(!token.is_cancelled());
}
