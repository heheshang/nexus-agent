//! SSE 帧编解码 —— **半帧缓冲**。
//!
//! # 这是 P5 的地基
//!
//! 架构文档 §6.1 给 P5 的完成判据是「**SSE 半帧处理有测试**」——
//! 这不是随便挑的：它是 SSE 协议里**唯一**真正会出错的地方。
//!
//! **TCP 分包可能把一帧劈成两半。**
//! - 写侧：必须缓冲半帧，否则写出格式错误的帧。
//! - 读侧：**更危险** —— 收到半帧就解析会得到**截断的 JSON**。
//!
//! 前端的表现是「事件莫名其妙少了几条」—— **极难排查**。
//!
//! # 帧格式（参考架构 §1.5 **已定稿**）
//!
//! ```text
//! event: <event>\n
//! data: <json>\n
//! \n            ← 空行即帧边界
//! ```
//!
//! 心跳是 `: ping\n\n`（注释行）。
//!
//! ⚠️ **逐字节**对齐 —— 前端 `lib/sse.ts` 是**按字节解析**的，
//! 任何格式差异都会表现为「前端收不到事件」。
//!
//! # 为什么自己实现，不用 `eventsource-stream`
//!
//! `eventsource-stream` 是**读侧**（解析上游 SSE）的库 ——
//! `nexus-llm` 已经用了它。本模块是**写侧**（产出 SSE 帧）。
//!
//! 更重要的是：用它做「自己产出自己解析」的回归测试**没有意义** ——
//! 那测的是「crate A 能否解析 crate A 的输出」，
//! 而不是「输出是否符合参考实现的字节格式」。
//!
//! # ⚠️ 附录 C.1 的陷阱：分隔符必须连同帧内容一起切走
//!
//! ```text
//! SSE 帧解码只切走帧内容、不切分隔符 → 死循环
//! ```
//!
//! 分隔符留在缓冲区头部，下一轮 `position()` 又返回 0 —— **无限循环**。
//! 正确做法是 `split_to(pos + DELIM.len())`。

use serde_json::Value;

/// SSE 帧分隔符（LF 变体）。
pub const FRAME_DELIMITER: &[u8] = b"\n\n";

/// SSE 帧分隔符（CRLF 变体）。
///
/// 代理 / 网关可能改写换行 —— 解码器要**容忍**，但**编码器不产出**。
const FRAME_DELIMITER_CRLF: &[u8] = b"\r\n\r\n";

/// 帧内容的大小上限（**默认** 1 MiB）。
///
/// # 为什么需要上限
///
/// `FrameDecoder` 是**增量式**的 —— 喂进来的字节会留在缓冲区里，
/// 直到凑出分隔符为止。
///
/// 如果上游**永远不送分隔符**（流损坏、代理截断，或**恶意**输入），
/// 缓冲区就会**一直增长**，直到进程内存耗尽。
///
/// 1 MiB 是个**宽松**的上限 —— 真实的 SSE 帧（事件名 + JSON 负载）
/// 通常是**几百字节**量级。1 MiB 足以容纳任何合理负载，
/// 同时把「无界增长」变成「有界增长」。
///
/// ⚠️ 上限约束的是**帧内容**（**不含**分隔符）的字节数。
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// 帧解析的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// 帧既无 `event` 也无 `data` —— 畸形帧。
    #[error("帧既无 event 也无 data")]
    MalformedFrame,
    /// 负载不是合法 JSON。
    #[error("帧负载不是合法 JSON: {0}")]
    InvalidPayload(String),
    /// 帧内容**超过**大小上限。
    ///
    /// ⚠️ **不携带缓冲内容** —— 只带 `limit` 与 `actual` 两个**数字**。
    ///
    /// 上游字节是**不可信输入**；把它写进 `Display` 就等于
    /// 让它进入日志 / 响应体。`actual` 是**长度**，
    /// 不是**内容** —— 长度不泄露任何负载。
    #[error("帧大小超过上限：{actual} 字节 > 上限 {limit} 字节")]
    FrameTooLarge { limit: usize, actual: usize },
}

