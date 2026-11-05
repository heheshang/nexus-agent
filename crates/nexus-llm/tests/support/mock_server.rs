//! 本地 mock HTTP server —— 测试用的最小实现。
//!
//! # 为什么手写而不用 `wiremock`
//!
//! `wiremock` 不在 workspace 依赖里，而**根 manifest 的依赖变更属于跨 crate 的决定**，
//! 需要任务授权（NEX-19 备注）。用 `tokio` 的 `TcpListener` 手写最小 server
//! 既不加依赖，又足够覆盖本任务需要的场景。
//!
//! # 支持的场景
//!
//! - 返回固定 JSON body（非流式）。
//! - 返回固定 SSE 帧序列（流式）。
//! - 返回指定 HTTP 状态码。
//!
//! # 记录收到的请求
//!
//! 服务端把收到的请求体存下来 —— 测试需要断言「客户端发出去的 body 是什么」。

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// mock server 的行为。
#[derive(Debug, Clone)]
pub enum MockBehavior {
    /// 返回固定的非流式 JSON。
    Json {
        /// HTTP 状态码。
        status: u16,
        /// 响应体。
        body: String,
    },
    /// 返回固定的 SSE 帧序列。
    Sse {
        /// HTTP 状态码。
        status: u16,
        /// 帧序列（每项是一个完整的 SSE 事件块）。
        frames: Vec<String>,
    },
}

/// 一个 mock HTTP server。
#[derive(Debug)]
pub struct MockServer {
    /// server 的基地址（不含路径）。
    pub base_url: String,
    /// 收到的请求体（按顺序）。
    received: Arc<Mutex<Vec<String>>>,
}

impl MockServer {
    /// 启动一个 mock server。
    pub async fn start(behavior: MockBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑定随机端口");
        let addr = listener.local_addr().expect("读取本地地址");
        let received = Arc::new(Mutex::new(Vec::new()));

        let received_clone = received.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let behavior = behavior.clone();
                let received = received_clone.clone();

                tokio::spawn(async move {
                    let body = read_request_body(&mut socket).await;
                    received
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(body);

                    let response = render_response(&behavior);
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        Self {
            base_url: format!("http://{addr}/v1"),
            received,
        }
    }

    /// 收到的请求体（按顺序）。
    pub fn received_bodies(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// 读掉 HTTP 请求头与 body。
async fn read_request_body(socket: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    // 先读到头结束。
    let head_end = loop {
        match socket.read(&mut chunk).await {
            Ok(0) => return String::new(),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = find_double_crlf(&buf) {
                    break pos;
                }
            }
            Err(_) => return String::new(),
        }
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < content_length {
        match socket.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }

    String::from_utf8_lossy(&body).to_string()
}

/// 找 `\r\n\r\n` 的位置。
fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// 渲染 HTTP 响应。
fn render_response(behavior: &MockBehavior) -> String {
    match behavior {
        MockBehavior::Json { status, body } => {
            format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        MockBehavior::Sse { status, frames } => {
            let body = frames.join("");
            format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
    }
}
