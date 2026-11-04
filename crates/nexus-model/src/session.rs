//! 会话与消息领域类型。
//!
//! **纯 DTO** —— 见 [`crate::user`] 模块文档的同一原则。
//!
//! # 会话 ID 是 UUID
//!
//! 架构文档 §2.8.1 把 `SessionId` 定为 `Uuid` 底座：会话 ID 会返回给客户端，
//! 客户端可能**提前**用它发起请求（前端乐观渲染）。自增 ID 要求先插入才知道值，
//! UUID 可以客户端预生成。

use chrono::{DateTime, Utc};
use nexus_core::id::{AgentId, MessageId, SessionId, ToolCallId, TraceId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 会话领域类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// 会话 ID（UUID）。
    pub id: SessionId,
    /// 归属用户。
    pub user_id: UserId,
    /// 绑定的 Agent（可空 —— 会话可以不绑定 Agent）。
    pub agent_id: Option<AgentId>,
    /// 标题。
    pub title: String,
    /// 状态。
    ///
    /// 迁移里的默认值是 `'active'`；完整取值集合未见于文档，故保持 `String`。
    pub status: String,
    /// 是否置顶。
    pub pinned: bool,
    /// 消息条数（数据库维护的计数，**不是** `COUNT(*)` 的实时值）。
    pub message_count: i32,
    /// 累计 token 用量。
    pub token_usage: i64,
    /// 最后一条消息的时间。
    pub last_msg_at: Option<DateTime<Utc>>,
    /// 扩展元数据。
    pub metadata: Value,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 最后更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 消息领域类型。
///
/// # `reasoning_content` 与 `content` 分开
///
/// 参考实现把「状态」与「内容」分开持久化 —— 失败也保留已生成的部分内容
/// （参考架构文档 §3.x「终态与内容分离」）。合并成一个字段就丢掉了这个能力。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// 消息 ID。
    pub id: MessageId,
    /// 所属会话。
    pub session_id: SessionId,
    /// 角色（`user` / `assistant` / `tool` / `system`）。
    pub role: String,
    /// 正文内容。
    pub content: String,
    /// 推理内容（可空）。
    pub reasoning_content: Option<String>,
    /// 工具调用列表（可空）。
    pub tool_calls: Option<Value>,
    /// 工具调用 ID（可空）。
    pub tool_call_id: Option<ToolCallId>,
    /// 引用（可空）。
    pub refs: Option<Value>,
    /// 模型名（可空）。
    pub model_name: Option<String>,
    /// 输入 token 数。
    pub prompt_tokens: i32,
    /// 输出 token 数。
    pub completion_tokens: i32,
    /// 首 token 延迟（毫秒）。
    pub first_token_ms: i32,
    /// 总延迟（毫秒）。
    pub latency_ms: i32,
    /// 状态（`SMALLINT`）。
    pub status: i16,
    /// 追踪 ID。
    pub trace_id: TraceId,
    /// 扩展元数据。
    pub metadata: Value,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 新建会话的输入。
///
/// `id` 由**调用方**生成 —— 会话 ID 可以在客户端预生成（§2.8.1），
/// 所以仓储层必须接受外部给定的 ID，而不是自己 `new_v4()`。
#[derive(Debug, Clone, PartialEq)]
pub struct NewSession {
    /// 会话 ID。
    pub id: SessionId,
    /// 归属用户。
    pub user_id: UserId,
    /// 绑定的 Agent。
    pub agent_id: Option<AgentId>,
    /// 标题（`None` 时用数据库默认值 `'新对话'`）。
    pub title: Option<String>,
}