/// 解析后的帧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// 事件名 —— SSE `event:` 字段的值。
    pub event: String,
    /// 数据 —— 多行 `data:` 用 `\n` 连接（SSE 规范）。
    pub data: String,
}

impl Frame {
    /// 把 `data` 解析成 JSON 负载。
    ///
    /// 失败返回 [`FrameError::InvalidPayload`] —— 不静默吞掉。
    pub fn json(&self) -> Result<Value, FrameError> {
        serde_json::from_str(&self.data).map_err(|e| FrameError::InvalidPayload(e.to_string()))
    }
}

/// 把负载编码为 SSE 帧。
///
/// **纯函数** —— 同样的输入得到同样的字节。
///
/// 输出格式：`event: {name}\ndata: {json}\n\n`
///
/// # 为什么**不用** `serde_json::to_string`
///
/// `to_string` 会转义 `/` 之外的某些字符，且**不保证**键序。
/// SSE 帧的 `data:` 内容必须能被前端原样 `JSON.parse` ——
///
/// 用 `to_vec`（紧凑形式）比 `to_string`（可能带换行美化）更可控。
pub fn encode_frame(event: &str, data: &Value) -> Vec<u8> {
    // `serde_json::Value` 的序列化在实践中不可失败（键必为字符串）。
    // 仍然不 panic —— 服务器进程里一个 `expect` 就是一次可用性事故。
    let payload = serde_json::to_vec(data).unwrap_or_else(|_| b"null".to_vec());
    let mut buf = Vec::with_capacity(payload.len() + event.len() + 16);
    buf.extend_from_slice(b"event: ");
    buf.extend_from_slice(event.as_bytes());
    buf.extend_from_slice(b"\ndata: ");
    buf.extend_from_slice(&payload);
    buf.extend_from_slice(FRAME_DELIMITER);
    buf
}

/// 心跳注释帧。
///
/// `: ping\n\n` —— 注释行，前端靠 `:` 开头识别并跳过。
pub fn ping() -> Vec<u8> {
    b": ping\n\n".to_vec()
}

/// SSE 帧解码器（增量式）。
///
/// `push` 返回其中所有**完整**的帧；
/// **不完整的尾部会被缓冲**，等待后续数据。
#[derive(Debug)]
pub struct FrameDecoder {
    buf: Vec<u8>,
    /// 已经扫描过、确认**不含分隔符起点**的字节数。
    ///
    /// 逐字节喂入时，若每次都从 0 重扫，退化是 O(n²)。
    /// 下一次只需从「可能跨次到达的分隔符起点」重扫。
    scanned: usize,
    /// 帧内容的大小上限（字节）—— 不含分隔符。
    limit: usize,
    /// 上限已触发时的错误 —— 解码器进入**不可恢复**状态。
    ///
    /// 一旦超限就**无法重新同步**（不知道下一帧从哪开始），
    /// 所以后续的 `push` **直接**返回同一个错误，**不再缓冲**。
    over_limit: Option<FrameError>,
}

impl Default for FrameDecoder {
    /// ⚠️ **手写** `Default` —— 不能 derive。
    ///
    /// `limit` 的默认值必须是 [`MAX_FRAME_BYTES`]，
    /// 而 `usize` 的 derive 默认是 `0` —— 那会让解码器**拒绝一切**非空帧。
    fn default() -> Self {
        Self {
            buf: Vec::new(),
            scanned: 0,
            limit: MAX_FRAME_BYTES,
            over_limit: None,
        }
    }
}

