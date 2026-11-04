//! 会话仓储的集成测试 —— 连**真实 PostgreSQL**。
//!
//! # 覆盖的行为层不变量
//!
//! | 测试 | 断言的不变量 |
//! |---|---|
//! | `create_with_client_generated_id` | 会话 ID **可以由客户端预生成**（§2.8.1） |
//! | `create_applies_database_default_title` | 标题默认值由数据库提供 |
//! | `find_owned_rejects_other_users_session` | 归属校验**下沉到 SQL**（不泄露资源存在性） |
//! | `require_by_id_reports_session_not_found` | 404 用**专门的**业务码 |
//! | `delete_cascades_to_messages` | 外键级联行为与迁移一致 |

mod common;

use nexus_core::error::ErrorCode;
use nexus_core::id::{SessionId, UserId};
use nexus_model::{NewSession, NewUser};
use nexus_repo::{SessionRepository, UserRepository};
use uuid::Uuid;

/// 造一个唯一的用户。
async fn create_user(repo: &UserRepository) -> UserId {
    let created = repo
        .create(NewUser {
            username: format!("test_{}", Uuid::new_v4().simple()),
            email: format!("{}@example.test", Uuid::new_v4().simple()),
            password_hash: "argon2id$fake-hash-for-shape-test".to_string(),
        })
        .await
        .expect("创建用户失败");
    created.id
}

/// 造一个新的会话输入（**客户端预生成 ID**）。
fn new_session(user_id: UserId) -> NewSession {
    NewSession {
        id: SessionId::new_v4(),
        user_id,
        agent_id: None,
        title: None,
    }
}

#[tokio::test]
async fn create_with_client_generated_id() {
    let Some(pool) = common::test_pool("create_with_client_generated_id")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool);
    let user_id = create_user(&users).await;

    // ID 在插入**之前**就生成好了 —— 这正是 §2.8.1 选 UUID 的理由。
    let input = new_session(user_id);
    let requested_id = input.id;

    let created = sessions.create(input).await.expect("创建会话失败");

    assert_eq!(created.id, requested_id, "会话 ID 必须尊重调用方给定的值");
    assert_eq!(created.user_id, user_id);
    assert_eq!(created.status, "active");
    assert!(!created.pinned);
    assert_eq!(created.message_count, 0);

    let found = sessions
        .find_by_id(created.id)
        .await
        .expect("按 ID 查询失败")
        .expect("刚创建的会话应能查到");
    assert_eq!(found.id, created.id);
    assert_eq!(found.user_id, user_id);
}

#[tokio::test]
async fn create_applies_database_default_title() {
    let Some(pool) = common::test_pool("create_applies_database_default_title")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool);
    let user_id = create_user(&users).await;

    let created = sessions
        .create(new_session(user_id))
        .await
        .expect("创建会话失败");

    // 迁移 6 的 DEFAULT '新对话'。
    assert_eq!(created.title, "新对话");

    let titled = sessions
        .create(NewSession {
            id: SessionId::new_v4(),
            user_id,
            agent_id: None,
            title: Some("自定义标题".to_string()),
        })
        .await
        .expect("创建会话失败");
    assert_eq!(titled.title, "自定义标题");
}

#[tokio::test]
async fn find_owned_rejects_other_users_session() {
    let Some(pool) = common::test_pool("find_owned_rejects_other_users_session")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool);
    let owner = create_user(&users).await;
    let stranger = create_user(&users).await;

    let created = sessions
        .create(new_session(owner))
        .await
        .expect("创建会话失败");

    // 归属正确 → 能查到。
    assert!(
        sessions
            .find_owned(created.id, owner)
            .await
            .expect("查询失败")
            .is_some()
    );

    // 归属不对 → **查不到**（`None`，不是错误）。
    // 关键：**不能**返回「存在但无权访问」—— 那能泄露资源存在性。
    assert!(
        sessions
            .find_owned(created.id, stranger)
            .await
            .expect("查询失败")
            .is_none()
    );
}

#[tokio::test]
async fn require_by_id_reports_session_not_found() {
    let Some(pool) = common::test_pool("require_by_id_reports_session_not_found")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let sessions = SessionRepository::new(pool);

    let err = sessions
        .require_by_id(SessionId::new_v4())
        .await
        .expect_err("查不到时应当报错");

    // 会话查不到用**专门的**业务码，不是 Unauthorized。
    assert_eq!(err.user_facing().code, ErrorCode::SessionNotFound);
}

#[tokio::test]
async fn update_title_and_pinned() {
    let Some(pool) = common::test_pool("update_title_and_pinned")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool);
    let user_id = create_user(&users).await;

    let created = sessions
        .create(new_session(user_id))
        .await
        .expect("创建会话失败");

    let renamed = sessions
        .update_title(created.id, "改过的标题")
        .await
        .expect("更新标题失败");
    assert_eq!(renamed.title, "改过的标题");

    let pinned = sessions
        .set_pinned(created.id, true)
        .await
        .expect("置顶失败");
    assert!(pinned.pinned);
}

#[tokio::test]
async fn list_by_user_returns_only_own_sessions() {
    let Some(pool) = common::test_pool("list_by_user_returns_only_own_sessions")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool);
    let mine = create_user(&users).await;
    let theirs = create_user(&users).await;

    let my_session = sessions
        .create(new_session(mine))
        .await
        .expect("创建会话失败");
    sessions
        .create(new_session(theirs))
        .await
        .expect("创建会话失败");

    let listed = sessions
        .list_by_user(mine, 100, 0)
        .await
        .expect("分页查询失败");

    assert!(
        listed.iter().all(|s| s.user_id == mine),
        "列表里出现了别人的会话"
    );
    assert!(
        listed.iter().any(|s| s.id == my_session.id),
        "列表里应有刚创建的会话"
    );
}

#[tokio::test]
async fn delete_fails_while_messages_exist() {
    let Some(pool) = common::test_pool("delete_fails_while_messages_exist")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let users = UserRepository::new(pool.clone());
    let sessions = SessionRepository::new(pool.clone());
    let user_id = create_user(&users).await;

    let created = sessions
        .create(new_session(user_id))
        .await
        .expect("创建会话失败");

    // 插一条消息（`messages.session_id → sessions.id` 是外键）。
    sqlx::query("INSERT INTO messages (session_id, role, content) VALUES ($1, 'user', '你好')")
        .bind(created.id.into_inner())
        .execute(&pool)
        .await
        .expect("插入消息失败");

    // 迁移 6 **没有**声明 `ON DELETE CASCADE` —— 所以有消息的会话删不掉。
    // 这条断言记录**当前 schema 的真实行为**，不是 bug：
    // 删会话前必须先删消息（或补一条级联迁移，那是破坏性契约变更）。
    let err = sessions
        .delete(created.id)
        .await
        .expect_err("有消息的会话不应被删掉");

    assert_eq!(err.user_facing().code, ErrorCode::InvalidParam);

    // 先删消息，再删会话 —— 顺序对了就成功。
    sqlx::query("DELETE FROM messages WHERE session_id = $1")
        .bind(created.id.into_inner())
        .execute(&pool)
        .await
        .expect("删除消息失败");

    assert!(
        sessions.delete(created.id).await.expect("删除会话失败"),
        "消息清空后应能删除会话"
    );
}
