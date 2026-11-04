//! 降级行为断言：Redis / Milvus 不可用**不阻断** PG 侧工作。
//!
//! 架构文档 §1.3 的存储职责表把 Redis 列为「纯派生 · 可丢弃」，
//! §1.3.1 把它变成**可执行断言**：
//!
//! > 如果移除 Redis，系统必须照常工作（性能下降，功能不变）。
//!
//! 这个断言在本任务的可执行范围是：
//! **Redis 客户端构造失败时，PG 侧的客户端构造**不受任何影响** ——
//! 两者的失败互不牵连。
//!
//! ⚠️ 本文件**不需要真实后端** —— 判据是「构造错误的类型与码」，
//! 而不是「真的跑一遍业务流程」。完整的「停掉 Redis 系统仍工作」
//! 属于 P7/P9 的集成测试范围（架构文档 §5.4 的架构测试清单）。
//!
//! # 为什么不用「黑洞 IP」做不可达地址
//!
//! 直觉上 `192.0.2.1`（RFC 5737 的 TEST-NET-1）是最合适的不可达地址 ——
//! 但**本机网络的出站 TCP 是全部接受的**（实测 `nc -z 192.0.2.1 19530` 成功）。
//! 那台机器上（比如 CI runner）换个网络环境行为就不同 ——
//! **测试结果依赖运行环境**，那是不可接受的。
//!
//! 所以这里用**本机临时监听**：起一个真实的 TCP 监听但不回应 HTTP/2，
//! 于是「接受连接但永不完成握手」—— 一个**确定**的挂起场景。

use std::net::TcpListener;
use std::time::Duration;

use nexus_config::ConfigStack;
use nexus_core::error::ErrorCode;
use nexus_infra::{milvus, pg, redis};
use serde_json::json;

/// 一个**必然连不上**的地址。
///
/// 端口 1 在任何机器上都不会有服务在监听（需要 root 才能绑定）。
const UNREACHABLE: &str = "127.0.0.1:1";

/// 起一个「接受 TCP 但永不回应」的监听器。
///
/// 返回 `(监听地址, 后台线程句柄)`。
///
/// 绑在 `127.0.0.1:0` 上 —— 端口由内核分配，不会撞端口。
/// 后台线程只 `accept` 不读写 —— 客户端的握手永远等不到回应。
fn blackhole_listener() -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定临时端口失败");
    let addr = listener.local_addr().expect("读取本地地址失败");
    let handle = std::thread::spawn(move || {
        // 不断接受连接，但**永不读写** —— 客户端的协议握手会一直挂着。
        for stream in listener.incoming() {
            if stream.is_err() {
                break;
            }
            // 丢弃 `stream`（关闭连接）。
            // 客户端会看到「连上又被关」或「一直等」，两种都是失败。
        }
    });
    (format!("http://{addr}"), handle)
}

fn stack_with(redis_url: &str, database_url: &str) -> ConfigStack {
    let mut stack = ConfigStack::new();
    stack.set_base(json!({
        "redis": { "url": redis_url },
        "database": { "url": database_url }
    }));
    stack
}

#[tokio::test]
async fn a_failing_redis_does_not_prevent_the_pg_side_from_being_constructed() {
    // 两个后端都指向**不可达**的地址。
    let stack = stack_with(
        &format!("redis://{UNREACHABLE}/0"),
        &format!("postgres://u:p@{UNREACHABLE}/nonexistent"),
    );

    // Redis 侧：明确失败，返回 `Result` 而**不是 panic**。
    let redis_err = redis::connect(&stack).await.unwrap_err();
    assert_eq!(
        redis_err.facing.code,
        ErrorCode::StorageUnavailable,
        "Redis 不可达应报 StorageUnavailable（可重试）"
    );

    // PG 侧：同样失败，但**错误是它自己的** —— 不带 Redis 的任何信息。
    //
    // 这断言的是「失败隔离」：Redis 挂掉不会让 PG 的错误变成
    // 「Redis 不可用」之类的误导性信息。
    let pg_err = pg::connect(&stack).await.unwrap_err();
    assert_eq!(pg_err.facing.code, ErrorCode::StorageUnavailable);

    // PG 的错误信息里**不该**出现 Redis 的痕迹。
    let pg_msg = format!("{:?}", pg_err.facing);
    assert!(
        !pg_msg.to_lowercase().contains("redis"),
        "PG 的错误信息不应提到 Redis：{pg_msg}"
    );
}

#[tokio::test]
async fn the_pg_side_stays_usable_when_redis_is_absent_entirely() {
    // **完全不配** Redis —— 这是「移除 Redis」的字面形态。
    let mut stack = ConfigStack::new();
    stack.set_base(json!({
        "database": { "url": format!("postgres://u:p@{UNREACHABLE}/nonexistent") }
    }));

    // Redis 侧：缺配置 → 明确的配置错误。
    let redis_err = redis::connect(&stack).await.unwrap_err();
    assert_eq!(redis_err.facing.code, ErrorCode::InvalidParam);

    // PG 侧：依然能走到「连接」这一步（报的是连接失败，不是配置缺失）。
    let pg_err = pg::connect(&stack).await.unwrap_err();
    assert_eq!(
        pg_err.facing.code,
        ErrorCode::StorageUnavailable,
        "PG 的配置是完整的，不该报配置缺失"
    );
}