impl FrameDecoder {
    /// 创建一个空解码器 —— 帧大小上限为 [`MAX_FRAME_BYTES`]。
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建一个**自定义帧大小上限**的解码器。
    ///
    /// # 参数
    ///
    /// `limit` —— 单帧**内容**的字节上限（**不含**分隔符）。
    ///
    /// # 为什么需要它
    ///
    /// 默认值 1 MiB 对绝大多数场景**足够宽松**；
    /// 但「可配置」让上限成为**调用方的选择**而不是**写死的数字** ——
    /// 不同上游的帧大小差别很大。
    ///
    /// ⚠️ 上限约束的是**帧内容**，**不是**缓冲区总长 ——
    /// 一次喂入里可以包含**多个**帧，每个帧各自受上限约束。
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }

    /// 喂入一段字节，返回其中所有**完整**的帧。
    ///
    /// # 错误
    ///
    /// - 遇到**畸形帧**（既无 `event` 又无 `data`）时返回
    ///   [`FrameError::MalformedFrame`]。
    /// - 帧内容**超过**大小上限时返回 [`FrameError::FrameTooLarge`]。
    ///
    /// ⚠️ 此时**已经解码出来的帧会丢失** —— 因为返回类型是 `Result`。
    /// 这是刻意的：畸形帧说明**上游已经坏了**，
    /// 继续读只会产出更多垃圾。调用方应当**终止**这条流。
    ///
    /// # ⚠️ 超限后解码器进入**不可恢复**状态
    ///
    /// 超限时无法知道下一帧的边界在哪（分隔符还没出现）——
    /// **无法重新同步**。所以解码器**释放缓冲区**并记住该错误，
    /// 之后每次 `push` 都**直接**返回同一个错误。
    ///
    /// 这不是「保守」—— 这是**唯一正确**的行为：
    /// 若继续缓冲，攻击者只要持续发送无分隔符字节就能**再次**撑爆内存。
    ///
    /// # ⚠️ 上限约束的是**保留量**，不是**瞬时峰值**
    ///
    /// `push` 会先把整个 chunk 拷进缓冲区，再判断超没超限。
    /// 所以**单次** `push` 传入的巨型 chunk 会让峰值内存短暂达到
    /// `chunk.len() + 旧缓冲`。
    ///
    /// 这**不是**漏洞：调用方本来就**持有**那个 chunk
    /// （它是上游 socket 读出来的字节），我们的拷贝不会让它**超出**一个副本。
    /// 上限防的是「**跨多次 push 累积**」——那才是无界的。
    /// 逐字节/小块喂入时峰值就是 `limit` 本身。
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        // ⚠️ **先**检查是否已超限 —— 已超限时**不再复制**任何字节进来。
        if let Some(err) = &self.over_limit {
            return Err(err.clone());
        }

        self.buf.extend_from_slice(chunk);

        let mut frames = Vec::new();
        // 只从「上次没扫过」的位置开始找 —— 否则逐字节喂入时
        // 每次都要重扫整个缓冲，退化为 O(n²)。
        let mut scan_from = self.scanned;

        loop {
            let Some((pos, delim_len)) = find_delimiter(&self.buf[scan_from..]) else {
                // 没有分隔符：整个缓冲区就是**一个未完成的帧**。
                // ⚠️ 它的长度就是该帧内容的**最终长度下界** ——
                // 已经超限就不可能再变小。
                if self.buf.len() > self.limit {
                    return Err(self.exceeded(self.buf.len()));
                }
                // 下次只需回退「可能跨次到达的最长分隔符」长度 - 1。
                // 最长的是 CRLF（4 字节），回退 3 字节即可覆盖跨界起点。
                self.scanned = self
                    .buf
                    .len()
                    .saturating_sub(FRAME_DELIMITER_CRLF.len() - 1);
                return Ok(frames);
            };
            let pos = scan_from + pos;

            // ⚠️ 帧内容长度 = 分隔符的**绝对**位置 ——
            // 缓冲区每次都从**帧首**切走，所以 `pos` 就是内容字节数。
            //
            // ⚠️ 必须在**切走之前**检查 —— 否则超大的帧会先被
            // 完整地搬进 `raw`（又一次拷贝），才发现超限。
            if pos > self.limit {
                return Err(self.exceeded(pos));
            }

            // ⚠️ 必须把分隔符**连同帧内容一起**切走（**一次** drain ——
            // 两次 drain 会把尾部字节搬移两遍）。
            // 只切内容会把分隔符留在缓冲区头部，
            // 下一轮 find_delimiter() 又返回 0 → **死循环**。
            let mut raw: Vec<u8> = self.buf.drain(..pos + delim_len).collect();
            raw.truncate(pos);

            // 剩余缓冲区从头开始扫描（序号已变）。
            scan_from = 0;

            // 纯注释帧（心跳）被**跳过** —— 不是错误。
            let Some(frame) = parse_frame(&raw)? else {
                continue;
            };
            frames.push(frame);
        }
    }
    /// 当前缓冲的**半帧**字节数。
    ///
    /// 测试要能断言「半帧被**缓冲**」而不是「半帧被**丢弃**」。
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    /// 帧内容的大小上限（字节）。
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// 记录超限并**释放**缓冲区。
    ///
    /// # 为什么释放
    ///
    /// 解码器已经**不可恢复** —— 保留这堆字节既没用，
    /// 又占着内存（那正是我们要防的）。
    fn exceeded(&mut self, actual: usize) -> FrameError {
        let err = FrameError::FrameTooLarge {
            limit: self.limit,
            actual,
        };
        self.over_limit = Some(err.clone());
        // ⚠️ `Vec::new()` 而**不是** `clear()` ——
        // `clear()` 保留**容量**（那 1 MiB 还在），
        // 而解码器已经不会复用这块内存了。
        self.buf = Vec::new();
        self.scanned = 0;
        err
    }
}

