//! 行类型 —— **数据库列布局**的映射。
//!
//! # 职责边界（架构文档 §4.2 差异 2）
//!
//! 行类型**只**在 `nexus-repo` 定义（`#[derive(FromRow)]`），
//! 领域类型在 `nexus-model`。两者分开，改表结构不会波及 API 契约。
//!
//! `nexus-model` 保持**零数据库痕迹** —— 由 `scripts/check-deps.sh` 规则 6 强制。
//!
//! # 为什么用 `query_as` + `FromRow` 而不是 `query!` 编译期宏
//!
//! 架构文档 §3.9.3 已定稿：项目处于重建期，schema 会频繁变；
//! `FOR UPDATE SKIP LOCKED` / `ON CONFLICT` / `FILTER(WHERE ...)` 这类关键 SQL
//! 本来就要写原生语句，编译期宏对它们支持有限。
//!
//! **代价**：字段名写错要到运行时才炸。缓解手段是**启动期 smoke check**
//! （`nexus-infra` 的 `assert_phase0_tables`）。
//!
//! # 类型化 ID 与行类型的关系
//!
//! 行类型里的 ID 字段**直接是 `nexus_core` 的类型化 ID**（`UserId` / `SessionId` …），
//! 而不是裸 `i64`。实现方式是 `#[sqlx(try_from = "i64")]` ——
//! sqlx 先按数据库原生类型解码，再转成类型化 ID。
//!
//! 这样做的收益：**行类型不可能把 `user_id` 填进 `agent_id` 字段**
//! —— 类型系统直接挡住。而裸 `i64` 的行类型是挡不住的。
//!
//! # 可空外键：`NullableId`
//!
//! 可空外键（`agent_id BIGINT REFERENCES agents(id)`）用
//! [`NullableId`] 而不是 `Option<UserId>`。
//!
//! 原因有二：
//!
//! 1. **sqlx 的 `try_from` 要求 `TryFrom` 实现**，
//!    而 std **不提供** `Option<i64>` → `Option<UserId>` 的转换
//!    （`Option<T>: From<Option<U>>` 只在 `U == T` 时成立）。
//! 2. **孤儿规则挡住了直接补 impl** ——
//!    `impl From<Option<i64>> for Option<UserId>` 的自类型
//!    `Option<UserId>` 既不是本 crate 的类型，`Option` 也不是 `#[fundamental]`，
//!    所以编译器会拒绝。
//!
//! [`NullableId`] 是**本 crate 的**类型，孤儿规则允许为它实现 `TryFrom`。
//!
//! # 哪些表**不在**本模块
//!
//! `workflow_runs` 与 `workflow_step_runs` **没有**行类型 ——
//! 架构文档 §4.2 差异 1 与 `docs/operations/p2-plan.md` 的红线 3 都规定：
//! `workflow_runs` 由 `nexus-execution` **独占**，行类型也归它。
//! `nexus-repo` 引用它就是偏离架构决策。
//!
//! # 裸 `i64` 主键
//!
//! `mcp_servers` / `workflow_versions` / `execution_operations` / `execution_attempts` /
//! `execution_approvals` / `execution_outbox` 的主键**没有**对应的类型化 ID ——
//! 架构文档 §2.8.1 的 18 种 ID 是**定稿**契约，不得增删。
//! 这些表用裸 `i64` / `String` 主键，并在此显式说明。

use nexus_core::id::{
    AgentId, MessageId, ModelConfigId, ProviderId, RunId, SessionId, ToolId, UserId, WorkflowId,
};
use serde_json::Value;
use sqlx::FromRow;
use sqlx::types::Json;

/// `NUMERIC` 列的 Rust 类型。
///
/// PG 的 `NUMERIC` 在**二进制协议**下有自己的编码（不是 IEEE754），
/// sqlx 需要 `rust_decimal` feature 才能解码 —— 否则 `f64` 的类型兼容性检查
/// 就会直接失败（`FLOAT8 ≠ NUMERIC`）。
///
/// 精确十进制比 `f64` 更适合价格字段，所以走 `rust_decimal` 而不是 `f64`。
pub type Numeric = sqlx::types::Decimal;

