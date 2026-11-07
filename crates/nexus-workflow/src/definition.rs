//! **冻结定义** —— 工作流定义与一次运行之间的**绑定**。
//!
//! 架构文档 §2.5.2：「参考实现已有 `definition` JSONB + `definition_hash` +
//! `engine_version` 三件套。本设计继承，并明确其语义」。
//!
//! # 为什么需要「冻结」
//!
//! 工作流定义是**可变的** —— 用户在跑的过程中可能改它。
//! 但一次运行的语义必须**冻结**在启动那一刻：
//!
//! | 字段 | 回答的问题 | 不匹配时 |
//! |---|---|---|
//! | `canonical` | 「这次跑的到底是什么」 | 快照本身就是答案 |
//! | `hash` | 「当前定义的哈希和运行记录里的一致吗」 | 检测到外部修改 → 拒绝续跑 |
//! | `engine_version` | 「用哪个版本的引擎解释这个定义」 | 版本不兼容 → 需要迁移路径 |
//!
//! # 为什么哈希必须**键序无关**
//!
//! 同一份定义的 JSON 键序不同、空白不同，就产生不同哈希 ——
//! 那么「检测运行中的定义是否被改过」这个功能**就是假的**。
//! 规范化规则见架构文档 §2.5.3，**已定稿**。
//!
//! # 为什么**复用** `nexus_core::canonical`
//!
//! 规范化规则是**已定稿**的，而 `nexus-core` 里已有**实测过**的实现
//! （架构文档附录 B：「规范化 JSON 键序无关、数组序敏感」）。
//!
//! 再写一份 = 两份规范化 = **哈希漂移**的入口 ——
//! 一处用 `BTreeMap` 排序、另一处用 `IndexMap`，哈希就对不上，
//! 而「检测到定义被改过」会**误报**。
//!
//! 放在 `nexus-core` 正是因为 `nexus-workflow` 与 `nexus-execution`
//! **互相不能依赖**（否则成环）—— `nexus-core` 是依赖图的根。
//!
//! # 与 `engine_version` 的关系
//!
//! 哈希相同**不代表**能续跑：同一份定义在**不同引擎版本**下的语义可能不同。
//! 所以版本是**独立**的一维，不塞进哈希里。

use nexus_core::canonical::{CanonicalizeError, canonical_hash_hex, canonicalize};
use nexus_core::error::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};

/// **引擎版本** —— 「用哪个版本的引擎解释这个定义」。
///
/// # 为什么要**类型化**
///
/// 它要与 `definition_hash` 配对出现在**检查点**的行里
/// （`execution_checkpoints.engine_version VARCHAR(32)`）。
/// 裸 `String` 传参时，「把 `hash` 传进 `engine_version` 的位置」编译不过。
///
/// # 为什么是 `String` 底座
///
/// 它是**引擎的版本标识**（如 `"1.0.0"`），不是数据库实体的自增主键 ——
/// 对应架构文档 §2.8.1 的第三类底层类型（`String`：
/// 「外部系统传入或非实体的技术标识」）。
///
/// ⚠️ **它不在架构文档 §2.8.1 的 18 种 ID 类型清单里** ——
///
/// 那张清单列的是**实体 ID 与技术标识 ID**（`UserId` / `SessionId` / `RunId` …），
/// 且 `crates/nexus-core/tests/contract.rs` 的 `contract_1_eighteen_id_types_are_all_distinct`
/// **硬断言**数量恰好是 18。引擎版本是**解释语义的那一维**，
/// 不标识任何实体 —— 它不进那张清单，也不进 `nexus-core::id`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EngineVersion(String);

impl EngineVersion {
    /// 构造。
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }

    /// 取回底层值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EngineVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// **规范化后的定义** —— 冻结快照的**本体**。
///
/// # 为什么是**字节**而不是 `serde_json::Value`
///
/// 规范化后的字节是**哈希的输入** —— 存字节就存了「被哈希的那个东西」，
/// 不存 Value 就避免了「存的值和哈希的值不是同一个」的可能。
///
/// ⚠️ 这是**不可变**的 —— 冻结定义的语义就是「不再变」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalJson(Vec<u8>);

