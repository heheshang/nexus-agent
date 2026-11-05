//! 工具调用协议的完整性校验。
//!
//! # 为什么协议校验**先于**裁剪
//!
//! 参考架构 §2.2 不变量 ③ 与 §3.2.2 的注释原文：
//!
//! > Validate the entire protocol before any history can be discarded.
//! > The same accounting is used by both admission and history reduction.
//!
//! 理由是**错误信息必须指向真正的原因**：
//! 如果先裁剪，配对断裂会被裁剪**掩盖** ——
//! 报出来的是「预算不足」，而真正的问题是「协议非法」。
//!
//! 调试成本天差地别：前者让人去调预算，后者让人去查工具调用链。
//!
//! # 校验什么
//!
//! | 检查 | 违反时的含义 |
//! |---|---|
//! | `tool` 消息必须有对应的 pending 调用 | 孤立 tool_result —— 模型无法判断它对应哪次调用 |
//! | 非 `tool` 消息前不得有未闭合的调用 | 缺失 tool_result —— 模型看到「发起了调用但没有结果」 |
//! | 调用 ID 不得为空、不得重复 | 重复 ID 会让结果与调用**错配** |
//! | 遍历结束不得有未闭合的调用 | 同「缺失 tool_result」 |

use std::collections::HashSet;

use nexus_core::error::{AppError, ErrorCode};
use nexus_llm::types::{Message, Role};

/// 协议完整性校验的结果。
///
/// # 为什么返回**统计**而不只是 `Ok`
///
/// 参考架构 §3.2.2 要求「准入」与「裁剪」**用同一套算法** ——
/// 避免两套算法算出不同结论。
///
/// 返回的 `pending` 与 `call_ids` 让调用方能复用同一套账本，
/// 而不是重新扫一遍消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolSummary {
    /// 消息里出现过的全部工具调用 ID。
    pub call_ids: HashSet<String>,
    /// 已闭合的调用（`tool` 消息消费掉的）。
    pub resolved: usize,
}

/// 协议非法。
///
/// 三种形态各有**不同的可诊断信息** —— 合并成一个泛化错误
/// 会让人无法判断是哪一种出了问题。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    /// 孤立的工具结果 —— 没有任何调用与它对应。
    #[error("孤立的工具结果：{tool_call_id}")]
    UnpairedToolResult {
        /// 找不到对应调用的调用 ID。
        tool_call_id: String,
    },

    /// 缺失工具结果 —— 在下一个非 `tool` 消息之前，调用没有结果。
    #[error("缺失工具结果：{call_ids:?}")]
    MissingToolResult {
        /// 尚未闭合的调用 ID 列表。
        call_ids: Vec<String>,
    },

    /// 重复的调用 ID。
    #[error("重复的工具调用 ID：{call_id}")]
    DuplicateToolCallId {
        /// 重复出现的调用 ID。
        call_id: String,
    },

    /// 调用 ID 为空。
    #[error("工具调用 ID 为空")]
    EmptyToolCallId,
}

impl ProtocolError {
    /// 转成应用错误。
    ///
    /// 统一映射到 `InvalidParam`（10001）——
    /// 协议非法是**明确失败**，重试同样的历史只会再撞一次同样的错误。
    pub fn to_app_error(&self) -> AppError {
        AppError::new(ErrorCode::InvalidParam, self.to_string())
    }
}

