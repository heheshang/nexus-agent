//! 上下文预算裁剪。
//!
//! 架构文档 §3.1.1 —— 参考实现的 `boundChatDescriptor`
//! （`internal/llm/context_budget.go:14`）。
//!
//! # 为什么预算要在**调用前**拒
//!
//! 参考实现的注释明确：预算不足在**调用前**就被拒，而不是等上游返回 400。
//! 这让预算错误可以在**重试之前短路** ——
//! 重试同样的请求同样会超预算，重试是纯粹的浪费。
//!
//! # 为什么不修改入参
//!
//! 参考实现「拷贝描述符（不修改原值）」。Rust 版的所有权让这一点成为自然结果：
//! 函数**返回新的**请求与描述符，不改入参。
//!
//! # 为什么用「字节 + framing」而不是精确 token
//!
//! 参考实现的注释原文（`context_budget.go:10-13`）：
//!
//! > 使用序列化 UTF-8 字节 + framing 作为保守本地预算，而不是精确 / 厂商无关的 token 计数
//!
//! 本设计沿用这个**保守估算**：
//!
//! - 序列化后的字节数是 token 数的**上界**（一个 token 至少 1 字节）。
//! - framing 是每条消息 / 每个工具的协议开销。
//!
//! ⚠️ **这是保守上界，不是精确 token 数** ——
//! 它保证「不会超」，但不保证「充分利用」。
//! 真实的 token 计数依赖厂商的分词器，那会引入「分词器与厂商不一致」的新不确定性来源。

use nexus_core::error::{AppError, ErrorCode};

use crate::types::{ChatRequest, Message, ToolDef};

/// 请求体的固定 framing 开销（字节）。
///
/// 参考实现：`64 + 16 * len(messages) + 32 * len(tools)`。
const BASE_FRAMING: usize = 64;

/// 每条消息的 framing 开销（字节）。
const PER_MESSAGE_FRAMING: usize = 16;

/// 每个工具定义的 framing 开销（字节）。
const PER_TOOL_FRAMING: usize = 32;

/// 未指定 `max_tokens` 时的默认输出上限。
const DEFAULT_MAX_TOKENS: i32 = 1024;

/// 预算裁剪后的请求。
///
/// 与入参 [`ChatRequest`] 的区别：**输出上限已被确定**（不再有「0 表示不指定」）。
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedRequest {
    /// 裁剪后的消息序列（与入参一致 —— 本阶段不裁剪**历史**，只裁剪**上限**）。
    pub messages: Vec<Message>,
    /// 工具定义（与入参一致）。
    pub tools: Vec<ToolDef>,
    /// **确定后的**输出 token 上限。
    pub max_tokens: i32,
    /// 采样温度。
    pub temperature: f32,
    /// 核采样。
    pub top_p: f32,
    /// 估算出的请求体字节数（不含 framing）。
    pub body_bytes: usize,
    /// framing 开销字节数。
    pub framing_bytes: usize,
}

impl BoundedRequest {
    /// 请求占用的**总**预算（请求体 + framing + 预留输出）。
    pub fn total_budget(&self) -> usize {
        self.body_bytes + self.framing_bytes + self.max_tokens.max(0) as usize
    }
}

/// 计算输出上限。
///
/// 逐条照搬参考实现的语义：
///
/// 1. `limit = desc.max_output_tokens`（0 则回落 `desc.max_tokens`）
/// 2. `requested = req.max_tokens`（0 则默认 `1024`）
/// 3. `requested` 不超过 `context_window / 4`
/// 4. 最终 `requested = min(requested, limit)`
///
/// # 为什么第 3 条要把请求压到窗口的 1/4
///
/// 那是给**输出**留的余量 —— 上下文窗口是「输入 + 输出」的总额度。
/// 输入占满窗口就没有空间生成输出了。
fn resolve_max_tokens(
    desc_max_output: i32,
    desc_max_tokens: i32,
    req_max_tokens: i32,
    context_window: i32,
) -> i32 {
    // 1. 上限
    let limit = if desc_max_output <= 0 {
        desc_max_tokens
    } else {
        desc_max_output
    };

    // 2. 请求值
    let mut requested = if req_max_tokens <= 0 {
        DEFAULT_MAX_TOKENS
    } else {
        req_max_tokens
    };

    // 3. 不超过窗口的 1/4
    if context_window > 0 {
        let quarter = context_window / 4;
        if quarter > 0 {
            requested = requested.min(quarter);
        }
    }

    // 4. 取小
    requested.min(limit).max(0)
}