/// **可空**的类型化 ID。
///
/// 用于可空外键列（`agent_id` / `fallback_model_config_id` / `decided_by` …）。
///
/// # 为什么不是 `Option<UserId>`
///
/// 见[模块文档](self)的「可空外键」小节 —— 简言之：
/// `sqlx` 的 `try_from` 要求 `TryFrom` 实现，而 std 不提供
/// `Option<i64>` → `Option<UserId>` 的转换；直接补 impl 会被孤儿规则拒绝。
///
/// # 用法
///
/// ```ignore
/// row.agent_id.get()      // Option<AgentId>
/// row.agent_id.is_none()  // bool
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NullableId<T>(Option<T>);

impl<T> NullableId<T> {
    /// 构造「有值」。
    pub fn some(value: T) -> Self {
        Self(Some(value))
    }

    /// 构造「无值」。
    pub fn none() -> Self {
        Self(None)
    }

    /// 取回内部的可空值。
    pub fn get(self) -> Option<T> {
        self.0
    }

    /// 是否为空。
    pub fn is_none(self) -> bool {
        self.0.is_none()
    }

    /// 是否非空。
    pub fn is_some(self) -> bool {
        self.0.is_some()
    }
}

impl<T> From<NullableId<T>> for Option<T> {
    fn from(value: NullableId<T>) -> Self {
        value.0
    }
}

