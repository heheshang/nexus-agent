//! 上下文历史的整轮淘汰。
//!
//! 架构文档 §3.1.1；参考架构 §3.2.1（`compact_input_history`）与 §3.2.2。
//!
//! # 决策 D5：整轮淘汰，**不**生成摘要
//!
//! 超预算时按**整轮**删除历史，保留 system 消息、首个 user 之前的前缀、
//! 最新一轮 user。**不**生成摘要。
//!
//! 三条依据（详见 `docs/specs/p4/spec.md` §6.2）：
//!
//! 1. **可验证性** —— 整轮淘汰的每条不变量都能写成二元断言；
//!    摘要的正确性**在原理上无法断言**。
//! 2. **事实漂移** —— 摘要错了，模型就基于错误事实推理。
//! 3. **注入面** —— 工具输出是**不可信输入**；摘要把「被丢弃的数据」
//!    变成「被保留的上下文」，是注入面的**放大**。
//!
//! # 为什么是「整轮」而不是「按条」
//!
//! 按条裁剪会留下**孤立**的 `tool_result` —— 一个没有对应 `tool_call`
//! 的工具结果。那直接**破坏模型协议**：模型看到一条无主的工具结果，
//! 无法判断它对应哪次调用。
//!
//! 参考架构 §2.2 的取舍说明原文：
//! 「它比『按条裁剪』正确得多 —— 按条裁剪会留下孤立 tool_result，
//! 直接破坏模型协议」。
//!
//! 一轮的完整定义：一个 user 消息 + 其后的 assistant 消息 +
//! **所有**并行 tool 结果。
//!
//! # 协议完整性**先于**裁剪
//!
//! 参考架构 §3.2.2 的注释原文：
//! 「Validate the entire protocol before any history can be discarded」——
//! 理由是**错误信息必须指向真正的原因**。
//! 先裁剪会让配对断裂被**掩盖**，报出来的是「预算不足」而非「协议非法」。
//!
//! 所以本模块**先**调用 [`crate::protocol::validate_messages`]，
//! **再**做预算估算与裁剪。
//!
//! # 预算估算的口径
//!
//! 与 `nexus_llm::budget` **保持一致**：序列化字节 + framing 的**保守上界**。
//!
//! 不接真实 tokenizer —— 参考架构 §3.2.5 的理由：
//! 那会引入「分词器与厂商不一致」的新不确定性来源。

use nexus_core::error::{AppError, ErrorCode};
use nexus_llm::types::{Message, Role, ToolDef};

use crate::protocol::validate_messages;

/// 请求体的固定 framing 开销（字节）。
///
/// 与 `nexus_llm::budget` 的 `BASE_FRAMING` 保持一致。
const BASE_FRAMING: usize = 64;

/// 每条消息的 framing 开销（字节）。
const PER_MESSAGE_FRAMING: usize = 16;

/// 每个工具定义的 framing 开销（字节）。
const PER_TOOL_FRAMING: usize = 32;

/// 裁剪结果。
///
/// # 为什么返回结构体而不是裸 `Vec<Message>`
///
/// 裁剪需要报告**发生了什么** —— 丢了多少轮、剩多少字节。
/// 那不是「调试信息」，是**调用方做决策的依据**
/// （比如「历史被裁了很多」可能提示用户该开新会话了）。
#[derive(Debug, Clone, PartialEq)]
pub struct CompactedHistory {
    /// 裁剪后的消息序列。
    pub messages: Vec<Message>,
    /// 被淘汰的**轮**数。
    pub evicted_turns: usize,
    /// 裁剪后估算的请求体字节数（不含 framing）。
    pub body_bytes: usize,
    /// 裁剪后估算的 framing 开销字节数。
    pub framing_bytes: usize,
}

impl CompactedHistory {
    /// 裁剪后占用的**总**预算（请求体 + framing）。
    pub fn total_budget(&self) -> usize {
        self.body_bytes + self.framing_bytes
    }
}

