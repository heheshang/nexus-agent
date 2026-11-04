//! 用户仓储。
//!
//! # 形状约定
//!
//! | 方法 | 「查不到」时 | 错误 |
//! |---|---|---|
//! | `find_by_id` / `find_by_username` | 返回 `Ok(None)` | — |
//! | `require_by_id` | 返回 `Err` | `ErrorCode::UserNotFound` 类的终止型错误 |
//!
//! 「查不到」**不是错误** —— 只有当调用方明确要求「必须有这条记录」时
//! 才用 `require_*`。这让调用方自己决定 404 的时机。

use nexus_core::error::{AppError, ErrorCode};
use nexus_core::id::UserId;
use nexus_model::{NewUser, User, UserCredential};
use sqlx::PgPool;

use crate::error::map_sqlx_error;
use crate::rows::UserRow;

/// 用户仓储。
///
/// # 为什么持有 `PgPool` 而不是 `&PgPool`
///
/// 持有所有权，仓储可以被 `Arc` 共享给多个任务 ——
/// `PgPool` 本身就是克隆廉价、引用计数的句柄。
#[derive(Debug, Clone)]
pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    /// 用连接池构造。
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 底层连接池。
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 按 ID 查用户。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn find_by_id(&self, id: UserId) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, role, avatar_url, status, \
                    created_at, updated_at \
             FROM users WHERE id = $1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(crate::convert::user))
    }

    /// 按登录名查用户。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, role, avatar_url, status, \
                    created_at, updated_at \
             FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(crate::convert::user))
    }

    /// 按登录名查**凭据** —— 只给登录校验路径用。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn find_credential_by_username(
        &self,
        username: &str,
    ) -> Result<Option<UserCredential>, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, role, avatar_url, status, \
                    created_at, updated_at \
             FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(row.map(crate::convert::user_credential))
    }

    /// 按 ID 查用户，**查不到就报错**。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::Unauthorized`]
    ///   （用户不存在与口令错误**必须**返回同一个码 —— 否则能被用来枚举账号）
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn require_by_id(&self, id: UserId) -> Result<User, AppError> {
        self.find_by_id(id)
            .await?
            .ok_or_else(|| AppError::new(ErrorCode::Unauthorized, "用户不存在或口令错误"))
    }

    /// 创建用户。
    ///
    /// `role` / `avatar_url` / `status` **不在这里赋值** ——
    /// 走数据库的 `DEFAULT`（`'user'` / `''` / `1`）。
    /// 两处各写一份默认值必然漂移。
    ///
    /// # 错误
    ///
    /// - 登录名或邮箱重复 → [`ErrorCode::UserExists`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn create(&self, new_user: NewUser) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "INSERT INTO users (username, email, password_hash) \
             VALUES ($1, $2, $3) \
             RETURNING id, username, email, password_hash, role, avatar_url, status, \
                       created_at, updated_at",
        )
        .bind(&new_user.username)
        .bind(&new_user.email)
        .bind(&new_user.password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(crate::convert::user(row))
    }

    /// 更新用户的头像 URL。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::Unauthorized`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn update_avatar_url(&self, id: UserId, avatar_url: &str) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "UPDATE users SET avatar_url = $2, updated_at = now() \
             WHERE id = $1 \
             RETURNING id, username, email, password_hash, role, avatar_url, status, \
                       created_at, updated_at",
        )
        .bind(id.into_inner())
        .bind(avatar_url)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(crate::convert::user)
            .ok_or_else(|| AppError::new(ErrorCode::Unauthorized, "用户不存在或口令错误"))
    }

    /// 更新用户的状态。
    ///
    /// # 错误
    ///
    /// - 记录不存在 → [`ErrorCode::Unauthorized`]
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    pub async fn update_status(&self, id: UserId, status: i16) -> Result<User, AppError> {
        let row = sqlx::query_as::<_, UserRow>(
            "UPDATE users SET status = $2, updated_at = now() \
             WHERE id = $1 \
             RETURNING id, username, email, password_hash, role, avatar_url, status, \
                       created_at, updated_at",
        )
        .bind(id.into_inner())
        .bind(status)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        row.map(crate::convert::user)
            .ok_or_else(|| AppError::new(ErrorCode::Unauthorized, "用户不存在或口令错误"))
    }

    /// 删除用户。
    ///
    /// # 错误
    ///
    /// - 数据库不可用 → [`ErrorCode::StorageUnavailable`]
    ///
    /// **不返回「不存在」错误** —— 删除不存在的记录是幂等的，
    /// 调用方不需要区分「删掉了」与「本来就没有」。
    pub async fn delete(&self, id: UserId) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(map_sqlx_error)?;

        Ok(result.rows_affected() > 0)
    }

    /// 按状态列出用户（分页）。
    ///
    /// 排序键是 `id` —— 自增主键的顺序 ≈ 创建时间顺序，
    /// 且**稳定**（见架构文档 §2.8.1 对 `i64` 底座的说明）。
    /// 用 `created_at` 排序会在同一毫秒内创建的行之间产生**不稳定分页**。
    ///
    /// # 错误
    ///
    /// 数据库不可用 → [`ErrorCode::StorageUnavailable`]。
    pub async fn list_by_status(
        &self,
        status: i16,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>, AppError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT id, username, email, password_hash, role, avatar_url, status, \
                    created_at, updated_at \
             FROM users WHERE status = $1 ORDER BY id LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_sqlx_error)?;

        Ok(rows.into_iter().map(crate::convert::user).collect())
    }
}