/// 从 `Option<i64>` 构造 [`NullableId`]。
///
/// 泛型 impl —— 只要目标 ID 类型能从 `i64` 转（`nexus_core` 的 18 种 ID 全部可以），
/// 就能用。
///
/// # 为什么**没有** `From<Option<T>> for NullableId<T>`
///
/// 因为那个 impl 会让 `Option<i64>: Into<NullableId<i64>>` 成立，
/// 于是 std 的 blanket impl（`impl<T, U> TryFrom<U> for T where U: Into<T>`）
/// 与下面这个 `TryFrom` impl **冲突**（E0119）。
///
/// 所以构造请用 [`NullableId::some`] / [`NullableId::none`]。
impl<T> TryFrom<Option<i64>> for NullableId<T>
where
    T: TryFrom<i64>,
{
    type Error = T::Error;

    fn try_from(value: Option<i64>) -> Result<Self, Self::Error> {
        match value {
            Some(raw) => Ok(Self(Some(T::try_from(raw)?))),
            None => Ok(Self(None)),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// 1 · 身份与模型配置（迁移 1）
// ═══════════════════════════════════════════════════════════

/// `users` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: UserId,
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: String,
    /// 口令散列。
    ///
    /// **只用于登录校验路径** —— 转换到领域类型时会被剥掉
    /// （见 [`crate::convert::user_credential`]）。
    pub password_hash: String,
    /// 角色。
    pub role: String,
    /// 头像 URL。
    pub avatar_url: String,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// `model_providers` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ModelProviderRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: ProviderId,
    /// 供应商标识。
    pub code: String,
    /// 显示名。
    pub name: String,
    /// 基础 URL。
    pub base_url: String,
    /// API Key。
    ///
    /// ⚠️ **凭据**。迁移 1 的注释原文说明凭据**不在 SQL 里加密**，
    /// 要靠应用层用主密钥做显式的凭据迁移/管理同步。
    /// 本行类型只是**如实映射** —— 脱敏由上层负责。
    pub api_key: String,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// `model_configs` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ModelConfigRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: ModelConfigId,
    /// 所属供应商。
    #[sqlx(try_from = "i64")]
    pub provider_id: ProviderId,
    /// 模型名。
    pub model_name: String,
    /// 别名。
    pub alias: String,
    /// 模型类型。
    #[sqlx(rename = "type")]
    pub type_: String,
    /// 上下文窗口。
    pub context_window: i32,
    /// 最大输出。
    pub max_output: i32,
    /// 输入价格。
    pub input_price: Numeric,
    /// 输出价格。
    pub output_price: Numeric,
    /// 优先级。
    pub priority: i32,
    /// 能力列表。
    pub capabilities: Json<Value>,
    /// 是否默认。
    pub is_default: bool,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// 向量维度。
    pub embedding_dimension: i32,
}

// ═══════════════════════════════════════════════════════════
// 2 · Agent 与工具注册表（迁移 2）
// ═══════════════════════════════════════════════════════════

/// `agents` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct AgentRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: AgentId,
    /// 名称。
    pub name: String,
    /// 描述（可空）。
    pub description: Option<String>,
    /// 类型。
    #[sqlx(rename = "type")]
    pub type_: String,
    /// 系统提示词（可空）。
    pub system_prompt: Option<String>,
    /// 主模型配置。
    #[sqlx(try_from = "i64")]
    pub model_config_id: ModelConfigId,
    /// 回退模型配置（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub fallback_model_config_id: NullableId<ModelConfigId>,
    /// 采样温度。
    pub temperature: f32,
    /// top-p。
    pub top_p: f32,
    /// 最大 token。
    pub max_tokens: i32,
    /// 最大迭代轮数。
    pub max_iterations: i32,
    /// 是否启用记忆。
    pub memory_enabled: bool,
    /// 记忆窗口。
    pub memory_window: i32,
    /// 是否启用长期记忆。
    pub memory_long_enabled: bool,
    /// 知识库 ID 列表。
    pub kb_ids: Json<Value>,
    /// 工具列表。
    pub tools: Json<Value>,
    /// 是否预设。
    pub is_preset: bool,
    /// 扩展配置。
    pub config: Json<Value>,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// 归属用户（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub owner_id: NullableId<UserId>,
}

/// `mcp_servers` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct McpServerRow {
    /// 主键。
    ///
    /// §2.8.1 的 18 种类型化 ID 里没有 MCP 服务器 —— 用裸 `i64`。
    pub id: i64,
    /// 名称。
    pub name: String,
    /// 传输方式。
    pub transport: String,
    /// URL。
    pub url: String,
    /// 启动命令。
    pub command: String,
    /// 命令参数。
    pub args: Json<Value>,
    /// 环境变量。
    ///
    /// ⚠️ **可能含凭据**（如 `API_KEY`）。
    pub env: Json<Value>,
    /// HTTP 头。
    ///
    /// ⚠️ **可能含凭据** —— 迁移 011 用独立的 `headers_present` 标志位
    /// 记录「有没有头」，说明头里可能有敏感值。
    pub headers: Json<Value>,
    /// 连接状态。
    pub status: String,
    /// 最近一次健康检查时间。
    pub last_health_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 工具缓存。
    pub tools_cache: Json<Value>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// 是否启用。
    pub enabled: bool,
    /// 配置版本。
    pub config_version: i64,
    /// 加密后的凭据。
    ///
    /// ⚠️ **凭据** —— 由应用层用主密钥加密后写入。
    pub encrypted_credentials: String,
    /// 是否配置了 HTTP 头。
    pub headers_present: bool,
    /// 允许访问的用户 ID 列表。
    pub allowed_user_ids: Json<Value>,
    /// 工具指纹。
    pub tools_hash: String,
    /// 归档时间。
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `tools` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ToolRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: ToolId,
    /// 工具标识。
    pub code: String,
    /// 显示名。
    pub name: String,
    /// 工具类型。
    #[sqlx(rename = "type")]
    pub type_: String,
    /// 描述。
    pub description: String,
    /// 参数 schema。
    pub parameters: Json<Value>,
    /// 本地 handler（可空）。
    pub handler: Option<String>,
    /// 所属 MCP 服务器（可空）。
    ///
    /// §2.8.1 没有 MCP 服务器的类型化 ID —— 用裸 `i64`。
    #[sqlx(try_from = "Option<i64>")]
    pub mcp_server_id: NullableId<i64>,
    /// 端点（可空）。
    pub endpoint: Option<String>,
    /// 鉴权配置（可空）。
    ///
    /// ⚠️ **可能含凭据**。
    pub auth_config: Option<Json<Value>>,
    /// 默认配置。
    pub default_config: Json<Value>,
    /// 超时（毫秒）。
    pub timeout_ms: i32,
    /// 是否危险。
    pub is_dangerous: bool,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// 重放策略（`readonly` / `idempotent` / `manual`）。
    pub replay_policy: String,
    /// MCP 工具名。
    pub mcp_name: String,
    /// MCP 配置版本。
    pub mcp_config_version: i64,
    /// 绑定版本。
    pub binding_version: i64,
    /// 策略审阅人（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub policy_reviewed_by: NullableId<UserId>,
    /// 策略审阅时间。
    pub policy_reviewed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 重放证据。
    pub replay_evidence: String,
}

// ═══════════════════════════════════════════════════════════
// 3 · 工作流定义（迁移 3）
// ═══════════════════════════════════════════════════════════

