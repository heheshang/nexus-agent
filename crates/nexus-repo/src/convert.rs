//! 行类型 → 领域类型的转换。
//!
//! # 为什么要有这一层
//!
//! 架构文档 §4.2 差异 2 的核心：**行类型不能直接流到能力层**。
//! 如果仓储直接返回 `UserRow`，那么：
//!
//! - 能力层就看见了**数据库列布局**（`password_hash` / `status: i16` / `avatar_url`），
//!   改表结构就会波及能力层 —— 正是差异 2 要避免的。
//! - `password_hash` 会被顺手带出去 —— 它是**凭据**，不该出现在领域类型里。
//!
//! 所以这里提供**显式**的转换函数：行类型是「怎么存的」，领域类型是「是什么」。
//!
//! # 转换是**无损**的（除了刻意剥离的凭据）
//!
//! 领域类型缺失的字段**只有一个**：`password_hash`。
//! 它被单独放进 [`user_credential`] —— 只有登录校验路径会调它。

use nexus_core::id::{ToolCallId, TraceId};
use nexus_model::{Message, Session, User, UserCredential};

use crate::rows::{MessageRow, SessionRow, UserRow};

/// 行类型 → 用户领域类型。
///
/// **剥离 `password_hash`** —— 凭据不进领域类型（见模块文档）。
pub fn user(row: UserRow) -> User {
    User {
        id: row.id,
        username: row.username,
        email: row.email,
        role: row.role,
        avatar_url: row.avatar_url,
        status: row.status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// 行类型 → 用户凭据。
///
/// **单独提供** —— 登录校验需要散列，但领域类型 `User` 里不该有它。
pub fn user_credential(row: UserRow) -> UserCredential {
    UserCredential {
        user_id: row.id,
        password_hash: row.password_hash,
    }
}

/// 行类型 → 会话领域类型。
pub fn session(row: SessionRow) -> Session {
    Session {
        id: row.id,
        user_id: row.user_id,
        agent_id: row.agent_id.get(),
        title: row.title,
        status: row.status,
        pinned: row.pinned,
        message_count: row.message_count,
        token_usage: row.token_usage,
        last_msg_at: row.last_msg_at,
        metadata: row.metadata.0,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

/// 行类型 → 消息领域类型。
pub fn message(row: MessageRow) -> Message {
    Message {
        id: row.id,
        session_id: row.session_id,
        role: row.role,
        content: row.content,
        reasoning_content: row.reasoning_content,
        tool_calls: row.tool_calls.map(|json| json.0),
        tool_call_id: row.tool_call_id.map(ToolCallId::new),
        refs: row.refs.map(|json| json.0),
        model_name: row.model_name,
        prompt_tokens: row.prompt_tokens,
        completion_tokens: row.completion_tokens,
        first_token_ms: row.first_token_ms,
        latency_ms: row.latency_ms,
        status: row.status,
        trace_id: TraceId::new(row.trace_id),
        metadata: row.metadata.0,
        created_at: row.created_at,
    }
}

/// 去掉 PostgreSQL `CHAR(n)`（`bpchar`）的**尾随空格**。
///
/// # 为什么需要
///
/// `workflow_versions.definition_hash` 与 `execution_runs.definition_hash`
/// 的列类型是 `CHAR(64)`。PostgreSQL 的 `bpchar` **按定长存储**——
/// 写入 64 字符的哈希刚好填满，但**短于 64 的值会被补空格**。
///
/// 更要紧的是：`bpchar` 的比较语义会**忽略尾随空格**
/// （`'abc'::char(64) = 'abc'` 为真），所以**数据库层面看不出差异**，
/// 但 Rust 侧拿到的是带空格的字符串 —— 拿它去比对哈希就会**永远不相等**。
///
/// 这是「看着像细节、实际会炸」的坑（参考架构文档附录 C.1 那类）。
///
/// # 为什么不靠 SQL 侧 `TRIM`
///
/// 因为行类型要能对应**任意**查询 —— 加 `TRIM` 意味着「行类型只能配特定 SQL 用」，
/// 那就失去了行类型的通用性。转换层做归一化更合适。
pub fn trim_bpchar(value: &str) -> &str {
    value.trim_end_matches(' ')
}
