//! 类型化 ID。
//!
//! 架构文档 §2.8 —— 「参考实现混用 `int64` 自增 ID（业务表）和 `string`/UUID（会话、追踪类）」，
//! 本设计**类型化**：每个 ID 成为**独立的类型**。
//!
//! # 为什么必须是独立类型而不是别名
//!
//! ```ignore
//! type UserId = i64;   // ❌ 别名：传错类型不报错
//! struct UserId(i64);  // ✅ 独立类型：传错类型编译失败
//! ```
//!
//! 收益（§2.8）：函数签名上直接看出「这个参数是会话 ID 还是用户 ID」，
//! 传错**编译报错**。参考实现里 `func(ctx, userID, sessionID int64)` 这类签名无法区分。
//!
//! # 三类底层类型（§2.8.1）
//!
//! | 底层 | 为什么 | 本模块中的 ID |
//! |---|---|---|
//! | `i64` | 自增主键，顺序有意义 | 13 种业务实体 ID |
//! | `Uuid` | 客户端可生成 / 跨系统唯一 | `SessionId` |
//! | `String` | 外部系统传入或非实体的技术标识 | `ToolCallId` / `VectorId` / `TraceId` / `InterruptId` |
//!
//! # 序列化约定（重要，附录 C.1 陷阱）
//!
//! ID 序列化为**裸值** —— `SessionId` → UUID 字符串，`UserId` → 数字，
//! `ToolCallId` → 字符串，**不是** `{"value": ...}`。
//!
//! 搞错会导致数据库写入失败 —— 属于「看着像细节、实际会炸」的坑。

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 定义一个 `i64` 底座的 ID 类型。
macro_rules! define_i64_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(i64);

        impl $name {
            /// 构造。
            pub const fn new(value: i64) -> Self {
                Self(value)
            }

            /// 取回底层值。
            pub const fn into_inner(self) -> i64 {
                self.0
            }

            /// 底层值（按引用取，避免消耗 `self`）。
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for i64 {
            fn from(value: $name) -> i64 {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

/// 定义一个 `Uuid` 底座的 ID 类型。
macro_rules! define_uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// 构造。
            pub const fn new(value: Uuid) -> Self {
                Self(value)
            }

            /// 生成一个新的随机 ID（v4）。
            pub fn new_v4() -> Self {
                Self(Uuid::new_v4())
            }

            /// 取回底层值。
            pub const fn into_inner(self) -> Uuid {
                self.0
            }

            /// 底层值（按引用取，避免消耗 `self`）。
            pub const fn get(self) -> Uuid {
                self.0
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Uuid {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

/// 定义一个 `String` 底座的 ID 类型。
macro_rules! define_string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// 构造。
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// 取回底层值。
            pub fn into_inner(self) -> String {
                self.0
            }

            /// 底层值（按引用取）。
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

// ═══════════════════════════════════════════════════════════
// 1xxxx · i64 底座（自增主键，顺序有意义）
// ═══════════════════════════════════════════════════════════

define_i64_id! {
    /// 用户 ID。
    UserId
}
define_i64_id! {
    /// Agent 定义 ID。
    AgentId
}
define_i64_id! {
    /// 消息 ID。
    MessageId
}
define_i64_id! {
    /// 执行记录 ID。
    RunId
}
define_i64_id! {
    /// 工作流定义 ID。
    WorkflowId
}
define_i64_id! {
    /// 工具注册表 ID。
    ToolId
}
define_i64_id! {
    /// 模型配置 ID。
    ModelConfigId
}
define_i64_id! {
    /// 供应商配置 ID。
    ProviderId
}
define_i64_id! {
    /// 向量化配置 ID。
    EmbeddingConfigId
}
define_i64_id! {
    /// 知识库 ID。
    KbId
}
define_i64_id! {
    /// 文档 ID。
    DocumentId
}
define_i64_id! {
    /// 数据集 ID。
    DatasetId
}
define_i64_id! {
    /// 用例 ID。
    CaseId
}

// ═══════════════════════════════════════════════════════════
// 2xxxx · Uuid 底座（客户端可生成 / 跨系统唯一）
// ═══════════════════════════════════════════════════════════

define_uuid_id! {
    /// 会话 ID。
    ///
    /// **是 UUID 而非自增** —— 会话 ID 会返回给客户端，客户端可能**提前**用它发起请求
    /// （前端乐观渲染）。自增 ID 要求先插入才知道值，UUID 可以客户端预生成。
    /// 这是参考实现的设计意图，本设计把它**显式化到类型上**（§2.8.1）。
    SessionId
}

// ═══════════════════════════════════════════════════════════
// 3xxxx · String 底座（外部系统传入或非实体的技术标识）
// ═══════════════════════════════════════════════════════════

define_string_id! {
    /// 单次工具调用 ID —— 外部系统传入。
    ToolCallId
}
define_string_id! {
    /// 向量记录 ID —— 技术标识。
    VectorId
}
define_string_id! {
    /// 追踪 ID —— 技术标识。
    TraceId
}
define_string_id! {
    /// 人工中断标记 ID —— 技术标识。
    InterruptId
}

/// 全部 18 种 ID 类型的**契约清单**。
///
/// 用于契约测试 —— §2.8.1 的表是定稿的，任何增删改都要同步这里，
/// 而测试会立刻发现遗漏。
pub const ID_TYPE_NAMES: &[&str] = &[
    "UserId",
    "SessionId",
    "AgentId",
    "MessageId",
    "RunId",
    "WorkflowId",
    "ToolId",
    "ToolCallId",
    "ModelConfigId",
    "ProviderId",
    "EmbeddingConfigId",
    "KbId",
    "DocumentId",
    "DatasetId",
    "CaseId",
    "VectorId",
    "TraceId",
    "InterruptId",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;

    /// 契约测试 —— §2.8.1 的 18 种 ID 必须齐备，且**类型互不相同**。
    ///
    /// `TypeId` 不同 ⇒ 它们确实是**独立的类型**，而不是 `type UserId = i64` 式的别名。
    /// 这正是 §2.8 设计意图的核心 —— 别名传错不报错。
    #[test]
    fn all_id_types_are_distinct() {
        let ids: Vec<TypeId> = vec![
            TypeId::of::<UserId>(),
            TypeId::of::<SessionId>(),
            TypeId::of::<AgentId>(),
            TypeId::of::<MessageId>(),
            TypeId::of::<RunId>(),
            TypeId::of::<WorkflowId>(),
            TypeId::of::<ToolId>(),
            TypeId::of::<ToolCallId>(),
            TypeId::of::<ModelConfigId>(),
            TypeId::of::<ProviderId>(),
            TypeId::of::<EmbeddingConfigId>(),
            TypeId::of::<KbId>(),
            TypeId::of::<DocumentId>(),
            TypeId::of::<DatasetId>(),
            TypeId::of::<CaseId>(),
            TypeId::of::<VectorId>(),
            TypeId::of::<TraceId>(),
            TypeId::of::<InterruptId>(),
        ];

        assert_eq!(ids.len(), 18, "ID 类型数量与 §2.8.1 不符");
        assert_eq!(ID_TYPE_NAMES.len(), 18, "契约清单长度应为 18");

        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            18,
            "存在重复的 ID 类型 —— 某些是别名而非独立类型"
        );
    }

    #[test]
    fn id_serializes_as_bare_value() {
        // i64 底座 → 数字
        assert_eq!(serde_json::to_string(&UserId::new(42)).unwrap(), "42");
        assert_eq!(serde_json::to_string(&RunId::new(7)).unwrap(), "7");

        // String 底座 → 字符串
        assert_eq!(
            serde_json::to_string(&ToolCallId::new("call-1")).unwrap(),
            "\"call-1\""
        );
        assert_eq!(
            serde_json::to_string(&TraceId::new("trace-1")).unwrap(),
            "\"trace-1\""
        );

        // Uuid 底座 → UUID 字符串（不是 {"value": ...}）
        let uuid = Uuid::new_v4();
        let json = serde_json::to_string(&SessionId::new(uuid)).unwrap();
        assert_eq!(json, format!("\"{uuid}\""));
        assert!(
            !json.contains("value"),
            "SessionId 必须序列化为裸 UUID 字符串：{json}"
        );
    }

    #[test]
    fn id_roundtrips_through_json() {
        let original = SessionId::new_v4();
        let json = serde_json::to_string(&original).unwrap();
        let back: SessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);

        let original = ToolCallId::new("c-1");
        let json = serde_json::to_string(&original).unwrap();
        let back: ToolCallId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn id_conversions_work() {
        assert_eq!(UserId::from(5).into_inner(), 5);
        assert_eq!(i64::from(UserId::new(5)), 5);
        assert_eq!(ToolCallId::from("x").as_str(), "x");
        assert_eq!(String::from(ToolCallId::new("x")), "x");

        let uuid = Uuid::new_v4();
        assert_eq!(SessionId::from(uuid).into_inner(), uuid);
    }

    #[test]
    fn id_display_is_the_bare_value() {
        assert_eq!(UserId::new(42).to_string(), "42");
        assert_eq!(ToolCallId::new("c-1").to_string(), "c-1");
    }

    #[test]
    fn session_id_can_be_generated_client_side() {
        // UUID 的意义就在这里：客户端可以**提前**生成，无需先插入。
        let a = SessionId::new_v4();
        let b = SessionId::new_v4();
        assert_ne!(a, b);
    }
}
