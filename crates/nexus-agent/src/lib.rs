//! nexus-agent —— Agent 运行时。
//!
//! # 本 crate 负责什么
//!
//! - **ReAct 循环** —— 模型要工具就调工具，工具失败就回灌让模型自己纠正，
//!   模型给答案就结束。
//! - **上下文预算** —— 历史超预算时按**整轮**淘汰（决策 D5）。
//! - **事件 sink 与背压** —— 单写者序号、必达 / 可丢双通道。
//! - **工具调用协议的完整性校验** —— `tool_call ↔ tool_result` 配对。
//!
//! # 本 crate **不**负责什么
//!
//! | 项 | 归属 |
//! |---|---|
//! | SSE 桥接 / 半帧处理 | P5（`nexus-api`） |
//! | 危险工具的**授权边界** | P8（`nexus-tool` / `nexus-mcp`） |
//! | Schema 子集白名单校验 | P8；本 crate 只把 `parameters` 原样透传 |
//! | 多智能体路由 / Host | 后续阶段；本 crate 是**单 Agent** |
//! | Checkpoint / 中断恢复 | P6（`nexus-workflow` + `nexus-execution`） |
//! | 长期记忆召回 / RAG 组装 | P7；本 crate 的输入历史由调用方**组装好**传入 |
//! | 装配层（谁构造运行时） | P5 / P6 |
//!
//! # trait 由**本 crate** 定义
//!
//! 架构文档 §4.1.2 的 **consumer-defined trait**：
//! trait 定义在**消费者**，实现在**提供方**。
//!
//! ReAct 循环是消费者，网关与工具是实现方 —— 所以 [`ChatModel`] 与
//! [`InvokableTool`] 定义在这里，`nexus-llm::Gateway` 经 trait **注入**。
//!
//! 本 crate **不引用** `nexus_llm::Gateway` 的具体类型
//! （参考架构 §2.2 的明确要求），
//! 但**复用** `nexus-llm` 的**协议数据类型**（决策 D4）。
//!
//! # 不新增 crate 依赖方向
//!
//! 本 crate 的依赖方向是**既有**的（架构文档 §4.1.1 的 `AG → LLM`、`AG → TOOL`）。
//! P4 **不改动** `scripts/check-deps.sh` 的依赖表。
//!
//! # 契约
//!
//! - 错误码：只**复用** `nexus-core` 已定稿的 42 个，**不新增**。
//! - 事件：只**复用** `nexus-core` 已定稿的 12 个，**不新增**。
//! - ID 类型：只**复用** `nexus-core` 已定稿的 18 个。
//!
//! 详见 `docs/specs/p4/spec.md` 与 `docs/specs/p4/contracts/`。

pub mod compaction;
pub mod events;
pub mod protocol;
pub mod traits;

pub use compaction::{CompactedHistory, compact_input_history};
pub use events::{EventSink, EventSinkConfig, EventStream, Outcome};
pub use protocol::{ProtocolError, ProtocolSummary, validate_messages};
pub use traits::{ChatModel, InvokableTool};

/// crate 版本号，便于运行时自检与日志标注。
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");
