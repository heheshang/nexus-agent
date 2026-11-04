//! 用户仓储的集成测试 —— 连**真实 PostgreSQL**。
//!
//! # 覆盖的行为层不变量
//!
//! | 测试 | 断言的不变量 |
//! |---|---|
//! | `create_then_find_by_id_roundtrips` | 行类型 → 领域类型的转换**无损** |
//! | `create_applies_database_defaults` | 默认值由**数据库**提供，不在领域层重复一份 |
//! | `credential_is_separate_from_domain_user` | 凭据**不混进**领域类型 |
//! | `create_rejects_duplicate_username` | 唯一约束 → `UserExists` |
//! | `find_missing_user_returns_none` | 「查不到」**不是错误** |
//! | `delete_is_idempotent` | 删除幂等 |
//!
//! # 数据隔离
//!
//! 每个测试用**唯一的用户名/邮箱**（后缀是随机 UUID），
//! 避免与并行执行的测试、或测试库里残留的数据冲突。
//! 测试**不删除**自己插入的数据 —— 它用随机名字，不会干扰别人。

mod common;

use nexus_core::error::ErrorCode;
use nexus_core::id::UserId;
use nexus_model::{NewUser, User};
use nexus_repo::UserRepository;
use uuid::Uuid;

/// 造一个唯一的登录名。
fn unique_username() -> String {
    format!("test_{}", Uuid::new_v4().simple())
}

/// 造一个唯一的邮箱。
fn unique_email() -> String {
    format!("{}@example.test", Uuid::new_v4().simple())
}

/// 造一个**独一无二**的 `status` 值。
///
/// `status` 列是 `SMALLINT`（`-32768..=32767`）—— 取值空间有限，
/// 不能用 UUID 那种「无限」的唯一值。
///
/// 取值策略：从 `SMALLINT` 的**负半区**随机取。
/// 正半区（`0..=32767`）留给真实业务语义（`0` = 禁用、`1` = 有效），
/// 负半区在业务上无意义 —— 用它做测试隔离，**不会**与真实数据混淆。
///
/// 碰撞概率：每次测试从 32768 个值里随机取一个，
/// 跑 1000 次测试的碰撞概率约 1.5%。对「隔离维度」来说足够 ——
/// 真的撞上了也会表现为「多出一行」，而不是静默的假通过。
fn unique_status() -> i16 {
    // `i16::MIN` = -32768。用 `rem_euclid` 映射到 `[-32768, -1]`。
    let raw = (Uuid::new_v4().as_u128() % 32768) as i32;
    (-32768 + raw) as i16
}

/// 建一个新的用户输入。
fn new_user() -> NewUser {
    NewUser {
        username: unique_username(),
        email: unique_email(),
        password_hash: "argon2id$fake-hash-for-shape-test".to_string(),
    }
}

#[tokio::test]
async fn create_then_find_by_id_roundtrips() {
    let Some(pool) = common::test_pool("create_then_find_by_id_roundtrips")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let input = new_user();

    let created: User = repo.create(input.clone()).await.expect("创建用户失败");

    // 领域类型不包含 password_hash —— 这是刻意的（凭据分离）。
    assert_eq!(created.username, input.username);
    assert_eq!(created.email, input.email);
    assert!(created.id.into_inner() > 0, "自增主键应为正数");

    let found = repo
        .find_by_id(created.id)
        .await
        .expect("按 ID 查询失败")
        .expect("刚创建的用户应能查到");

    assert_eq!(found.id, created.id);
    assert_eq!(found.username, input.username);
    assert_eq!(found.email, input.email);
    assert_eq!(found.created_at, created.created_at);
    assert_eq!(found.updated_at, created.updated_at);
}

#[tokio::test]
async fn create_applies_database_defaults() {
    let Some(pool) = common::test_pool("create_applies_database_defaults")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let created = repo.create(new_user()).await.expect("创建用户失败");

    // 默认值来自迁移 1 的 DEFAULT 子句 —— 领域层不重复定义它们。
    assert_eq!(created.role, "user");
    assert_eq!(created.avatar_url, "");
    assert_eq!(created.status, 1);
}

#[tokio::test]
async fn credential_is_separate_from_domain_user() {
    let Some(pool) = common::test_pool("credential_is_separate_from_domain_user")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let input = new_user();
    let created = repo.create(input.clone()).await.expect("创建用户失败");

    let credential = repo
        .find_credential_by_username(&input.username)
        .await
        .expect("查询凭据失败")
        .expect("刚创建的用户应能查到凭据");

    assert_eq!(credential.user_id, created.id);
    assert_eq!(credential.password_hash, input.password_hash);

    // 凭据的 Debug **不能**泄露散列 —— 它会被打进日志。
    let debug_text = format!("{credential:?}");
    assert!(
        !debug_text.contains("argon2id"),
        "凭据的 Debug 泄露了口令散列：{debug_text}"
    );
    assert!(
        debug_text.contains("[REDACTED]"),
        "凭据的 Debug 应当把散列替换为 [REDACTED]：{debug_text}"
    );
}