impl CanonicalJson {
    /// 构造（**不**校验是否已规范化 —— 调用方用 [`FrozenDefinition::new`] 走完整路径）。
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// 取回字节。
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// **定义哈希** —— 64 位小写十六进制（`CHAR(64)` 列的形状）。
///
/// # 为什么是**字符串**而不是 `[u8; 32]`
///
/// `definition_hash` 列是 `CHAR(64)` —— 存进数据库的就是这 64 个字符。
/// 返回**字节**让调用方自己决定编码，就多了一种「同一个哈希有两种表示」的坑
/// （这正是 `nexus_core::canonical` 里 `canonical_hash` 与
/// `canonical_hash_hex` 分开的原因）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DefinitionHash(String);

impl DefinitionHash {
    /// 构造。
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }

    /// 取回底层值。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DefinitionHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// **冻结的工作流定义** —— run 一旦创建，就与这个快照绑定。
///
/// 架构文档 §2.5.2 的三件套。
///
/// # 为什么三件套**各自独立**
///
/// | 字段 | 管什么 | 变了会怎样 |
/// |---|---|---|
/// | `canonical` | 这次跑的**内容** | 内容变了 → 是另一次运行 |
/// | `hash` | 内容**有没有**变过 | 哈希变了 → 定义被改过 → 拒绝续跑 |
/// | `engine_version` | 用**哪版引擎**解释 | 版本变了 → 语义可能不同 → 需要迁移路径 |
///
/// 把三者塞进一个哈希里（「哈希里带上引擎版本」）看似省事 ——
/// 但那样就**分不清**「定义被改了」与「引擎升级了」，
/// 而这两件事的**处理方式完全不同**（前者拒绝续跑，后者要迁移路径）。
///
/// # 不变量
///
/// `hash` **一定**是 `canonical` 的哈希。
/// 这条不变量由**构造方式**保证 —— 字段是**私有**的，
/// 外部**造不出**「hash 与 canonical 不配」的 `FrozenDefinition`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenDefinition {
    /// 规范化后的定义（规范化保证哈希稳定）。
    canonical: CanonicalJson,
    /// 定义哈希 —— 用于检测「运行中的定义是否被改过」。
    hash: DefinitionHash,
    /// 引擎版本 —— 定义语义随引擎演进而变，必须记录。
    engine_version: EngineVersion,
}

impl FrozenDefinition {
    /// 从一份**原始**定义 JSON 冻结。
    ///
    /// # 做了什么
    ///
    /// 1. **规范化**（§2.5.3）—— 键序无关、数组序敏感、数字最短往返；
    /// 2. **算哈希** —— SHA-256，64 位十六进制。
    ///
    /// ⚠️ 规范化与哈希用的是**同一份** `canonical` 字节 ——
    /// 不是「先算哈希再规范化」。顺序反了就会出现
    /// 「规范化了但哈希是原始值的」这种分裂。
    ///
    /// # 错误
    ///
    /// - 数字无法用有限的十进制表示（`NaN` / `inf`）→
    ///   [`ErrorCode::InvalidParam`]（参数非法）
    /// - 原始定义**不是合法 JSON** → [`ErrorCode::WorkflowDsl`]
    ///
    /// # 为什么规范化失败映射到 `InvalidParam` 而**不是** `Internal`
    ///
    /// 规范化失败说明**输入有问题**（`NaN` / `inf` 根本不是合法 JSON 数字）——
    /// 那是调用方的错，归**参数段**（1xxxx），不是系统段（9xxxx）。
    pub fn new(raw: &serde_json::Value, engine_version: EngineVersion) -> Result<Self, AppError> {
        let bytes = canonicalize(raw).map_err(|e: CanonicalizeError| {
            AppError::new(ErrorCode::InvalidParam, format!("定义无法规范化：{e}"))
        })?;
        let hash = canonical_hash_hex(raw).map_err(|e: CanonicalizeError| {
            AppError::new(ErrorCode::InvalidParam, format!("定义无法计算哈希：{e}"))
        })?;
        Ok(Self {
            canonical: CanonicalJson::from_bytes(bytes),
            hash: DefinitionHash::new(hash),
            engine_version,
        })
    }

    /// 规范化后的定义。
    pub fn canonical(&self) -> &CanonicalJson {
        &self.canonical
    }

    /// 定义哈希。
    pub fn hash(&self) -> &DefinitionHash {
        &self.hash
    }

    /// 引擎版本。
    pub fn engine_version(&self) -> &EngineVersion {
        &self.engine_version
    }
}