#[tokio::test]
async fn the_redis_side_never_panics_on_a_malformed_url() {
    // 一个**格式错误**的 URL —— `redis::Client::open` 会解析失败。
    let stack = stack_with("not-a-redis-url", "postgres://u:p@h/d");

    // 构造失败返回错误，不是 panic。
    let err = redis::connect(&stack).await.unwrap_err();
    assert_eq!(err.facing.code, ErrorCode::InvalidParam);

    // 惰性构造同理。
    let lazy = redis::connect_lazy(&stack);
    assert!(lazy.is_err(), "格式非法的 URL 在惰性构造时也应报错");
}

#[tokio::test]
async fn redis_connect_is_bounded_against_a_hanging_peer() {
    // 连一个「接受 TCP 但永不回应」的地址 —— 握手会一直挂着。
    //
    // 这个测试断言的是「构造有界」：不会永远挂着。
    //
    // ⚠️ 不能用黑洞 IP —— 本机网络会接受那些地址的 TCP 连接（实测）。
    let (url, _handle) = blackhole_listener();
    let stack = stack_with(&url, "postgres://u:p@h/d");

    let started = std::time::Instant::now();
    let result = redis::connect(&stack).await;
    let elapsed = started.elapsed();

    // 预算来自 `redis::DEFAULT_CONNECT_TIMEOUT_SECS`。
    // 留 5 秒余量给 CI 上的调度抖动。
    let budget = Duration::from_secs(redis::DEFAULT_CONNECT_TIMEOUT_SECS + 5);

    assert!(
        elapsed < budget,
        "Redis 构造耗时过长（{elapsed:?}）—— 构造必须有界，预算 {budget:?}"
    );
    assert!(result.is_err(), "挂起的对端应导致构造失败");
}

#[tokio::test]
async fn milvus_construction_is_tcp_level_only() {
    // **实测事实**：milvus 的客户端构造**只做 TCP 连接**，不做 HTTP/2 握手。
    //
    // 证据（实测 `tonic-0.13.1/src/transport/channel/service/connection.rs:90`）：
    // `Connection::connect` 走的是 `ready_oneshot()` ——
    // 那只等**连接器**就绪（TCP 建连），协议握手是**延迟**的。
    //
    // 后果：对端「接受 TCP 但不回应」时，**构造会成功**，
    // 错误要到**第一次真正的 RPC** 才暴露。
    //
    // 这不是 bug，是 gRPC 客户端的常态。但调用方必须知道 ——
    // 「`connect()` 成功」**不等于**「Milvus 可用」。
    let (url, _handle) = blackhole_listener();

    let mut stack = ConfigStack::new();
    stack.set_base(json!({ "milvus": { "url": url } }));

    let started = std::time::Instant::now();
    let result = milvus::connect(&stack).await;
    let elapsed = started.elapsed();

    let budget = Duration::from_secs(milvus::DEFAULT_CONNECT_BUDGET_SECS + 5);

    assert!(
        elapsed < budget,
        "Milvus 构造耗时过长（{elapsed:?}）—— 构造必须有界，预算 {budget:?}"
    );
    assert!(
        result.is_ok(),
        "TCP 层可连时构造应成功（协议握手是延迟的）：{result:?}"
    );
}

#[tokio::test]
async fn milvus_construction_fails_when_tcp_is_refused() {
    // 对端**拒绝** TCP（端口 1）—— 构造必须失败，且**有界**。
    //
    // milvus 的 `ClientBuilder::timeout()` 只设 tonic 的 `Endpoint::timeout`
    // —— 那是**请求**超时，不是**建连**超时。
    // 建连超时是 tonic 的 `connect_timeout`，**未被设置**（实测
    // `tonic-0.13.1/src/transport/channel/endpoint.rs:431`）。
    //
    // 不加总预算的话，遇到「SYN 被丢弃」的主机会挂到 OS 的 TCP 超时（数分钟）。
    let mut stack = ConfigStack::new();
    stack.set_base(json!({ "milvus": { "url": format!("http://{UNREACHABLE}") } }));

    let started = std::time::Instant::now();
    let result = milvus::connect(&stack).await;
    let elapsed = started.elapsed();

    let budget = Duration::from_secs(milvus::DEFAULT_CONNECT_BUDGET_SECS + 5);

    assert!(
        elapsed < budget,
        "Milvus 构造耗时过长（{elapsed:?}）—— 构造必须有界，预算 {budget:?}"
    );
    assert!(result.is_err(), "TCP 被拒时构造应失败");
}