/// `workflows` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct WorkflowRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: WorkflowId,
    /// 名称。
    pub name: String,
    /// 描述（可空）。
    pub description: Option<String>,
    /// DSL 文本。
    pub dsl: String,
    /// 版本号。
    pub version: i32,
    /// 是否激活。
    pub is_active: bool,
    /// 创建者。
    #[sqlx(try_from = "i64")]
    pub creator_id: UserId,
    /// 状态。
    pub status: i16,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// 归档时间。
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `workflow_versions` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct WorkflowVersionRow {
    /// 主键。
    ///
    /// §2.8.1 没有工作流版本 ID —— 用裸 `i64`。
    #[sqlx(try_from = "i64")]
    pub id: i64,
    /// 所属工作流。
    #[sqlx(try_from = "i64")]
    pub workflow_id: WorkflowId,
    /// 版本号。
    pub version: i32,
    /// 定义哈希。
    ///
    /// ⚠️ 列类型是 `CHAR(64)` —— PostgreSQL 的 `bpchar` 会**补空格到定长**。
    /// 比对哈希前必须 `trim_end()`，见 [`crate::convert::trim_bpchar`]。
    pub definition_hash: String,
    /// 冻结的定义快照。
    pub definition: Json<Value>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ═══════════════════════════════════════════════════════════
// 4 · 执行引擎侧（迁移 4）
// ═══════════════════════════════════════════════════════════

/// `execution_runs` 表行 —— **引擎侧**的权威状态。
///
/// # 为什么这里有 `execution_runs` 而没有 `workflow_runs`
///
/// 架构文档 §4.2 差异 1：`workflow_runs` 由 `nexus-execution` **独占**，
/// `nexus-repo` 不得引用它的类型。租约与 fencing 走的是 `execution_runs`
/// （见 NEX-12）。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionRunRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: RunId,
    /// 冻结的定义快照。
    pub definition: Json<Value>,
    /// 定义哈希。
    ///
    /// ⚠️ 列类型是 `CHAR(64)` —— 尾随空格问题见 [`crate::convert::trim_bpchar`]。
    pub definition_hash: String,
    /// 引擎版本。
    pub engine_version: String,
    /// 图 schema 版本。
    pub graph_schema: i32,
    /// 状态。
    pub status: String,
    /// 世代号 —— fencing 的依据。
    pub generation: i64,
    /// 租约持有者。
    pub lease_owner: String,
    /// 租约到期时间。
    pub lease_until: Option<chrono::DateTime<chrono::Utc>>,
    /// 截止时间。
    pub deadline_at: chrono::DateTime<chrono::Utc>,
    /// 可被领取的时间。
    pub available_at: chrono::DateTime<chrono::Utc>,
    /// 下一个事件序号。
    pub next_seq: i64,
    /// 状态版本号。
    pub state_version: i64,
    /// 待处理的中断 ID。
    pub pending_ids: Json<Value>,
    /// 归属用户。
    #[sqlx(try_from = "i64")]
    pub owner_id: UserId,
    /// 幂等键。
    pub request_key: String,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// `execution_checkpoints` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionCheckpointRow {
    /// 所属运行。
    #[sqlx(try_from = "i64")]
    pub run_id: RunId,
    /// 检查点 ID。
    pub checkpoint_id: String,
    /// 定义哈希。
    ///
    /// ⚠️ 列类型是 `CHAR(64)` —— 尾随空格问题见 [`crate::convert::trim_bpchar`]。
    pub definition_hash: String,
    /// 引擎版本。
    pub engine_version: String,
    /// 图 schema 版本。
    pub graph_schema: i32,
    /// 负载。
    pub payload: Vec<u8>,
    /// 更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// `execution_operations` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionOperationRow {
    /// 主键（`VARCHAR(64)`，非自增）。
    pub id: String,
    /// 所属运行。
    #[sqlx(try_from = "i64")]
    pub run_id: RunId,
    /// 操作路径。
    pub path: String,
    /// 操作种类。
    pub kind: String,
    /// 参数哈希。
    pub args_hash: String,
    /// 重放策略。
    pub replay_policy: String,
    /// 状态。
    pub status: String,
    /// 世代号快照 —— 僵尸写入的检测依据。
    pub generation: i64,
    /// 输出（可空）。
    pub output: Option<Json<Value>>,
    /// 错误信息。
    pub error: String,
    /// 尝试次数。
    pub attempt: i32,
    /// 开始时间。
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// 结束时间。
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `execution_attempts` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionAttemptRow {
    /// 主键。
    ///
    /// §2.8.1 没有执行尝试 ID —— 用裸 `i64`。
    #[sqlx(try_from = "i64")]
    pub id: i64,
    /// 所属操作。
    pub operation_id: String,
    /// 世代号。
    pub generation: i64,
    /// 尝试序号。
    pub attempt: i32,
    /// 状态。
    pub status: String,
    /// 错误信息。
    pub error: String,
    /// 开始时间。
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// 结束时间。
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// `execution_approvals` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionApprovalRow {
    /// 主键。
    ///
    /// §2.8.1 没有审批 ID —— 用裸 `i64`。
    #[sqlx(try_from = "i64")]
    pub id: i64,
    /// 所属运行。
    #[sqlx(try_from = "i64")]
    pub run_id: RunId,
    /// 中断标记 ID。
    pub interrupt_id: String,
    /// 操作路径。
    pub path: String,
    /// 参数哈希。
    pub args_hash: String,
    /// 审批提示。
    pub prompt: String,
    /// 决策（`pending` / `approved` / `rejected`）。
    pub decision: String,
    /// 决策人（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub decided_by: NullableId<UserId>,
    /// 决策时间。
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 过期时间。
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// 备注。
    pub comment: String,
}

/// `execution_events` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionEventRow {
    /// 所属运行。
    #[sqlx(try_from = "i64")]
    pub run_id: RunId,
    /// 序号（由数据库原子分配）。
    pub seq: i64,
    /// 事件类型。
    #[sqlx(rename = "type")]
    pub type_: String,
    /// 负载 schema 版本。
    pub schema_version: i32,
    /// 事件数据。
    pub data: Json<Value>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// `execution_outbox` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct ExecutionOutboxRow {
    /// 主键。
    ///
    /// §2.8.1 没有 outbox ID —— 用裸 `i64`。
    #[sqlx(try_from = "i64")]
    pub id: i64,
    /// 所属运行（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub run_id: NullableId<RunId>,
    /// 序号（可空）。
    pub seq: Option<i64>,
    /// 主题。
    pub topic: String,
    /// 负载。
    pub payload: Json<Value>,
    /// 发布时间（`NULL` = 待投递）。
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 投递去重键。
    pub dedup_key: Option<String>,
}

