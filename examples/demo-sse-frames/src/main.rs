//! demo-sse-frames
//!
//! 验证 SSE 帧边界的处理：**TCP 分包可能把一帧劈成两半**。
//!
//! 这是流式对话最容易出错的地方：
//! - 写侧：必须缓冲半帧，否则写出格式错误的帧
//! - 读侧：**更危险** —— 收到半帧就解析会得到截断的 JSON
//!
//! 帧格式：
//! ```text
//! event: delta
//! data: {"content":"..."}
//! <空行>
//! ```

use bytes::{BufMut, BytesMut};
use serde_json::Value;

/// SSE 帧分隔符。
const FRAME_DELIMITER: &[u8] = b"\n\n";

/// 把负载编码为 SSE 帧。
pub fn encode(event: &str, data: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(data).expect("序列化不应失败");
    let mut buf = BytesMut::with_capacity(payload.len() + 32);
    buf.put_slice(b"event: ");
    buf.put_slice(event.as_bytes());
    buf.put_slice(b"\ndata: ");
    buf.put_slice(&payload);
    buf.put_slice(b"\n\n");
    buf.to_vec()
}

/// 心跳注释帧。
pub fn ping() -> Vec<u8> {
    b": ping\n\n".to_vec()
}

/// 解析后的帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub event: String,
    pub data: String,
}

/// SSE 帧解码器（增量式）。
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: BytesMut,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段字节，返回其中所有**完整**的帧。
    ///
    /// 不完整的尾部会被缓冲，等待后续数据。
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(chunk);

        let mut frames = Vec::new();
        while let Some(pos) = self.buf.windows(2).position(|w| w == FRAME_DELIMITER) {
            // ⚠️ 必须把分隔符**一起**切走。
            // 只 split_to(pos) 会把 "\n\n" 留在缓冲区头部，
            // 下一轮 position() 又返回 0 → 死循环。
            let frame_with_delim = self.buf.split_to(pos + FRAME_DELIMITER.len());
            // 去掉尾部分隔符
            let mut frame = frame_with_delim[..pos].to_vec();
            // 去掉尾部可能的 '\r'（CRLF 变体）
            if frame.last() == Some(&b'\r') {
                frame.pop();
            }
            frames.push(frame);
        }
        frames
    }

    /// 当前缓冲的半帧字节数。
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

/// 解析一帧。
pub fn parse_frame(raw: &[u8]) -> Result<Frame, String> {
    let text = String::from_utf8_lossy(raw);
    let mut event = String::new();
    let mut data_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        // 注释行（心跳）忽略
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim_start().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    if event.is_empty() && data_lines.is_empty() {
        return Err(format!("帧既无 event 也无 data：{text:?}"));
    }
    Ok(Frame {
        event,
        // SSE 规范：多行 data 用 \n 连接
        data: data_lines.join("\n"),
    })
}

