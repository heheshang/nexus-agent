//! SSE 解析 —— 把 SSE 事件翻译成网关的 [`StreamEvent`]。
//!
//! # 首事件边界
//!
//! 「首事件前可回退、首事件后不可回退」的判定在**网关层**（[`crate::gateway`]），
//! 本模块只负责**忠实翻译**。
//!
//! # 为什么用 `eventsource-stream`
//!
//! SSE 的分帧规则（空行分隔、`data:` 前缀、多行拼接）已被实现并测试过 ——
//! 自己手写分帧就是**重新发明一个容易出 bug 的解析器**。

use eventsource_stream::Event;

use crate::convert::OpenAiStreamChunk;
use crate::error::LlmError;
use crate::types::StreamEvent;

/// 解析一个 SSE 事件。
///
/// # 返回 `Ok(None)` 的情况
///
/// - 空 `data` —— 心跳/注释。
/// - `data: [DONE]` —— 流结束标记。
pub fn parse(event: &Event) -> Result<Option<StreamEvent>, LlmError> {
    let data = event.data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(None);
    }
    let chunk: OpenAiStreamChunk = serde_json::from_str(data)
        .map_err(|e| LlmError::Malformed(format!("SSE 载荷解析失败: {e}")))?;
    Ok(chunk_to_event(chunk))
}

/// 把一个流式 chunk 翻译成网关的 [`StreamEvent`]。
///
/// # 返回 `None` 的情况
///
/// chunk 里没有任何有内容 —— 上游可能发空帧保活。
fn chunk_to_event(chunk: OpenAiStreamChunk) -> Option<StreamEvent> {
    let choice = chunk.choices.into_iter().next()?;
    let content = choice.delta.content.unwrap_or_default();
    let reasoning = choice.delta.reasoning_content.unwrap_or_default();
    let finish_reason = choice.finish_reason.unwrap_or_default();
    let tool_calls: Vec<crate::types::ToolCall> = choice
        .delta
        .tool_calls
        .into_iter()
        .map(|c| crate::types::ToolCall {
            id: c.id,
            name: c.name,
            args: c.arguments,
        })
        .collect();
    let usage = chunk.usage.map(|u| crate::types::Usage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        cost: None,
    });
    if content.is_empty()
        && reasoning.is_empty()
        && finish_reason.is_empty()
        && tool_calls.is_empty()
        && usage.is_none()
    {
        return None;
    }
    Some(StreamEvent {
        err: None,
        content,
        reasoning_content: reasoning,
        tool_calls,
        finish_reason,
        usage,
        model: chunk.model,
    })
}