// ═══════════════════════════════════════════════════════════
// 6 · 会话与消息（迁移 6）
// ═══════════════════════════════════════════════════════════

/// `sessions` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct SessionRow {
    /// 主键（UUID）。
    #[sqlx(try_from = "uuid::Uuid")]
    pub id: SessionId,
    /// 归属用户。
    #[sqlx(try_from = "i64")]
    pub user_id: UserId,
    /// 绑定的 Agent（可空）。
    #[sqlx(try_from = "Option<i64>")]
    pub agent_id: NullableId<AgentId>,
    /// 标题。
    pub title: String,
    /// 状态。
    pub status: String,
    /// 是否置顶。
    pub pinned: bool,
    /// 消息条数。
    pub message_count: i32,
    /// 累计 token 用量。
    pub token_usage: i64,
    /// 最后一条消息的时间。
    pub last_msg_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 扩展元数据。
    pub metadata: Json<Value>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 最后更新时间。
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// `messages` 表行。
#[derive(Debug, Clone, FromRow)]
pub struct MessageRow {
    /// 主键。
    #[sqlx(try_from = "i64")]
    pub id: MessageId,
    /// 所属会话。
    #[sqlx(try_from = "uuid::Uuid")]
    pub session_id: SessionId,
    /// 角色。
    pub role: String,
    /// 正文内容。
    pub content: String,
    /// 推理内容（可空）。
    pub reasoning_content: Option<String>,
    /// 工具调用列表（可空）。
    pub tool_calls: Option<Json<Value>>,
    /// 工具调用 ID（可空）。
    ///
    /// §2.8.1 的 `ToolCallId` 是 `String` 底座 —— 这里的 `String` 直接对应。
    pub tool_call_id: Option<String>,
    /// 引用（可空）。
    pub refs: Option<Json<Value>>,
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
    /// 状态。
    pub status: i16,
    /// 追踪 ID。
    ///
    /// §2.8.1 的 `TraceId` 是 `String` 底座 —— 这里的 `String` 直接对应。
    pub trace_id: String,
    /// 扩展元数据。
    pub metadata: Json<Value>,
    /// 创建时间。
    pub created_at: chrono::DateTime<chrono::Utc>,
}
