//! nexus-repo
//!
//! **数据访问层**：数据库**行类型**（`#[derive(FromRow)]`）+ 仓储。
//!
//! # 职责边界（架构文档 §4.2 差异 2）
//!
//! | crate | 内容 | 禁止 |
//! |---|---|---|
//! | `nexus-model` | 领域类型（纯 DTO） | 任何数据库痕迹（`FromRow` / `sqlx::`） |
//! | `nexus-repo`（本 crate） | **行类型** + 仓储 | 业务语义 |
//!
//! 两者**分开** —— 改表结构不会波及 API 契约。
//! `nexus-model` 的纯净由 `scripts/check-deps.sh` 规则 6 扫描源码强制。
//!
//! # 子模块
//!
//! | 模块 | 内容 |
//! |---|---|
//! | [`rows`] | Phase 0 表的行类型（`#[derive(FromRow)]`） |
//! | [`convert`] | 行类型 → 领域类型的**显式**转换 |
//! | [`error`] | `sqlx::Error` → [`AppError`] 的**唯一**转换点 |
//! | [`users`] | 用户仓储 |
//! | [`sessions`] | 会话仓储 |
//! | [`lease`] | 租约与 fencing 的**类型**（`Fence` / `RunLease`） |
//! | [`leases`] | 执行租约的仓储（领取 + 围栏写入） |
//!
//! # 依赖方向
//!
//! `nexus-repo` → 能力层（`nexus-rag` / `nexus-memory` / `nexus-vector` / `nexus-ingestion`）
//! 是**单向**的 —— 本 crate 依赖它们只是为了**实现它们的 trait**。
//! 绝不能反向（架构文档 §4.1.2），由 `scripts/check-deps.sh` 规则 5 强制。
//!
//! # 本 crate **不碰**的表
//!
//! `workflow_runs` 与 `workflow_step_runs` 由 `nexus-execution` **独占**
//! （架构文档 §4.2 差异 1）—— 它们的行类型也在 `nexus-execution`，
//! 不在本 crate。
//!
//! # SQL 校验方式
//!
//! 一律 `query_as` + `FromRow`（**运行期**校验），**不用** `query!` 编译期宏
//! —— 架构文档 §3.9.3 已定稿。代价与缓解手段见 [`rows`] 模块文档。

pub mod convert;
pub mod error;
pub mod lease;
pub mod leases;
pub mod rows;
pub mod sessions;
pub mod users;

pub use error::map_sqlx_error;
pub use lease::{Fence, LeaseLost, LeaseOutcome, RunLease};
pub use leases::{
    CounterColumn, DEFAULT_LEASE_TTL_SECS, ExecutionLeaseRepository, LeaseInfo, LeaseSettings,
};
pub use rows::{
    AgentRow, ExecutionApprovalRow, ExecutionAttemptRow, ExecutionCheckpointRow, ExecutionEventRow,
    ExecutionOperationRow, ExecutionOutboxRow, ExecutionRunRow, McpServerRow, MessageRow,
    ModelConfigRow, ModelProviderRow, NullableId, Numeric, SessionRow, ToolRow, UserRow,
    WorkflowRow, WorkflowVersionRow,
};
pub use sessions::SessionRepository;
pub use users::UserRepository;

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
