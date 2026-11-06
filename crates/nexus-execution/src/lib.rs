//! nexus-execution
//!
//! **执行层**：领取 run → 校验版本 → 编译图 → 执行 → 落库。
//!
//! # 本 crate 负责什么
//!
//! | 职责 | 说明 |
//! |---|---|
//! | **驱动**（driver） | 「领取 → 校验 → 编译 → 执行」的骨架 |
//! | **操作幂等账本** | 「同一路径不重复执行」—— 持久执行的核心不变量 |
//! | **检查点** | 续跑的「断点」—— 版本三元组不一致就**拒绝**，不静默重放 |
//! | **审批挂起与恢复** | 审批指纹绑定「哪一次调用、哪些参数」 |
//! | **取消与人工核对** | 「不可知即人工核对」—— 取消保留不确定性 |
//!
//! # 本 crate **不**负责什么
//!
//! | 不负责 | 归谁 |
//! |---|---|
//! | **引擎侧持久化**（租约 / fencing / 幂等提交 / 事件序号） | **`nexus-repo`**（决策 D12 · 选项 B，2026-11-06 操作者裁决） |
//! | **图引擎**（冻结定义 / 图编译 / 节点执行契约） | **`nexus-workflow`** |
//! | **HTTP 端点** | **`nexus-api`**（P6 不交付 —— 决策 D17） |
//! | **业务编排** | **`nexus-service`**（P5 的决策 D9） |
//!
//! # 引擎侧持久化**留在** `nexus-repo`（决策 D12 · 选项 B）
//!
//! 依赖方向是 **`nexus-execution → nexus-repo`**（单向）——
//! 执行层**经由**数据层的带租约 API 做围栏写入，**不自己**拼围栏 SQL。
//!
//! 理由：围栏条件**只有一处定义**
//! （`nexus_repo::Fence::CONDITIONS_TEMPLATE`）。
//! 若把持久化能力下沉到执行层，围栏条件就会与数据层的引擎侧表
//! （`execution_runs`）**分离** —— 写入面分裂，fencing 的保证变弱。
//!
//! 反向依赖（`nexus-repo → nexus-execution`）**禁止** —— 会成环。
//! 见架构文档 §4.4 规则 4。
//!
//! # 契约
//!
//! | 项 | 位置 |
//! |---|---|
//! | 引擎侧持久化能力 | `nexus_repo::{ExecutionLeaseRepository, OutboxRepository}` |
//! | 围栏条件（**唯一**定义处） | `nexus_repo::Fence::CONDITIONS_TEMPLATE` |
//! | 独占的表（**写入权**） | `workflow_runs` / `workflow_step_runs`（架构文档 §4.2 差异 1） |
//!
//! 三条结构层断言守护这些边界 ——
//! 见 `crates/nexus-execution/tests/architecture.rs`。

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