/// 校验消息列表的工具调用协议完整性。
///
/// # 规则（逐条消息推进）
///
/// 1. `Role::Tool` 的消息 —— `tool_call_id` 必须在 `pending` 里，
///    否则是**孤立结果**；命中则从 `pending` 移除。
/// 2. `Role::Assistant` 且带 `tool_calls` 的消息 —— 每个调用 ID
///    必须**非空**且**不重复**，然后加入 `pending`。
/// 3. 其他角色的消息 —— 若 `pending` 非空，说明**缺失结果**
///    （有调用发起了却没有结果，却先来了下一条消息）。
///
/// 遍历结束时 `pending` 必须为空，否则是**缺失工具结果**。
///
/// # 为什么用 `HashSet`
///
/// 只做**存在性**判断，无序不影响结论 —— 参考架构 §3.2.2 的说明一致。
///
/// # 为什么 `pending` 是 `Vec` 而不是 `HashSet`
///
/// 错误信息里要**按发生顺序**列出未闭合的调用 ID ——
/// 顺序本身是诊断信息的一部分。
pub fn validate_messages(messages: &[Message]) -> Result<ProtocolSummary, ProtocolError> {
    let mut pending: Vec<String> = Vec::new();
    let mut seen_call_ids: HashSet<String> = HashSet::new();
    let mut resolved = 0usize;

    for message in messages {
        match message.role {
            Role::Tool => {
                // ⚠️ 必须**先取出位置再改** ——
                // 直接 `find` 拿引用再 `retain` 会撞 E0502（同时可变与不可变借用）。
                let found = pending.iter().position(|id| *id == message.tool_call_id);
                let Some(index) = found else {
                    return Err(ProtocolError::UnpairedToolResult {
                        tool_call_id: message.tool_call_id.clone(),
                    });
                };
                pending.remove(index);
                resolved += 1;
            }
            Role::Assistant => {
                for call in &message.tool_calls {
                    if call.id.is_empty() {
                        return Err(ProtocolError::EmptyToolCallId);
                    }
                    if !seen_call_ids.insert(call.id.clone()) {
                        return Err(ProtocolError::DuplicateToolCallId {
                            call_id: call.id.clone(),
                        });
                    }
                    pending.push(call.id.clone());
                }
            }
            _ => {
                // 非 tool 消息出现在未闭合的调用之前 —— 那些调用没有结果。
                if !pending.is_empty() {
                    return Err(ProtocolError::MissingToolResult {
                        call_ids: pending.clone(),
                    });
                }
            }
        }
    }

    if !pending.is_empty() {
        return Err(ProtocolError::MissingToolResult { call_ids: pending });
    }

    Ok(ProtocolSummary {
        call_ids: seen_call_ids,
        resolved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_llm::types::ToolCall;

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

    #[test]
    fn an_unpaired_tool_result_is_rejected() {
        let messages = vec![Message::tool_result("c1", "ok")];
        let err = validate_messages(&messages).expect_err("孤立工具结果必须被拒");
        assert_eq!(
            err,
            ProtocolError::UnpairedToolResult {
                tool_call_id: "c1".to_string()
            }
        );
    }

    #[test]
    fn a_missing_tool_result_is_rejected() {
        // 调用发起了，但下一个 user 消息之前没有结果。
        let messages = vec![
            assistant_with_calls(&["c1"]),
            Message::new(Role::User, "继续"),
        ];
        let err = validate_messages(&messages).expect_err("缺失工具结果必须被拒");
        assert_eq!(
            err,
            ProtocolError::MissingToolResult {
                call_ids: vec!["c1".to_string()]
            }
        );
    }

    #[test]
    fn a_missing_tool_result_at_the_end_is_rejected() {
        let messages = vec![assistant_with_calls(&["c1"])];
        let err = validate_messages(&messages).expect_err("结尾缺失工具结果必须被拒");
        assert_eq!(
            err,
            ProtocolError::MissingToolResult {
                call_ids: vec!["c1".to_string()]
            }
        );
    }

    #[test]
    fn a_duplicate_tool_call_id_is_rejected() {
        let messages = vec![
            assistant_with_calls(&["c1"]),
            Message::tool_result("c1", "ok"),
            assistant_with_calls(&["c1"]),
        ];
        let err = validate_messages(&messages).expect_err("重复调用 ID 必须被拒");
        assert_eq!(
            err,
            ProtocolError::DuplicateToolCallId {
                call_id: "c1".to_string()
            }
        );
    }

    #[test]
    fn an_empty_tool_call_id_is_rejected() {
        let messages = vec![assistant_with_calls(&[""])];
        let err = validate_messages(&messages).expect_err("空调用 ID 必须被拒");
        assert_eq!(err, ProtocolError::EmptyToolCallId);
    }

    #[test]
    fn a_well_formed_history_is_accepted() {
        let messages = vec![
            Message::new(Role::System, "你是助手"),
            Message::new(Role::User, "查一下"),
            assistant_with_calls(&["c1", "c2"]),
            Message::tool_result("c1", "a"),
            Message::tool_result("c2", "b"),
            Message::new(Role::User, "继续"),
        ];
        let summary = validate_messages(&messages).expect("合法历史必须被接受");
        assert_eq!(summary.resolved, 2);
        assert_eq!(summary.call_ids.len(), 2);
    }

    #[test]
    fn parallel_tool_results_in_any_order_are_accepted() {
        // 多个工具并发执行，结果**回来的顺序**不确定 ——
        // 协议只要求「每个结果都有对应调用」，不要求顺序一致。
        let messages = vec![
            assistant_with_calls(&["c1", "c2"]),
            Message::tool_result("c2", "b"),
            Message::tool_result("c1", "a"),
        ];
        let summary = validate_messages(&messages).expect("乱序结果必须被接受");
        assert_eq!(summary.resolved, 2);
    }

    #[test]
    fn the_protocol_error_maps_to_invalid_param() {
        let err = ProtocolError::EmptyToolCallId.to_app_error();
        assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);
        assert_eq!(err.user_facing().code.code(), 10001);
    }

    #[test]
    fn the_protocol_error_is_terminal() {
        // 协议非法是**明确失败** —— 重试同样的历史只会再撞一次同样的错误。
        let err = ProtocolError::EmptyToolCallId.to_app_error();
        assert_eq!(err.retry_policy(), nexus_core::error::RetryPolicy::Terminal);
    }

    #[test]
    fn the_pairing_check_runs_before_the_budget_check() {
        // AC #7 / P4-I04 —— 协议校验**先于**裁剪。
        //
        // 裁剪（预算）本身在 NEX-27 落地；本测试守住的��
        // **配对错误优先于预算错误**这条**顺序**语义。
        //
        // 构造：历史**同时**超预算（内容巨大）**且**配对断裂。
        // 期望：报的是**配对错误** —— 不是预算错误。
        //
        // 如果有人把顺序倒过来（先裁剪再校验），
        // 这条测试会看到预算错误 —— 它就红了。
        let huge = "x".repeat(1024 * 1024);
        let messages = vec![
            Message::new(Role::User, huge.clone()),
            Message::new(Role::User, huge),
            // 孤立工具结果 —— 配对断裂。
            Message::tool_result("never-called", "ok"),
        ];

        let err = validate_messages(&messages).expect_err("配对断裂必须被拒");
        assert_eq!(
            err,
            ProtocolError::UnpairedToolResult {
                tool_call_id: "never-called".to_string()
            },
            "配对错误必须优先于预算错误 —— 协议完整性先于裁剪"
        );
    }
}