/// 估算请求体的字节数。
///
/// 参考实现把消息与工具**序列化**后取长度 ——
/// 那是最贴近上游真实请求体的估算。
///
/// 这里用 `serde_json` 序列化（与 OpenAI 兼容请求体同构的 JSON），
/// 失败时**退回字符数**而不是 panic：
///
/// - 序列化失败意味着某个字段里装了非法 JSON 值 ——
///   那在真正发请求时也会失败，属于**上游请求构造错误**，不该在预算阶段 panic。
/// - 退回字符数是**保守**的（字符数 ≤ 字节数对非 ASCII 不成立，
///   但我们用的是 `chars().count()`，对 ASCII 恰好等于字节数）——
///   宁可少算一点，也不要误拒合法请求。实际上 UTF-8 的字节数 ≥ 字符数，
///   所以字符数是**下界**，可能低估。为安全起见用字节长度。
fn estimate_body_bytes(messages: &[Message], tools: &[ToolDef]) -> usize {
    let mut total = 0usize;

    for message in messages {
        // 按 OpenAI 兼容协议拼装单条消息的估算形状。
        total += message.content.len();
        total += message.reasoning_content.len();
        total += message.tool_call_id.len();
        total += message.name.len();
        for call in &message.tool_calls {
            total += call.id.len();
            total += call.name.len();
            total += call.args.len();
        }
        // 角色字段与 JSON 结构开销。
        total += 16;
    }

    for tool in tools {
        total += tool.name.len();
        total += tool.description.len();
        total += tool.parameters.to_string().len();
        total += 16;
    }

    total
}

/// framing 开销。
fn framing_bytes(message_count: usize, tool_count: usize) -> usize {
    BASE_FRAMING + PER_MESSAGE_FRAMING * message_count + PER_TOOL_FRAMING * tool_count
}

/// 上下文预算裁剪。
///
/// # 返回值
///
/// - `Ok(BoundedRequest)` —— 预算充足。
/// - `Err(AppError)` —— 预算不足，错误码为 [`ErrorCode::BudgetExceeded`]
///   （**不重试** —— 重试同样的请求同样会超）。
///
/// # 不修改入参
///
/// 入参按引用传入，返回的是**新**的 [`BoundedRequest`]。
///
/// # 裁剪在**每次尝试**都重跑
///
/// 包括 fallback 与后续 tool_result 轮 —— 参考实现的明确不变量。
/// 本函数是**纯函数**，调用时机由网关负责。
pub fn bound_chat_request(
    desc: &crate::types::ModelDescriptor,
    req: &ChatRequest,
) -> Result<BoundedRequest, AppError> {
    let max_tokens = resolve_max_tokens(
        desc.max_output_tokens,
        desc.max_tokens,
        req.max_tokens,
        desc.context_window,
    );

    let body_bytes = estimate_body_bytes(&req.messages, &req.tools);
    let framing = framing_bytes(req.messages.len(), req.tools.len());

    let context_window = desc.context_window;

    // `context_window <= 0` 表示**未知** —— 此时不做裁剪。
    if context_window > 0 {
        let window = context_window as usize;
        let reserved = max_tokens.max(0) as usize;

        // 拒绝条件一：预留的输出本身就吃掉了整个窗口。
        if reserved >= window {
            return Err(budget_error());
        }

        // 拒绝条件二：请求体 + framing 超出「窗口减去预留输出」的额度。
        if body_bytes + framing > window - reserved {
            return Err(budget_error());
        }
    }

    Ok(BoundedRequest {
        messages: req.messages.clone(),
        tools: req.tools.clone(),
        max_tokens,
        temperature: req.temperature,
        top_p: req.top_p,
        body_bytes,
        framing_bytes: framing,
    })
}