#[tokio::test]
async fn create_rejects_duplicate_username() {
    let Some(pool) = common::test_pool("create_rejects_duplicate_username")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let input = new_user();

    repo.create(input.clone()).await.expect("首次创建应成功");

    let err = repo
        .create(NewUser {
            username: input.username.clone(),
            email: unique_email(),
            password_hash: "argon2id$another".to_string(),
        })
        .await
        .expect_err("重复的登录名应当被拒绝");

    // 唯一约束冲突 → 业务码 UserExists（409，终止型）。
    assert_eq!(err.user_facing().code, ErrorCode::UserExists);
    assert_eq!(err.retry_policy(), nexus_core::error::RetryPolicy::Terminal);
}

#[tokio::test]
async fn find_missing_user_returns_none() {
    let Some(pool) = common::test_pool("find_missing_user_returns_none")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);

    // 「查不到」是 `Ok(None)`，**不是**错误。
    let missing = repo
        .find_by_id(UserId::new(i64::MAX))
        .await
        .expect("按 ID 查询不应失败");
    assert!(missing.is_none());

    let missing_by_name = repo
        .find_by_username("definitely-not-a-real-user-xyz")
        .await
        .expect("按登录名查询不应失败");
    assert!(missing_by_name.is_none());
}

#[tokio::test]
async fn require_by_id_errors_when_missing() {
    let Some(pool) = common::test_pool("require_by_id_errors_when_missing")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);

    let err = repo
        .require_by_id(UserId::new(i64::MAX))
        .await
        .expect_err("查不到时应当报错");

    // 用 Unauthorized（而非「用户不存在」）—— 否则能被用来枚举账号。
    assert_eq!(err.user_facing().code, ErrorCode::Unauthorized);
}

#[tokio::test]
async fn update_avatar_url_and_status() {
    let Some(pool) = common::test_pool("update_avatar_url_and_status")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let created = repo.create(new_user()).await.expect("创建用户失败");

    let updated = repo
        .update_avatar_url(created.id, "https://example.test/avatar.png")
        .await
        .expect("更新头像失败");
    assert_eq!(updated.avatar_url, "https://example.test/avatar.png");
    assert_eq!(updated.id, created.id);

    let disabled = repo
        .update_status(created.id, 0)
        .await
        .expect("更新状态失败");
    assert_eq!(disabled.status, 0);
}

#[tokio::test]
async fn delete_is_idempotent() {
    let Some(pool) = common::test_pool("delete_is_idempotent")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);
    let created = repo.create(new_user()).await.expect("创建用户失败");

    assert!(
        repo.delete(created.id).await.expect("删除失败"),
        "首次删除应成功"
    );
    assert!(
        !repo.delete(created.id).await.expect("重复删除不应失败"),
        "重复删除应返回 false（幂等），而不是报错"
    );
}

#[tokio::test]
async fn list_by_status_paginates() {
    let Some(pool) = common::test_pool("list_by_status_paginates")
        .await
        .expect("连接测试库失败")
    else {
        return;
    };

    let repo = UserRepository::new(pool);

    // 用一个**独一无二**的 status 值 —— 不能用固定值（如 7）。
    //
    // 固定值会让测试依赖「库是干净的」这个假设：测试库里残留的历史数据
    // （前几轮跑测试留下的、或并行跑的其它测试）会挤占分页首页，
    // 导致「刚创建的用户不在结果里」的**假失败**。
    //
    // status 是 `SMALLINT`，取值范围有限 —— 所以用**随机**值，
    // 碰撞概率随测试次数增长而趋近于零。
    //
    // ⚠️ 这不是「测试太严格」，而是**隔离维度选错了**：
    // 列表接口的分页语义天然受同状态的历史行影响，
    // 隔离维度必须落在「被查询的那个键」上。
    let status = unique_status();

    let mut ids = Vec::new();
    for _ in 0..2 {
        let created = repo.create(new_user()).await.expect("创建用户失败");
        repo.update_status(created.id, status)
            .await
            .expect("更新状态失败");
        ids.push(created.id);
    }

    let page = repo
        .list_by_status(status, 10, 0)
        .await
        .expect("分页查询失败");

    assert_eq!(
        page.len(),
        2,
        "该 status 下的用户应恰好是刚创建的两个，实际 {}",
        page.len()
    );
    for user in &page {
        assert_eq!(user.status, status);
    }
    assert!(
        page.iter().all(|u| ids.contains(&u.id)),
        "分页结果应包含刚创建的两个用户"
    );
}
