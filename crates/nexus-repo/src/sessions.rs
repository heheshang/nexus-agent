//! 会话仓储。
//!
//! # 会话 ID 由调用方生成
//!
//! 架构文档 §2.8.1：`SessionId` 是 `Uuid` 底座，因为会话 ID 会返回给客户端，
//! 客户端可能**提前**用它发起请求（前端乐观渲染）。
//! 所以插入时**必须**接受外部给定的 ID —— 不能由仓储 `new_v4()`。
//!
//! # 「查不到」的语义
//!
//! 会话查不到 → [`ErrorCode::SessionNotFound`]（404，终止型）。

use nexus_core::error::{AppError, ErrorCode};
use nexus_core::id::{SessionId, UserId};
use nexus_model::{NewSession, Session};
use sqlx::PgPool;

use crate::error::map_sqlx_error;
use crate::rows::SessionRow;

/// 会话仓储。
#[derive(Debug, Clone)]
pub struct SessionRepository {
    pool: PgPool,
}

impl SessionRepository {
    /// 用连接池构造。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 底层连接池。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 按 ID 查会话。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn find_by_id(&self, id: SessionId) -> Result<Option<Session>, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, agent_id, title, status, pinned, message_count, \
                    token_usage, last_msg_at, metadata, created_at, updated_at \
             FROM sessions WHERE id = $1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(crate::convert::session))
    }

    /// 按 ID 查会话，**查不到就报错**。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::SessionNotFound`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn require_by_id(&self, id: SessionId) -> Result<Session, AppError> {
        self.find_by_id(id)
            .await?
            .ok_or_else(|| AppError::new(ErrorCode::SessionNotFound, "会话不存在"))
    }

    /// 按 ID 查会话，**并校验归属**。
    ///
    /// # 为什么归属校验**下沉到 SQL 的 WHERE**
    ///
    /// 参考架构文档明确要求：**归属校验下沉到 SQL WHERE 子句**
    /// （`WHERE id = $1 AND user_id = $2`），而不是「先查出来再比较」。
    ///
    /// 理由有两个：
    /// 1. 避免「先查后比」的**竞态** —— 记录被转移归属的窗口期。
    /// 2. **不泄露「资源存在但无权访问」** —— 两者都返回同一个 404，
    ///    否则能用响应差异探测资源是否存在。
    ///
    /// # 错误
    ///
    /// - 记录不存在**或不属于该用户** → [`ErrorCode::SessionNotFound`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn find_owned(
        &self,
        id: SessionId,
        owner: UserId,
    ) -> Result<Option<Session>, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, agent_id, title, status, pinned, message_count, \
                    token_usage, last_msg_at, metadata, created_at, updated_at \
             FROM sessions WHERE id = $1 AND user_id = $2",
        )
        .bind(id.into_inner())
        .bind(owner.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(crate::convert::session))
    }

    /// 列出某用户的会话（按最近活跃倒序，分页）。
    ///
    /// 排序键是 `last_msg_at DESC` —— 对应 `idx_sessions_user` 索引
    /// （`sessions(user_id, last_msg_at DESC)`）。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn list_by_user(
        &self,
        user_id: UserId,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Session>, AppError> {
        let rows = sqlx::query_as::<_, SessionRow>(
            "SELECT id, user_id, agent_id, title, status, pinned, message_count, \
                    token_usage, last_msg_at, metadata, created_at, updated_at \
             FROM sessions WHERE user_id = $1 \
             ORDER BY last_msg_at DESC NULLS LAST, id DESC \
             LIMIT $2 OFFSET $3",
        )
        .bind(user_id.into_inner())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(crate::convert::session).collect())
    }

    /// 创建会话。
    ///
    /// `title` 为 `None` 时用数据库默认值 `'新对话'`。
    ///
    /// # 错误
    ///
    /// - 归属用户不存在 → [`ErrorCode::InvalidParam`]（外键约束）
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn create(&self, new_session: NewSession) -> Result<Session, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "INSERT INTO sessions (id, user_id, agent_id, title) \
             VALUES ($1, $2, $3, COALESCE($4, '新对话')) \
             RETURNING id, user_id, agent_id, title, status, pinned, message_count, \
                       token_usage, last_msg_at, metadata, created_at, updated_at",
        )
        .bind(new_session.id.into_inner())
        .bind(new_session.user_id.into_inner())
        .bind(new_session.agent_id.map(|id| id.into_inner()))
        .bind(new_session.title.as_deref())
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(crate::convert::session(row))
    }

    /// 更新会话标题。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::SessionNotFound`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn update_title(&self, id: SessionId, title: &str) -> Result<Session, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "UPDATE sessions SET title = $2, updated_at = now() WHERE id = $1 \
             RETURNING id, user_id, agent_id, title, status, pinned, message_count, \
                       token_usage, last_msg_at, metadata, created_at, updated_at",
        )
        .bind(id.into_inner())
        .bind(title)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(crate::convert::session)
            .ok_or_else(|| AppError::new(ErrorCode::SessionNotFound, "会话不存在"))
    }

    /// 置顶 / 取消置顶。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::SessionNotFound`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn set_pinned(&self, id: SessionId, pinned: bool) -> Result<Session, AppError> {
        let row = sqlx::query_as::<_, SessionRow>(
            "UPDATE sessions SET pinned = $2, updated_at = now() WHERE id = $1 \
             RETURNING id, user_id, agent_id, title, status, pinned, message_count, \
                       token_usage, last_msg_at, metadata, created_at, updated_at",
        )
        .bind(id.into_inner())
        .bind(pinned)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(crate::convert::session)
            .ok_or_else(|| AppError::new(ErrorCode::SessionNotFound, "会话不存在"))
    }

    /// 删除会话。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    ///
    /// **不返回「不存在」错误** —— 删除是幂等的。
    pub async fn delete(&self, id: SessionId) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        Ok(result.rows_affected() > 0)
    }
}
