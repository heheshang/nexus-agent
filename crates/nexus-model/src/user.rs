//! 用户领域类型。
//!
//! **纯 DTO** —— 不含任何数据库痕迹（无 `FromRow`、无 `sqlx::`）。
//! 架构文档 §4.2 差异 2 把「领域类型」与「数据库列布局」分成两个 crate：
//! 领域类型在 `nexus-model`，行类型在 `nexus-repo`。
//!
//! # 为什么 `User` 里没有 `password_hash`
//!
//! 口令散列是**凭据**，不是领域属性 —— 它只服务于登录校验。
//! 把它放进 `User` 意味着任何拿到 `User` 的地方都能顺手读到散列，
//! 而 `User` 是要直接序列化给 API 层的。
//!
//! 所以口令散列单独放在 [`UserCredential`] 里，并且**不实现 `Serialize`** ——
//! 从类型上杜绝「不小心把散列发给客户端」。

use chrono::{DateTime, Utc};
use nexus_core::id::UserId;
use serde::{Deserialize, Serialize};

/// 用户领域类型。
///
/// 与 `users` 表一一对应，但**不含** `password_hash`（见模块文档）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// 用户 ID。
    pub id: UserId,
    /// 登录名（唯一）。
    pub username: String,
    /// 邮箱（唯一）。
    pub email: String,
    /// 角色。
    ///
    /// **保持 `String` 而非枚举** —— Phase 0 的迁移只给了
    /// `DEFAULT 'user'`，取值集合尚未定稿（架构文档未列角色枚举）。
    /// 提前固化枚举等于**用实现替换开放问题**。
    pub role: String,
    /// 头像 URL。
    pub avatar_url: String,
    /// 状态（`SMALLINT`）。
    ///
    /// 同 [`User::role`]，取值集合未定稿，保持原始整数。
    /// 参考实现用 `status = 1` 过滤「有效记录」，但完整取值集合未见于文档。
    pub status: i16,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 最后更新时间。
    pub updated_at: DateTime<Utc>,
}

/// 用户凭据 —— 只在登录校验路径上使用。
///
/// **不实现 `Serialize`** —— 散列绝不能序列化出去。
/// `Debug` 也是手写的：派生实现会把散列原文打进日志。
#[derive(Clone, PartialEq, Eq)]
pub struct UserCredential {
    /// 所属用户。
    pub user_id: UserId,
    /// 口令散列。
    pub password_hash: String,
}

impl std::fmt::Debug for UserCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserCredential")
            .field("user_id", &self.user_id)
            .field("password_hash", &"[REDACTED]")
            .finish()
    }
}

/// 新建用户的输入。
///
/// 只暴露「创建时必填」的字段：`role` / `avatar_url` / `status`
/// 由数据库默认值兜底（`DEFAULT 'user'` / `DEFAULT ''` / `DEFAULT 1`），
/// 不在领域层重复一份默认值 —— 两处默认值必然漂移。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUser {
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: String,
    /// 口令散列（**调用方负责先做哈希**）。
    pub password_hash: String,
}