fn main() {
    // ── 1. 编码格式 ──
    let frame = encode("delta", &serde_json::json!({"content": "hi"}));
    println!("编码 = {:?}", String::from_utf8_lossy(&frame));
    assert_eq!(
        String::from_utf8(frame.clone()).unwrap(),
        "event: delta\ndata: {\"content\":\"hi\"}\n\n"
    );

    // ── 2. 逐字节喂入（最坏情况：每帧被切成 N 片）──
    let multi = [
        encode("delta", &serde_json::json!({"content": "第一段"})),
        encode(
            "tool_call",
            &serde_json::json!({"id": "c1", "name": "search"}),
        ),
        encode("done", &serde_json::json!({"finish_reason": "stop"})),
    ];
    let total: Vec<u8> = multi.iter().flatten().copied().collect();

    println!("\n--- 逐字节喂入（共 {} 字节）---", total.len());
    let mut decoder = FrameDecoder::new();
    let mut frames = Vec::new();
    for (i, b) in total.iter().enumerate() {
        frames.extend(decoder.push(&[*b]));
        if i < 8 {
            println!("  喂入第 {} 字节 → 完整帧数 = {}", i + 1, frames.len());
        }
    }
    println!("  最终完整帧数 = {}", frames.len());
    assert_eq!(frames.len(), 3, "逐字节喂入应得到 3 个完整帧");

    for f in &frames {
        let parsed = parse_frame(f).unwrap();
        println!("  event={:<10} data={}", parsed.event, parsed.data);
    }

    // ── 3. 任意位置切断都能正确重组 ──
    println!("\n--- 任意位置切断 ---");
    let single = encode("delta", &serde_json::json!({"content": "hello world"}));
    for split in 1..single.len() {
        let mut d = FrameDecoder::new();
        let mut got = d.push(&single[..split]);
        got.extend(d.push(&single[split..]));
        assert_eq!(got.len(), 1, "split={split} 帧数不对");
        let p = parse_frame(&got[0]).unwrap();
        assert_eq!(p.event, "delta");
        assert_eq!(p.data, r#"{"content":"hello world"}"#);
    }
    println!("✓ 在 {} 个切分位置都正确重组", single.len() - 1);

    // ── 4. 半帧被缓冲而非丢弃 ──
    let mut d = FrameDecoder::new();
    let got = d.push(&single[..single.len() - 5]);
    println!("\n--- 半帧处理 ---");
    println!("  吐出完整帧 = {}（应为 0）", got.len());
    println!("  缓冲字节数 = {}", d.buffered());
    assert!(got.is_empty());
    assert!(d.buffered() > 0);

    println!("\n✓ 半帧被缓冲，不产生截断的 JSON");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_matches_sse_spec() {
        let out = encode("delta", &serde_json::json!({"content": "hi"}));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "event: delta\ndata: {\"content\":\"hi\"}\n\n"
        );
    }

    #[test]
    fn byte_by_byte_feed_does_not_drop_frames() {
        let multi = [
            encode("delta", &serde_json::json!({"c": 1})),
            encode("done", &serde_json::json!({"c": 2})),
        ];
        let total: Vec<u8> = multi.iter().flatten().copied().collect();

        let mut d = FrameDecoder::new();
        let mut frames = Vec::new();
        for b in &total {
            frames.extend(d.push(&[*b]));
        }
        assert_eq!(frames.len(), 2);
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn separator_must_be_stripped_to_avoid_infinite_loop() {
        // 回归测试：曾经的 bug 是 `split_to(pos)` 没切走 "\n\n"，
        // 导致分隔符留在缓冲区头部，下一轮 position() 返回 0 → 无限循环。
        //
        // 这个测试在旧实现下会**挂住**（而不是失败），所以它同时是
        // 「实现不会退化」的保证。
        let single = encode("delta", &serde_json::json!({"c": 1}));
        let mut d = FrameDecoder::new();

        // 一次喂入完整帧 + 一部分下一帧
        let mut buf = single.clone();
        buf.extend_from_slice(b"event: next\ndata: ");
        let frames = d.push(&buf);

        assert_eq!(frames.len(), 1, "应只吐出一个完整帧");
        assert_eq!(d.buffered(), 18, "剩余应是半帧");
    }

    #[test]
    fn consecutive_frames_leave_no_separator() {
        let mut d = FrameDecoder::new();
        let mut buf = Vec::new();
        for i in 0..5 {
            buf.extend(encode("delta", &serde_json::json!({"i": i})));
        }
        let frames = d.push(&buf);
        assert_eq!(frames.len(), 5);
        assert_eq!(d.buffered(), 0, "缓冲区应清空 —— 不能残留分隔符");
    }

    #[test]
    fn arbitrary_split_points_reassemble_correctly() {
        let single = encode("delta", &serde_json::json!({"content": "hello world"}));
        for split in 1..single.len() {
            let mut d = FrameDecoder::new();
            let mut got = d.push(&single[..split]);
            got.extend(d.push(&single[split..]));
            assert_eq!(got.len(), 1, "split={split}");
            let p = parse_frame(&got[0]).unwrap();
            assert_eq!(p.data, r#"{"content":"hello world"}"#);
        }
    }

    #[test]
    fn partial_frame_is_buffered_not_emitted() {
        let single = encode("delta", &serde_json::json!({"x": 1}));
        let mut d = FrameDecoder::new();
        assert!(d.push(&single[..single.len() - 3]).is_empty());
        assert!(d.buffered() > 0);
    }

    #[test]
    fn multiline_data_is_joined_per_spec() {
        let raw = b"event: delta\ndata: line1\ndata: line2";
        let f = parse_frame(raw).unwrap();
        assert_eq!(f.data, "line1\nline2");
    }

    #[test]
    fn comment_lines_are_ignored() {
        let raw = b": ping\nevent: delta\ndata: {}";
        let f = parse_frame(raw).unwrap();
        assert_eq!(f.event, "delta");
    }

    #[test]
    fn empty_frame_is_error() {
        assert!(parse_frame(b"").is_err());
        assert!(parse_frame(b"foo: bar").is_err());
    }

    #[test]
    fn heartbeat_is_comment_line() {
        assert_eq!(ping(), b": ping\n\n");
    }
}
