//! OpenAI 兼容协议的**线上格式**。
//!
//! # 为什么这些类型与 [`crate::types`] 分开
//!
//! [`crate::types`] 是**网关协议类型** —— 网关内部流转，不对外。
//! 这里是**上游协议形状** —— 由供应商的 HTTP 接口决定。
//!
//! 两者**必须**分开：上游协议会变（供应商加字段、改字段名），
//! 而网关协议类型是内部契约。混在一起意味着上游一改就动到内部契约。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 非流式请求体（OpenAI 兼容协议）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAiChatRequest {
    /// 模型名。
    pub model: String,
    /// 消息列表。
    pub messages: Vec<OpenAiMessage>,
    /// 工具定义。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<OpenAiTool>,
    /// 采样温度。
    pub temperature: f32,
    /// 核采样。
    pub top_p: f32,
    /// 输出 token 上限。
    pub max_tokens: i32,
    /// 是否流式。
    pub stream: bool,
}

/// 请求里的单条消息。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAiMessage {
    /// 角色。
    pub role: String,
    /// 文本内容。
    pub content: String,
    /// 推理内容（部分供应商的扩展字段）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// 工具调用。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<OpenAiToolCall>,
    /// 工具调用 ID。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 名称。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// 工具调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiToolCall {
    /// 调用 ID。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// 参数（JSON **字符串** —— 协议如此）。
    pub arguments: String,
}

/// 工具定义。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAiTool {
    /// 工具类型（恒为 `function`）。
    #[serde(rename = "type")]
    pub kind: String,
    /// 函数定义。
    pub function: OpenAiFunction,
}

/// 函数定义。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenAiFunction {
    /// 名称。
    pub name: String,
    /// 描述。
    pub description: String,
    /// 参数 JSON Schema。
    pub parameters: Value,
}

/// 非流式响应。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiChatResponse {
    /// 候选回复。
    #[serde(default)]
    pub choices: Vec<OpenAiChoice>,
    /// 实际上游模型名。
    #[serde(default)]
    pub model: String,
    /// 用量。
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
}

/// 一个候选。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiChoice {
    /// 消息。
    pub message: OpenAiResponseMessage,
    /// 结束原因。
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// 响应里的消息。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiResponseMessage {
    /// 文本内容。
    #[serde(default)]
    pub content: Option<String>,
    /// 推理内容。
    #[serde(default)]
    pub reasoning_content: Option<String>,
    /// 工具调用。
    #[serde(default)]
    pub tool_calls: Vec<OpenAiToolCall>,
}

/// 上游返回的用量。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenAiUsage {
    /// 输入 token 数。
    #[serde(default)]
    pub prompt_tokens: i64,
    /// 输出 token 数。
    #[serde(default)]
    pub completion_tokens: i64,
}
