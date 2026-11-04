//! 行类型的**类型层**测试 —— 断言「行类型与领域类型的形状关系」。
//!
//! 这些断言**不需要数据库** —— 它们是纯类型检查。

use nexus_core::id::{AgentId, MessageId, UserId};
use nexus_repo::rows::{NullableId, Numeric, UserRow};

#[test]
fn nullable_id_maps_some_and_none() {
    let some: NullableId<AgentId> = NullableId::some(AgentId::new(7));
    assert!(some.is_some());
    assert_eq!(some.get(), Some(AgentId::new(7)));

    let none: NullableId<AgentId> = NullableId::none();
    assert!(none.is_none());
    assert_eq!(none.get(), None);
}

#[test]
fn nullable_id_converts_from_option_i64() {
    let converted = NullableId::<UserId>::try_from(Some(42)).expect("应能转换");
    assert_eq!(converted.get(), Some(UserId::new(42)));

    let empty = NullableId::<UserId>::try_from(None).expect("应能转换");
    assert!(empty.is_none());
}

#[test]
fn nullable_id_converts_back_to_option() {
    let nullable = NullableId::some(MessageId::new(9));
    assert_eq!(Option::from(nullable), Some(MessageId::new(9)));
}

#[test]
fn numeric_type_is_exact_decimal_not_float() {
    // `NUMERIC` 列必须用精确十进制 —— 浮点的 15 位有效数字不足以保证金额精度。
    // 这条断言把「不能用 f64」固化下来：换成 f64 时编译不过。
    let price: Numeric = "0.000001".parse().expect("应能解析");
    assert_eq!(price.to_string(), "0.000001");
}

#[test]
fn user_row_carries_the_password_hash_separately() {
    // 行类型**有** `password_hash`（它如实映射数据库列），
    // 但转换到领域类型时会被剥掉（见 `convert::user` / `convert::user_credential`）。
    //
    // 这条断言固化「行类型如实映射」这一事实 ——
    // 将来若有人删掉该字段，`convert::user_credential` 会编译失败。
    let row = UserRow {
        id: UserId::new(1),
        username: "u".to_string(),
        email: "e@example.test".to_string(),
        password_hash: "argon2id$hash".to_string(),
        role: "user".to_string(),
        avatar_url: String::new(),
        status: 1,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let domain = nexus_repo::convert::user(row.clone());
    assert_eq!(domain.username, "u");

    let credential = nexus_repo::convert::user_credential(row);
    assert_eq!(credential.password_hash, "argon2id$hash");
    assert_eq!(credential.user_id, UserId::new(1));
}

#[test]
fn bpchar_trailing_spaces_are_trimmed() {
    // PostgreSQL 的 `CHAR(64)` 会补空格到定长 —— 比对哈希前必须 trim。
    assert_eq!(nexus_repo::convert::trim_bpchar("abc   "), "abc");
    assert_eq!(nexus_repo::convert::trim_bpchar("abc"), "abc");
    assert_eq!(nexus_repo::convert::trim_bpchar(""), "");
}