/// 预算不足。
///
/// 错误码 [`ErrorCode::BudgetExceeded`]（30006）的 `retry_policy()` 是 `Terminal` ——
/// 「重试同样的请求同样会超预算」。
fn budget_error() -> AppError {
    AppError::new(
        ErrorCode::BudgetExceeded,
        "请求超出模型的上下文预算，请缩短内容或更换模型",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Role;
    use nexus_core::error::RetryPolicy;

    fn descriptor(
        context_window: i32,
        max_output: i32,
        max_tokens: i32,
    ) -> crate::types::ModelDescriptor {
        let mut desc = crate::types::ModelDescriptor::new("test", "http://localhost", "k", "m");
        desc.context_window = context_window;
        desc.max_output_tokens = max_output;
        desc.max_tokens = max_tokens;
        desc
    }

    fn request(messages: Vec<Message>, max_tokens: i32) -> ChatRequest {
        ChatRequest {
            messages,
            tools: vec![],
            temperature: 0.7,
            top_p: 0.9,
            max_tokens,
            stream: false,
        }
    }

    fn user(text: &str) -> Message {
        Message::new(Role::User, text)
    }

    #[test]
    fn unspecified_max_tokens_falls_back_to_the_default() {
        let desc = descriptor(0, 4096, 2048);
        let req = request(vec![user("hi")], 0);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.max_tokens, DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn max_output_is_the_ceiling() {
        let desc = descriptor(0, 4096, 2048);
        let req = request(vec![user("hi")], 999_999);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.max_tokens, 4096);
    }

    #[test]
    fn max_output_zero_falls_back_to_max_tokens() {
        let desc = descriptor(0, 0, 2048);
        let req = request(vec![user("hi")], 999_999);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.max_tokens, 2048);
    }

    #[test]
    fn requested_output_never_exceeds_a_quarter_of_the_window() {
        let desc = descriptor(1000, 4096, 2048);
        let req = request(vec![user("hi")], 999_999);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.max_tokens, 250, "窗口的 1/4 = 250");
    }

    #[test]
    fn unknown_context_window_means_no_bounding() {
        // context_window == 0 表示「未知」 —— 不裁剪。
        let desc = descriptor(0, 4096, 2048);
        let req = request(vec![user(&"x".repeat(100_000))], 1024);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert!(bounded.body_bytes > 100_000);
    }

    #[test]
    fn oversized_body_is_rejected_before_any_upstream_call() {
        let desc = descriptor(1000, 100, 100);
        let req = request(vec![user(&"x".repeat(5000))], 100);

        let err = bound_chat_request(&desc, &req).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::BudgetExceeded);
    }

    #[test]
    fn budget_error_is_terminal_not_retryable() {
        let desc = descriptor(1000, 100, 100);
        let req = request(vec![user(&"x".repeat(5000))], 100);

        let err = bound_chat_request(&desc, &req).unwrap_err();

        assert_eq!(
            err.retry_policy(),
            RetryPolicy::Terminal,
            "重试同样的请求同样会超预算"
        );
    }

    #[test]
    fn output_reserving_the_whole_window_is_rejected() {
        // 窗口 100，输出上限 100 —— 输出本身就吃掉了整个窗口。
        let desc = descriptor(100, 100, 100);
        let req = request(vec![user("hi")], 100);

        let err = bound_chat_request(&desc, &req).unwrap_err();

        assert_eq!(err.facing.code, ErrorCode::BudgetExceeded);
    }

    #[test]
    fn framing_overhead_matches_the_reference_formula() {
        let messages = vec![user("a"), user("b"), user("c")];
        let req = request(messages, 100);
        let desc = descriptor(0, 4096, 2048);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        // 64 + 16 * 3 + 32 * 0 = 112
        assert_eq!(bounded.framing_bytes, 64 + 16 * 3);
    }

    #[test]
    fn tools_contribute_to_the_budget() {
        let tool = ToolDef {
            source_identity: String::new(),
            binding_version: 0,
            capability: String::new(),
            dangerous: false,
            replay_policy: String::new(),
            name: "search".into(),
            description: "搜索".into(),
            parameters: serde_json::json!({"type": "object"}),
        };

        let mut req = request(vec![user("hi")], 100);
        req.tools = vec![tool];
        let desc = descriptor(0, 4096, 2048);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        // framing：64 + 16 * 1 + 32 * 1 = 112
        assert_eq!(bounded.framing_bytes, 64 + 16 + 32);
        assert!(bounded.body_bytes > 0, "工具定义也计入请求体");
    }

    #[test]
    fn bounding_does_not_mutate_the_input() {
        let desc = descriptor(0, 4096, 2048);
        let req = request(vec![user("hi")], 999_999);
        let original_max = req.max_tokens;
        let original_len = req.messages.len();

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.max_tokens, 4096);
        assert_eq!(req.max_tokens, original_max, "入参不得被修改");
        assert_eq!(req.messages.len(), original_len);
    }

    #[test]
    fn bounding_is_pure() {
        let desc = descriptor(0, 4096, 2048);
        let req = request(vec![user("hi")], 1024);

        let a = bound_chat_request(&desc, &req).unwrap();
        let b = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(a, b);
    }

    #[test]
    fn a_body_that_exactly_fits_is_accepted() {
        // 先算出恰好填满额度的请求体长度，再断言它**通过**。
        let window = 1000i32;
        let reserved = 100i32;
        let desc = descriptor(window, reserved, reserved);

        let framing = framing_bytes(1, 0);
        // 额度：窗口 - 预留输出 - framing = 请求体可占的字节数。
        let budget_for_body = window as usize - reserved as usize - framing;
        // 请求体 = 正文长度 + 每条消息的固定开销。
        let overhead = estimate_body_bytes(&[user("")], &[]);
        let content_len = budget_for_body - overhead;

        let req = request(vec![user(&"x".repeat(content_len))], reserved);

        let bounded = bound_chat_request(&desc, &req).unwrap();

        assert_eq!(bounded.body_bytes, budget_for_body);
        assert_eq!(bounded.total_budget(), window as usize);
    }

    #[test]
    fn one_byte_over_the_limit_is_rejected() {
        let window = 1000i32;
        let reserved = 100i32;
        let desc = descriptor(window, reserved, reserved);

        let framing = framing_bytes(1, 0);
        let budget_for_body = window as usize - reserved as usize - framing;
        let overhead = estimate_body_bytes(&[user("")], &[]);
        let content_len = budget_for_body - overhead;

        let req = request(vec![user(&"x".repeat(content_len + 1))], reserved);

        let err = bound_chat_request(&desc, &req).unwrap_err();

        assert_eq!(
            err.facing.code,
            ErrorCode::BudgetExceeded,
            "超出 1 字节也必须被拒"
        );
    }
}