/// 找出缓冲区里**最早**的分隔符。
///
/// 返回 `(内容长度, 分隔符长度)`。
///
/// 两种分隔符都要认 —— 但**要取更早的那个**。
///
/// # 为什么是**单次扫描**
///
/// 分成两次 `.position()`（LF 一次、CRLF 一次）时，
/// 若缓冲区里根本没有 CRLF，CRLF 那次会扫到缓冲区末尾 ——
/// 多帧连续解码时就是 O(帧数 × 缓冲区大小)。
/// 单次扫描在找到**最早**匹配时立即返回，配合调用方的扫描游标是线性的。
///
/// LF 以 `\n` 开头、CRLF 以 `\r` 开头，两者不可能起点相同，
/// 所以「先到先得」不会产生歧义。
fn find_delimiter(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, FRAME_DELIMITER.len()));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, FRAME_DELIMITER_CRLF.len()));
        }
        i += 1;
    }
    None
}

/// 解析一帧的**内容**（不含分隔符）。
///
/// # 返回
///
/// - `Ok(Some(frame))` —— 正常帧。
/// - `Ok(None)` —— **纯注释帧**（心跳）—— 被跳过，不是错误。
/// - `Err(MalformedFrame)` —— 既无 `event` 又无 `data` 的**非注释**帧。
///
/// # 规则
///
/// - 注释行（以 `:` 开头）被**忽略** —— 心跳就是这样。
/// - `event:` 后的**一个**空格被跳过。
/// - `data:` 后的**一个**空格被跳过。
/// - 多行 `data:` 用 `\n` 连接（SSE 规范）。
/// - 既无 `event` 又无 `data` → [`FrameError::MalformedFrame`]。
///
/// # ⚠️ 为什么注释帧是 `None` 而不是错误
///
/// 心跳（`: ping\n\n`）**只有**注释行 —— 它既无 `event` 也无 `data`。
/// 如果把它判成畸形帧，每隔一次心跳就会**中断整条流**。
///
/// 这是本模块实现时**真实踩到**的坑：初版把「无 event 无 data」一律判错，
/// 结果心跳帧让 `push` 返回 `Err` —— 心跳是**保活**机制，
/// 让它打断连接是**完全相反**的效果。
pub fn parse_frame(raw: &[u8]) -> Result<Option<Frame>, FrameError> {
    let text = String::from_utf8_lossy(raw);
    let mut event = String::new();
    let mut data_lines: Vec<&str> = Vec::new();
    let mut has_content = false;

    for line in text.lines() {
        // 注释行（心跳）忽略
        if line.starts_with(':') {
            continue;
        }
        // ⚠️ 只有**非注释**行才算「有内容」——
        // 否则纯注释帧会被判成畸形。
        has_content = true;
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.strip_prefix(' ').unwrap_or(rest).to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }

    if text.trim().is_empty() {
        // ⚠️ **真空帧**（分隔符紧跟分隔符）—— 不是注释帧。
        //
        // 必须与「纯注释帧」区分开：前者是**畸形**，
        // 后者是**心跳**（合法，被跳过）。
        return Err(FrameError::MalformedFrame);
    }

    if !has_content {
        // 纯注释帧（心跳）—— 跳过，不是错误。
        return Ok(None);
    }

    if event.is_empty() && data_lines.is_empty() {
        return Err(FrameError::MalformedFrame);
    }

    Ok(Some(Frame {
        event,
        // SSE 规范：多行 data 用 \n 连接
        data: data_lines.join("\n"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_encoded_frame_matches_the_reference_bytes() {
        // ⚠️ 断言用**字面量**，不是引用被测代码里的常量 ——
        // 否则就是同义反复（NEX-25 的教训）。
        let out = encode_frame("delta", &json!({"content": "hi"}));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "event: delta\ndata: {\"content\":\"hi\"}\n\n"
        );
    }

    #[test]
    fn the_encoder_is_pure() {
        let a = encode_frame("done", &json!({"finish_reason": "stop"}));
        let b = encode_frame("done", &json!({"finish_reason": "stop"}));
        assert_eq!(a, b);
    }

    #[test]
    fn a_partial_frame_is_buffered_not_emitted() {
        let single = encode_frame("delta", &json!({"x": 1}));
        let mut d = FrameDecoder::new();

        let got = d.push(&single[..single.len() - 3]).unwrap();
        assert!(got.is_empty(), "不完整的帧不应产出帧");
        assert!(d.buffered() > 0, "半帧应被缓冲，而不是丢弃");
    }

    #[test]
    fn a_byte_by_byte_feed_reassembles_the_frame() {
        let multi = [
            encode_frame("delta", &json!({"c": 1})),
            encode_frame("done", &json!({"c": 2})),
        ];
        let total: Vec<u8> = multi.iter().flatten().copied().collect();

        let mut d = FrameDecoder::new();
        let mut frames = Vec::new();
        for b in &total {
            frames.extend(d.push(&[*b]).unwrap());
        }
        assert_eq!(frames.len(), 2, "逐字节喂入不应丢帧");
        assert_eq!(d.buffered(), 0, "缓冲区应清空");
        assert_eq!(frames[0].event, "delta");
        assert_eq!(frames[1].event, "done");
    }

    #[test]
    fn every_split_point_reassembles_identically() {
        let single = encode_frame("delta", &json!({"content": "hello world"}));
        for split in 1..single.len() {
            let mut d = FrameDecoder::new();
            let mut got = d.push(&single[..split]).unwrap();
            got.extend(d.push(&single[split..]).unwrap());

            assert_eq!(got.len(), 1, "split={split} 帧数不对");
            assert_eq!(got[0].event, "delta", "split={split}");
            assert_eq!(got[0].data, r#"{"content":"hello world"}"#, "split={split}");
        }
    }

    #[test]
    fn consecutive_frames_leave_the_buffer_empty() {
        let mut d = FrameDecoder::new();
        let mut buf = Vec::new();
        for i in 0..5 {
            buf.extend(encode_frame("delta", &json!({"i": i})));
        }
        let frames = d.push(&buf).unwrap();
        assert_eq!(frames.len(), 5);
        assert_eq!(
            d.buffered(),
            0,
            "缓冲区应清空 —— 不能残留分隔符（否则死循环）"
        );
    }

    #[test]
    fn a_malformed_frame_is_rejected() {
        let mut d = FrameDecoder::new();
        // 既无 event 又无 data（**非注释**行）
        let err = d.push(b"foo: bar\n\n").unwrap_err();
        assert_eq!(err, FrameError::MalformedFrame);

        // 空帧
        let mut d2 = FrameDecoder::new();
        assert_eq!(d2.push(b"\n\n").unwrap_err(), FrameError::MalformedFrame);
    }

    #[test]
    fn a_crlf_variant_is_accepted() {
        let mut d = FrameDecoder::new();
        let frames = d.push(b"event: delta\r\ndata: {\"x\":1}\r\n\r\n").unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "delta");
        assert_eq!(frames[0].data, r#"{"x":1}"#);
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn multiline_data_is_joined_with_a_newline() {
        let raw = b"event: delta\ndata: line1\ndata: line2";
        let f = parse_frame(raw).unwrap().unwrap();
        assert_eq!(f.data, "line1\nline2");
    }

    #[test]
    fn a_comment_line_is_ignored() {
        let raw = b": ping\nevent: delta\ndata: {}";
        let f = parse_frame(raw).unwrap().unwrap();
        assert_eq!(f.event, "delta");
        assert_eq!(f.data, "{}");
    }

    #[test]
    fn the_heartbeat_is_a_comment_frame() {
        assert_eq!(ping(), b": ping\n\n");
        // 心跳被解码器忽略 —— 不产生帧
        let mut d = FrameDecoder::new();
        assert!(d.push(&ping()).unwrap().is_empty());
    }

    #[test]
    fn a_frame_round_trips_through_json() {
        let f = parse_frame(b"event: delta\ndata: {\"content\":\"hi\"}")
            .unwrap()
            .unwrap();
        assert_eq!(f.json().unwrap(), json!({"content": "hi"}));
    }

    #[test]
    fn an_invalid_json_payload_is_reported() {
        let f = parse_frame(b"event: delta\ndata: {not json")
            .unwrap()
            .unwrap();
        assert!(matches!(f.json(), Err(FrameError::InvalidPayload(_))));
    }

    #[test]
    fn a_data_only_frame_is_accepted() {
        // SSE 规范允许无 event 的 data-only 帧
        let f = parse_frame(b"data: {\"x\":1}").unwrap().unwrap();
        assert_eq!(f.event, "");
        assert_eq!(f.data, r#"{"x":1}"#);
    }

    // ── 帧大小上限（NEX-35） ──

    #[test]
    fn the_default_limit_is_one_mebibyte() {
        assert_eq!(MAX_FRAME_BYTES, 1024 * 1024);
        assert_eq!(FrameDecoder::new().limit(), MAX_FRAME_BYTES);
    }

    #[test]
    fn an_oversized_frame_is_rejected_without_panicking() {
        // 用**小上限**而不是真的 1 MiB —— 行为与上限大小无关，
        // 但测试跑得快且不占内存。
        let mut d = FrameDecoder::with_limit(64);

        // 65 字节的内容（**不含**分隔符）+ 分隔符 → 内容**超过**上限。
        //
        // ⚠️ 内容全是 `x`（既无 event 也无 data）—— 这**不是**为了让帧畸形，
        // 而是要保证**先**触发大小检查：畸形检查在**切走**之后才做，
        // 所以超限帧**永远**报 `FrameTooLarge`，不会报 `MalformedFrame`。
        let mut chunk = vec![b'x'; 65];
        chunk.extend_from_slice(b"\n\n");

        let err = d.push(&chunk).unwrap_err();
        assert_eq!(
            err,
            FrameError::FrameTooLarge {
                limit: 64,
                actual: 65,
            }
        );
    }

    #[test]
    fn a_frame_exactly_at_the_limit_is_decoded() {
        // 边界用例：内容**恰好等于**上限 → 正常解码。
        //
        // ⚠️ 这条抓的是「用 `>=` 而不是 `>`」的 off-by-one。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = b"event: delta\n".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 64 - chunk.len()));
        chunk.extend_from_slice(b"\n\n");

        let frames = d.push(&chunk).unwrap();
        assert_eq!(frames.len(), 1, "恰好等于上限的帧应能解码");
        assert_eq!(frames[0].event, "delta");
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn a_frame_one_byte_over_the_limit_is_rejected() {
        // 边界用例：内容比上限多 1 字节 → 拒绝。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = b"event: delta\n".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 65 - chunk.len()));
        chunk.extend_from_slice(b"\n\n");

        let err = d.push(&chunk).unwrap_err();
        assert_eq!(
            err,
            FrameError::FrameTooLarge {
                limit: 64,
                actual: 65,
            }
        );
    }

    #[test]
    fn an_oversized_frame_without_a_delimiter_is_rejected() {
        // ⚠️ **没有分隔符**的持续输入 —— 这才是真正的攻击形态。
        // 分隔符永远不到达 → 缓冲区一直增长。
        let mut d = FrameDecoder::with_limit(64);

        let err = d.push(&[b'x'; 65]).unwrap_err();
        assert_eq!(
            err,
            FrameError::FrameTooLarge {
                limit: 64,
                actual: 65,
            }
        );
    }

    #[test]
    fn the_buffer_stops_growing_after_the_limit_is_exceeded() {
        // 超限后**不再继续增长** —— 这是加固的**核心目的**。
        let mut d = FrameDecoder::with_limit(64);

        let first = d.push(&[b'x'; 65]).unwrap_err();
        let after = d.buffered();

        // 继续喂入 —— 缓冲区**不得**增长
        for _ in 0..10 {
            let err = d.push(&[b'y'; 100]).unwrap_err();
            // ⚠️ 必须断言**同一个**错误 —— 只断言「长度不变」抓不住
            // 「超限后继续缓冲」的错误实现（那种实现会重新检查大小，
            // 于是缓冲区又被清空，长度「看起来」没长）。
            assert_eq!(err, first, "超限后应返回同一个错误");
            assert_eq!(d.buffered(), after, "超限后缓冲区不得继续增长");
        }
    }

    #[test]
    fn the_limit_counts_frame_content_not_the_delimiter() {
        // ⚠️ 上限约束的是**帧内容**（**不含**分隔符）。
        //
        // 这条抓的是「把分隔符也算进帧长」的错误实现 ——
        // 那样一个**恰好**等于上限的帧会因为多出 2 字节而被误杀。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = b"event: delta\n".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 64 - chunk.len()));
        chunk.extend_from_slice(b"\n\n");

        // 内容 64 字节 + 分隔符 2 字节 = 66 字节总长
        assert_eq!(chunk.len(), 66, "本测试需要总长超过上限");

        let frames = d.push(&chunk).unwrap();
        assert_eq!(frames.len(), 1, "内容等于上限时应能解码");
    }

    #[test]
    fn the_decoder_releases_its_buffer_after_exceeding_the_limit() {
        // ⚠️ 断言的是**容量**，不只是**长度**。
        //
        // `Vec::clear()` 只把长度归零 —— **容量**还在（那 1 MiB 仍占着内存）。
        // 解码器已经**不可恢复**，这块内存**不会被复用** → 必须真正释放。
        //
        // 这条与 `the_buffer_stops_growing_after_the_limit_is_exceeded`
        // 抓的是**不同**的东西：那条抓「长度」，这条抓「容量」。
        let mut d = FrameDecoder::with_limit(64);

        assert!(d.push(&[b'x'; 65]).is_err());
        assert_eq!(d.buffered(), 0, "超限后缓冲区长度应归零");
        assert_eq!(
            d.buf.capacity(),
            0,
            "超限后应**释放**容量（clear() 只归零长度，容量仍占着内存）"
        );
    }

    #[test]
    fn the_limit_applies_to_the_crlf_delimiter_variant() {
        // 上限对 **CRLF** 同样生效 —— 代理改写换行不能绕过限制。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = b"event: delta\r\n".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 65 - chunk.len()));
        chunk.extend_from_slice(b"\r\n\r\n");

        let err = d.push(&chunk).unwrap_err();
        assert_eq!(
            err,
            FrameError::FrameTooLarge {
                limit: 64,
                actual: 65,
            }
        );
    }

    #[test]
    fn the_limit_is_applied_per_frame_not_per_chunk() {
        // ⚠️ 一次喂入里可以包含**多个**帧 —— 上限约束的是**单帧**，
        // 不是整个 chunk。
        //
        // 这条抓的是「拿 `chunk.len()` 当帧长」的错误实现。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = Vec::new();
        for _ in 0..5 {
            chunk.extend_from_slice(b"event: delta\ndata: {}\n\n");
        }
        assert!(chunk.len() > 64, "本测试需要 chunk 本身超过上限");

        let frames = d.push(&chunk).unwrap();
        assert_eq!(frames.len(), 5, "多个小帧应全部解码");
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn the_too_large_error_carries_no_buffered_content() {
        // 上游字节是**不可信输入** —— 错误消息不能把它带出去。
        //
        // ⚠️ 这条抓的是「把缓冲内容塞进 `Display`」的错误实现 ——
        // 那会让上游负载进入日志 / 响应体。
        const SECRET: &str = "SUPER_SECRET_UPSTREAM_PAYLOAD";
        let mut d = FrameDecoder::with_limit(64);

        // 反复塞入同一段秘密，直到超过上限
        let mut chunk = Vec::new();
        while chunk.len() <= 64 {
            chunk.extend_from_slice(SECRET.as_bytes());
        }

        let err = d.push(&chunk).unwrap_err();
        let msg = err.to_string();

        assert!(!msg.contains(SECRET), "错误消息不得包含缓冲内容：{msg}");
        // 也不得包含**任何**负载片段
        assert!(!msg.contains("SUPER"), "错误消息不得包含负载片段：{msg}");
        // ⚠️ 但**长度**必须出现 —— 那是诊断必需的，且不泄露内容
        assert!(msg.contains("64"), "错误消息应包含上限：{msg}");
    }

    #[test]
    fn the_decoder_stays_broken_after_exceeding_the_limit() {
        // 超限后解码器**不可恢复** —— 后续 push 返回**同一个**错误。
        //
        // ⚠️ 这条抓的是「超限后清空缓冲、继续正常解析」的错误实现 ——
        // 那会让攻击者能**反复**把内存撑到上限（DoS 放大）。
        let mut d = FrameDecoder::with_limit(64);

        let first = d.push(&[b'x'; 65]).unwrap_err();

        // ⚠️ 喂一个**合法**的小帧 —— 若解码器已复位，它会**成功**解码
        // （返回 `Ok`），而不是报错。
        //
        // 只断言「报错」不够：复位后的实现**也会**报错
        // （只是 `actual` 不同）。所以必须断言**错误内容相同**。
        let second = d.push(b"event: delta\ndata: {}\n\n").unwrap_err();

        assert_eq!(first, second, "超限后应返回同一个错误");
        assert_eq!(
            first,
            FrameError::FrameTooLarge {
                limit: 64,
                actual: 65,
            },
            "错误应保留**首次**超限时的大小（复位后会变成别的 actual）"
        );
        assert_eq!(d.buffered(), 0, "超限后缓冲区应已释放");
    }

    #[test]
    fn an_oversized_frame_after_a_normal_frame_is_rejected() {
        // 正常帧 + 超大帧（**同一个 chunk**）→ 正常帧产出，
        // 超大帧报错。
        //
        // ⚠️ 这条抓的是「拿**整个缓冲区**长度当帧长」的错误实现 ——
        // 那会让**正常帧**被误杀（缓冲区里还有超大帧）。
        //
        // ⚠️ 必须断言 **`actual` 是 65**（超大帧**自己**的内容长度），
        // 而**不是**整个 chunk 的长度（87）——
        // 只断言「报错了」抓不住这个变异（两种实现都报错）。
        let mut d = FrameDecoder::with_limit(64);

        let mut chunk = b"event: ok\ndata: {}\n\n".to_vec();
        chunk.extend(std::iter::repeat_n(b'x', 65));
        chunk.extend_from_slice(b"\n\n");

        let err = d.push(&chunk).unwrap_err();
        assert_eq!(
            err,
            FrameError::FrameTooLarge {
                limit: 64,
                // ⚠️ 超大帧**自己**的内容长度 —— 不是整个 chunk 的长度
                actual: 65,
            },
            "应报告**超大帧**自己的长度（正常帧不应被牵连）"
        );
    }
}
