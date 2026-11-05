//! 上游协议 ↔ 网关协议的转换。
//!
//! # 转换放在**边界**上
//!
//! 上游协议形状（`arguments` 是 JSON 字符串、`content` 可为 null）
//! 与网关协议形状（`args` 是字符串、`content` 是 `String`）**不同**。
//! 转换只在边界发生一次 —— 内部代码永远只看见网关协议类型。

use serde::Deserialize;

use crate::error::LlmError;
use crate::protocol::{OpenAiChatRequest, OpenAiChatResponse, OpenAiMessage, OpenAiTool};
use crate::types::{ChatRequest, ChatResponse, Message, ModelDescriptor, ToolDef, Usage};

impl OpenAiChatResponse {
    /// 转成网关的 [`ChatResponse`]。
    ///
    /// # 错误
    ///
    /// 上游返回**空 choices** 是协议违规 —— 返回 `Err(LlmError::Malformed)`。
    /// 不返回「空响应」—— 那会让调用方以为模型返回了空内容，
    /// 而真相是「上游的响应形状不对」。
    pub fn into_chat_response(self) -> Result<ChatResponse, LlmError> {
        let choice = self
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::Malformed("上游响应缺少 choices".to_string()))?;

        let tool_calls = choice
            .message
            .tool_calls
            .into_iter()
            .map(|c| crate::types::ToolCall {
                id: c.id,
                name: c.name,
                args: c.arguments,
            })
            .collect();

        let usage = self.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            // 上游不返回成本 —— 成本由本地按单价核算。
            cost: None,
        });

        Ok(ChatResponse {
            content: choice.message.content.unwrap_or_default(),
            reasoning_content: choice.message.reasoning_content.unwrap_or_default(),
            tool_calls,
            finish_reason: choice.finish_reason.unwrap_or_default(),
            model: self.model,
            usage: usage.unwrap_or_default(),
        })
    }
}

/// 把网关请求转成上游请求体。
///
/// # 参数
///
/// - `desc` —— 模型描述（提供模型名与采样默认值）。
/// - `req` —— 网关请求。
/// - `stream` —— 是否流式。
/// - `max_tokens` —— **已裁剪过的**输出上限（来自预算裁剪）。
pub fn build_request_body(
    desc: &ModelDescriptor,
    req: &ChatRequest,
    stream: bool,
    max_tokens: i32,
) -> OpenAiChatRequest {
    OpenAiChatRequest {
        model: desc.model_name.clone(),
        messages: req.messages.iter().map(to_openai_message).collect(),
        tools: req.tools.iter().map(to_openai_tool).collect(),
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens,
        stream,
    }
}

/// 网关消息 → 上游消息。
fn to_openai_message(msg: &Message) -> OpenAiMessage {
    OpenAiMessage {
        role: msg.role.as_str().to_string(),
        content: msg.content.clone(),
        reasoning_content: (!msg.reasoning_content.is_empty())
            .then(|| msg.reasoning_content.clone()),
        tool_calls: msg
            .tool_calls
            .iter()
            .map(|c| crate::protocol::OpenAiToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                arguments: c.args.clone(),
            })
            .collect(),
        tool_call_id: (!msg.tool_call_id.is_empty()).then(|| msg.tool_call_id.clone()),
        name: (!msg.name.is_empty()).then(|| msg.name.clone()),
    }
}

/// 网关工具定义 → 上游工具定义。
///
/// # ⚠️ 控制面字段**不参与**转换
///
/// `dangerous` / `replay_policy` / `source_identity` / `binding_version`
/// 是控制面的东西，**绝不发给模型**。这里是那道闸门 ——
///
/// 之前 `ToolDef` 自身的 `#[serde(skip)]` 已经挡过一次，
/// 这里再挡一次是因为**转换不经过 serde**（是手写字段映射）。
///
/// 两道闸门不是冗余：serde 那道防「直接序列化 `ToolDef`」，
/// 这道防「走转换路径」。
fn to_openai_tool(tool: &ToolDef) -> OpenAiTool {
    OpenAiTool {
        kind: "function".to_string(),
        function: crate::protocol::OpenAiFunction {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: tool.parameters.clone(),
        },
    }
}

/// 流式 chunk（`data: {...}` 的载荷）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiStreamChunk {
    /// 候选。
    #[serde(default)]
    pub choices: Vec<OpenAiStreamChoice>,
    /// 实际上游模型名。
    #[serde(default)]
    pub model: String,
    /// 用量（部分供应商在最后一帧携带）。
    #[serde(default)]
    pub usage: Option<OpenAiUsageAlias>,
}

/// 上游用量（流式里复用非流式的形状）。
pub type OpenAiUsageAlias = crate::protocol::OpenAiUsage;

/// 流式候选。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiStreamChoice {
    /// 增量。
    #[serde(default)]
    pub delta: OpenAiStreamDelta,
    /// 结束原因。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// 流式增量。
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct OpenAiStreamDelta {
    /// 文本内容。
    #[serde(default)]
    pub content: Option<String>,
    /// 推理内容。
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// 工具调用。
    #[serde(default)]
    pub tool_calls: Vec<OpenAiToolCallAlias>,
}

/// 流式工具调用。
pub type OpenAiToolCallAlias = crate::protocol::OpenAiToolCall;