/// 估算请求体的字节数。
///
/// 口径与 `nexus_llm::budget::estimate_body_bytes` **一致** ——
/// 两处必须同口径，否则「网关认为超预算、Agent 认为没超」
/// 会让错误出现在**意想不到**的地方。
///
/// # 为什么是 `pub`
///
/// 调用方需要**精确**知道一次请求占多少预算 ——
///
/// - 设定 `context_budget` 时要反推「够几轮」；
/// - 观测「还剩多少预算」时要有同一个口径。
///
/// ⚠️ 口径**必须**与内部裁剪一致 —— 否则调用方算的和裁剪算的对不上。
pub fn estimate_body_bytes(messages: &[Message], tools: &[ToolDef]) -> usize {
    let mut total = 0usize;

    for message in messages {
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
/// 估算 framing 开销的字节数。
///
/// # 为什么是 `pub`
///
/// 与 [`estimate_body_bytes`] 同理 —— 调用方要用**同一个口径**
/// 算「一次请求总共占多少预算」。
pub fn framing_bytes(message_count: usize, tool_count: usize) -> usize {
    BASE_FRAMING + PER_MESSAGE_FRAMING * message_count + PER_TOOL_FRAMING * tool_count
}

/// 估算一次请求占用的**总**预算（请求体 + framing）。
///
/// 这是**调用方**设定 `context_budget` 时要对齐的口径。
pub fn estimate_total_budget(messages: &[Message], tools: &[ToolDef]) -> usize {
    estimate_body_bytes(messages, tools) + framing_bytes(messages.len(), tools.len())
}

/// 一个**轮**的边界。
///
/// 一轮 = 一个 user 消息 + 其后的所有消息
/// （assistant 消息 + **所有**并行 tool 结果），
/// 直到下一个 user 消息为止。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Turn {
    /// 该轮的起始下标（含）。
    start: usize,
    /// 下一轮的起始下标（不含）—— 也就是本轮的结束下标。
    end: usize,
}

/// 找出所有**可淘汰**的轮。
///
/// # 为什么 `system` 消息不构成一轮的起点
///
/// system 消息是**指令**，必须保留。
/// 参考架构 §3.2.1 的算法第 3 步：「找出所有 user 消息位置 starts[]」——
/// 分轮**只**按 user 消息切。
///
/// # 为什么保留「首个 user 之前的前缀」
///
/// 那是运行开始时的上下文设定（通常是 system 消息，
/// 但也可能有别的角色）。它属于「起始设定」，不参与轮次淘汰。
fn find_turns(messages: &[Message]) -> Vec<Turn> {
    let starts: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::User)
        .map(|(index, _)| index)
        .collect();

    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| Turn {
            start,
            end: starts.get(i + 1).copied().unwrap_or(messages.len()),
        })
        .collect()
}

/// 上下文历史裁剪。
///
/// # 算法（参考架构 §3.2.1）
///
/// 1. **协议完整性校验**（`tool_call ↔ tool_result` 配对）—— **先于**任何裁剪。
/// 2. 估算总量（消息 + 工具定义）≤ 预算 → 原样返回。
/// 3. 找出所有 user 消息位置。
/// 4. 从最早的 user 轮开始，**整轮**删除（跳过 system 消息），
///    直到总量 ≤ 预算。
/// 5. 仍超预算 → [`ErrorCode::BudgetExceeded`]（30006）。
/// 6. **复核**裁剪结果的协议完整性。
///
/// # 保留什么
///
/// - **所有** system 消息
/// - 首个 user 之前的前缀
/// - **最新一轮** user
///
/// # 不修改入参
///
/// 入参按引用传入，返回**新**的 [`CompactedHistory`] —— 本函数是**纯函数**。
///
/// # 每步都重跑
///
/// 裁剪在**每次模型调用前**重跑 —— 因为工具结果会**增长**历史，
/// 第 1 步裁剪过的历史在第 3 步可能又超预算了。
/// 本函数是纯函数，调用时机由调用方负责。
pub fn compact_input_history(
    messages: &[Message],
    tools: &[ToolDef],
    budget: usize,
) -> Result<CompactedHistory, AppError> {
    // ── 第 1 步：协议完整性校验**先于**裁剪 ──
    //
    // 配对断裂时报的是**协议错误**，不是预算错误 ——
    // 错误信息必须指向真正的原因。
    validate_messages(messages).map_err(|err| err.to_app_error())?;

    let mut retained: Vec<Message> = messages.to_vec();
    let mut evicted_turns = 0usize;

    let mut body = estimate_body_bytes(&retained, tools);
    let mut framing = framing_bytes(retained.len(), tools.len());

    // ── 第 2 步：预算充足就原样返回 ──
    if body + framing <= budget {
        return Ok(CompactedHistory {
            messages: retained,
            evicted_turns,
            body_bytes: body,
            framing_bytes: framing,
        });
    }

    // ── 第 3、4 步：从最早的 user 轮开始整轮删除 ──
    //
    // ⚠️ 每轮都**重新**计算轮次边界 ——
    // 删除会**移动下标**，用一次算好的边界去 `drain` 后续轮次会删错位置。
    //
    // 最后一轮是「最新一轮 user」—— **必须保留**，
    // 所以当只剩一轮时就没有可淘汰的了。
    loop {
        let turns = find_turns(&retained);

        // 只有「最新一轮」或没有 user 轮 —— 没有可淘汰的轮次。
        if turns.len() <= 1 {
            break;
        }

        let turn = turns[0];

        // 整轮删除 —— 该轮的 assistant 消息与**所有**并行 tool 结果**一起**被删。
        //
        // ⚠️ 但**跳过 system 消息** —— system 是**指令**，必须保留
        // （参考架构 §3.2.1 的算法第 4 步：「直到下一个 user 轮为止的所有消息，
        // **但跳过 system 消息**」）。
        //
        // 所以不能直接 `drain(start..end)` —— 那会把轮次中间的 system 消息一起删掉。
        //
        // ⚠️ `end` 是在删除**之前**算出的 —— 每删一条消息，
        // 后续消息的**下标全部前移一位**。所以必须**同步收缩** `end`，
        // 否则会越界（`retained[index]` panic）。
        let mut index = turn.start;
        let mut end = turn.end;
        while index < end {
            if retained[index].role == Role::System {
                index += 1;
                continue;
            }
            retained.remove(index);
            end -= 1;
        }
        evicted_turns += 1;

        body = estimate_body_bytes(&retained, tools);
        framing = framing_bytes(retained.len(), tools.len());

        if body + framing <= budget {
            // ── 第 6 步：复核裁剪结果的协议完整性 ──
            verify_retained(&retained)?;
            return Ok(CompactedHistory {
                messages: retained,
                evicted_turns,
                body_bytes: body,
                framing_bytes: framing,
            });
        }
    }

    // ── 第 5 步：仍超预算 ──
    Err(budget_error())
}

/// ── 第 6 步：复核裁剪结果的协议完整性 ──
fn verify_retained(retained: &[Message]) -> Result<(), AppError> {
    validate_messages(retained)
        .map(|_| ())
        .map_err(|err| err.to_app_error())
}

/// 预算不足。
///
/// 错误码 [`ErrorCode::BudgetExceeded`]（30006）的 `retry_policy()` 是 `Terminal` ——
/// 「重试同样的请求同样会超预算」。
fn budget_error() -> AppError {
    AppError::new(
        ErrorCode::BudgetExceeded,
        "上下文超出预算，且可淘汰的历史轮次已用尽",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_llm::types::ToolCall;

    fn user(content: &str) -> Message {
        Message::new(Role::User, content)
    }

    fn assistant(content: &str) -> Message {
        Message::new(Role::Assistant, content)
    }

    fn assistant_with_calls(ids: &[&str]) -> Message {
        let mut message = Message::new(Role::Assistant, "");
        message.tool_calls = ids
            .iter()
            .map(|id| ToolCall {
                id: (*id).to_string(),
                name: "search".to_string(),
                args: "{}".to_string(),
            })
            .collect();
        message
    }

    /// 一组消息**恰好**占用的预算（body + framing）。
    ///
    /// 测试里用它构造「刚好够 / 刚好不够」的边界 ——
    /// 硬编码字节数会随消息内容变化而失效。
    fn budget_of(messages: &[Message]) -> usize {
        estimate_body_bytes(messages, &[]) + framing_bytes(messages.len(), 0)
    }

    /// 一个「两轮」历史：
    ///
    /// ```
    /// system
    /// user-1
    /// assistant(带 c1 调用)
    /// tool(c1)
    /// user-2
    /// assistant(带 c2 调用)
    /// tool(c2)
    /// ```
    fn two_turn_history() -> Vec<Message> {
        vec![
            Message::new(Role::System, "系统提示"),
            user("第一轮"),
            assistant_with_calls(&["c1"]),
            Message::tool_result("c1", "结果一"),
            user("第二轮"),
            assistant_with_calls(&["c2"]),
            Message::tool_result("c2", "结果二"),
        ]
    }

    #[test]
    fn compaction_is_pure() {
        let messages = two_turn_history();
        let before = messages.clone();
        let _ = compact_input_history(&messages, &[], 1024).expect("应成功");
        assert_eq!(messages, before, "裁剪不得修改入参");
    }

    #[test]
    fn compaction_does_not_mutate_the_input() {
        let messages = two_turn_history();
        let tools: Vec<ToolDef> = Vec::new();
        let original = messages.clone();
        let result = compact_input_history(&messages, &tools, 10);
        if let Ok(compacted) = result {
            assert_ne!(compacted.messages, original, "应确实裁掉了东西");
            assert_eq!(messages, original, "入参必须保持原样");
        }
    }

    #[test]
    fn a_history_within_the_budget_is_returned_unchanged() {
        let messages = two_turn_history();
        let result = compact_input_history(&messages, &[], 64 * 1024).expect("应成功");
        assert_eq!(result.messages, messages);
        assert_eq!(result.evicted_turns, 0);
    }

    #[test]
    fn all_system_messages_are_retained() {
        let messages = vec![
            Message::new(Role::System, "系统一"),
            user("第一轮"),
            assistant("回答一"),
            Message::new(Role::System, "系统二"),
            user("第二轮"),
            assistant("回答二"),
        ];
        // 淘汰第一轮后，**保留**的是：系统一、系统二、第二轮、回答二。
        //
        // ⚠️ 预算必须按**期望保留集**算 —— 因为 system 消息**不**随轮次淘汰，
        // 它们仍然占用预算。系统二就夹在第一轮**中间**。
        let expected = vec![
            Message::new(Role::System, "系统一"),
            Message::new(Role::System, "系统二"),
            user("第二轮"),
            assistant("回答二"),
        ];
        let result = compact_input_history(&messages, &[], budget_of(&expected)).expect("应成功");

        let systems: Vec<&str> = result
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            systems,
            vec!["系统一", "系统二"],
            "所有 system 消息必须保留 —— 包括**轮次中间**的 system 消息"
        );
        assert_eq!(result.messages, expected, "保留集应恰好是期望的那几条");
    }

    #[test]
    fn the_latest_user_turn_is_retained() {
        let messages = vec![
            user("第一轮"),
            assistant("回答一"),
            user("第二轮"),
            assistant("回答二"),
        ];
        // 预算刚好够**最新一轮**（user-2 + assistant-2）——
        // 所以更早的那一轮必须被淘汰。
        let budget = budget_of(&messages[2..]);
        let result = compact_input_history(&messages, &[], budget).expect("应成功");

        let users: Vec<&str> = result
            .messages
            .iter()
            .filter(|m| m.role == Role::User)
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            users,
            vec!["第二轮"],
            "最新一轮 user 必须保留；更早的轮次才可淘汰"
        );
    }

    #[test]
    fn a_whole_turn_is_evicted_with_its_parallel_tool_results() {
        // 两轮，各带一个工具调用。
        let messages = vec![
            user("第一轮"),
            assistant_with_calls(&["c1", "c2"]),
            Message::tool_result("c1", "结果一"),
            Message::tool_result("c2", "结果二"),
            user("第二轮"),
            assistant("最终回答"),
        ];

        // 预算刚好够第二轮 —— 第一轮必须**整轮**被删。
        let budget = budget_of(&messages[4..]);

        let result = compact_input_history(&messages, &[], budget).expect("应成功");
        assert_eq!(result.evicted_turns, 1, "应淘汰恰好一轮");

        // 第一轮的**所有**消息（含两个并行 tool 结果）都应消失。
        assert!(
            !result.messages.iter().any(|m| m.tool_call_id == "c1"),
            "并行 tool 结果必须随整轮一起被删"
        );
        assert!(
            !result.messages.iter().any(|m| m.tool_call_id == "c2"),
            "并行 tool 结果必须随整轮一起被删"
        );
        // 不留孤立 tool_result。
        verify_retained(&result.messages).expect("裁剪后协议必须仍然完整");
    }

    #[test]
    fn no_summary_message_is_generated() {
        let messages = vec![
            user("第一轮"),
            assistant("回答一"),
            user("第二轮"),
            assistant("回答二"),
        ];
        // 预算刚好够保留集（第二轮 + 回答二）。
        let expected = vec![user("第二轮"), assistant("回答二")];
        let result = compact_input_history(&messages, &[], budget_of(&expected)).expect("应成功");

        // 裁剪后的每条消息都必须是**原有**消息 —— 不新增任何摘要。
        for message in &result.messages {
            assert!(
                messages.contains(message),
                "裁剪不得生成新消息（摘要）—— 发现一条不在原始历史里的消息"
            );
        }
        assert_eq!(result.messages, expected);
    }

    #[test]
    fn a_history_that_exactly_fits_is_accepted() {
        let messages = vec![user("问题"), assistant("回答")];
        let exact = estimate_body_bytes(&messages, &[]) + framing_bytes(messages.len(), 0);
        let result = compact_input_history(&messages, &[], exact).expect("恰好填满必须通过");
        assert_eq!(result.messages, messages);
        assert_eq!(result.evicted_turns, 0);
    }

    #[test]
    fn an_oversized_history_is_rejected() {
        // 只有一轮 —— 没有可淘汰的轮次，只能报预算错误。
        let messages = vec![user(&"x".repeat(1024))];
        let err = compact_input_history(&messages, &[], 10).expect_err("必须被拒");
        assert_eq!(err.user_facing().code, ErrorCode::BudgetExceeded);
        assert_eq!(err.user_facing().code.code(), 30006);
    }

    #[test]
    fn tool_definitions_count_towards_the_budget() {
        let messages = vec![user("问题")];
        let tools = vec![ToolDef {
            source_identity: String::new(),
            binding_version: 0,
            capability: String::new(),
            dangerous: false,
            replay_policy: String::new(),
            name: "search".to_string(),
            description: "描述".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        // 预算够消息本身，但**不够**消息 + 工具定义 ——
        // 那就证明工具定义确实计入了预算。
        let messages_only = estimate_body_bytes(&messages, &[]) + framing_bytes(messages.len(), 0);
        let with_tools = estimate_body_bytes(&messages, &tools) + framing_bytes(messages.len(), 1);

        assert!(with_tools > messages_only, "工具定义必须增加预算占用");

        // 只给消息的预算 → 带工具时超预算。
        let err = compact_input_history(&messages, &tools, messages_only)
            .expect_err("工具定义计入预算后应超预算");
        assert_eq!(err.user_facing().code, ErrorCode::BudgetExceeded);

        // 给足预算 → 通过。
        let result = compact_input_history(&messages, &tools, with_tools).expect("应成功");
        assert!(result.total_budget() <= with_tools);
    }

    #[test]
    fn the_pairing_check_runs_before_the_budget_check() {
        // 同时**超预算**且**配对断裂** —— 报的必须是配对错误。
        let messages = vec![
            user(&"x".repeat(4096)),
            Message::tool_result("never-called", "结果"),
        ];
        let err = compact_input_history(&messages, &[], 10).expect_err("必须被拒");
        assert_eq!(
            err.user_facing().code,
            ErrorCode::InvalidParam,
            "协议错误必须优先于预算错误"
        );
    }

    #[test]
    fn the_budget_error_is_terminal_not_retryable() {
        let messages = vec![user(&"x".repeat(1024))];
        let err = compact_input_history(&messages, &[], 10).expect_err("必须被拒");
        assert_eq!(
            err.retry_policy(),
            nexus_core::error::RetryPolicy::Terminal,
            "重试同样的请求同样会超预算"
        );
    }

    #[test]
    fn the_prefix_before_the_first_user_is_retained() {
        let messages = vec![
            Message::new(Role::System, "系统"),
            user("第一轮"),
            assistant("回答一"),
            user("第二轮"),
            assistant("回答二"),
        ];
        // 保留集 = 系统 + 第二轮 + 回答二。
        let expected = vec![
            Message::new(Role::System, "系统"),
            user("第二轮"),
            assistant("回答二"),
        ];
        let result = compact_input_history(&messages, &[], budget_of(&expected)).expect("应成功");
        assert_eq!(
            result.messages[0].content, "系统",
            "首个 user 之前的前缀必须保留"
        );
        assert_eq!(result.messages, expected);
    }
}
