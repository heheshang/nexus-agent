//! ReAct 循环本体：模型 ↔ 工具的往复。
//!
//! # 管道顺序**有讲究**
//!
//! ```text
//! run(input, tools, cancel)
//!   → 协议完整性校验（tool_call ↔ tool_result 配对）        ← **先于**任何裁剪
//!   → 上下文裁剪（整轮淘汰）                                  ← 每步都重跑
//!   → 模型调用（ChatModel::chat）
//!   → 校验本步输出（tool_call ID / 最终答案）
//!   → 有工具调用？
//!       ├─ 否 → 发 done，终态
//!       └─ 是 → 并发执行工具（Semaphore 限流）
//!                 → 工具结果**结构化回灌**为 tool 角色消息
//!                 → 发 tool_result 事件
//!                 → 回到循环开头
//! ```
//!
//! # 三个安全阀
//!
//! | 阀 | 触发 | 终态 |
//! |---|---|---|
//! | 最大迭代 | 迭代次数 > `max_iterations` | `error` / `MaxIteration`（30002） |
//! | 单步超时 | 单次模型调用超时 | `error` / `ModelTimeout`（50002） |
//! | 总超时 | 整个运行超时 | `error` / `ModelTimeout`（50002） |
//!
//! > **三个阀的默认值由调用方传入**，`nexus-agent` **不内置**隐式默认值 ——
//! > 缺省值会让「没配」和「配成 0」无法区分（同 `nexus-llm` 的 `from_config` 纪律）。
//!
//! # 终态分类：**先固化错误，再取消**
//!
//! 参考实现 `eino_runner.go:216-218` 的注释原文：
//!
//! > Preserve the original error before canceling model/tool work.
//! > A provider failure must not turn into "user canceled" because cleanup cancels ctx.
//!
//! 所以本模块**先**判定终态并**固化**错误，**再**取消 ——
//! 否则供应商失败会被 `cancel` 覆盖成「用户取消」。
//!
//! 取消的终态是 `done`（**不是** `error`）—— 取消**不是**失败。
//!
//! # 工具输出是**数据**不是指令
//!
//! 工具结果回灌时是 `tool` 角色消息，**不是** `assistant` 角色。
//! 工具输出是**不可信输入** —— 用 `assistant` 角色回灌等于让工具输出
//! 冒充模型的发言，那它就获得了指令地位。

use std::sync::Arc;
use std::time::Duration;

use nexus_core::error::{AppError, ErrorCode};
use nexus_core::event::{EventPayload, FinishReason, ToolStatus};
use nexus_llm::types::{ChatRequest, Message, Role, ToolCall, ToolDef};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::compaction::compact_input_history;
use crate::events::EventSink;
use crate::traits::{ChatModel, InvokableTool};

/// 一次运行的配置。
///
/// # 为什么**全部**由调用方传入
///
/// 三个安全阀与工具并发上限都是**策略**，不是实现细节 ——
/// 不同场景的取值差别很大（交互式对话 vs 批处理）。
///
/// `nexus-agent` **不内置**隐式默认值 —— 缺省值会让「没配」和「配成 0」
/// 无法区分（同 `nexus-llm` 的 `from_config` 纪律）。
#[derive(Debug, Clone)]
pub struct ReactConfig {
    /// 最大迭代次数。
    ///
    /// 迭代次数 > 该值 → `MaxIteration`（30002）。
    pub max_iterations: usize,
    /// 单次模型调用的超时。
    ///
    /// 超时 → `ModelTimeout`（50002）。
    pub step_timeout: Duration,
    /// 整个运行的超时。
    ///
    /// 超时 → `ModelTimeout`（50002）。
    pub total_timeout: Duration,
    /// 同一轮内同时执行的工具数上限。
    pub max_concurrent_tools: usize,
    /// 上下文预算（字节）。
    pub context_budget: usize,
    /// 单个工具输出的最大长度（字节）。
    ///
    /// 超长输出会被**截断** —— 一个返回巨量输出的工具
    /// 不能撑爆上下文预算。
    pub max_tool_output: usize,
    /// 采样温度。
    pub temperature: f32,
    /// 核采样。
    pub top_p: f32,
    /// 输出 token 上限。
    pub max_tokens: i32,
}

impl Default for ReactConfig {
    /// ⚠️ 这个 `Default` **只用于测试**与示例。
    ///
    /// 生产路径必须**显式**构造 —— 见 [`ReactConfig`] 的文档。
    fn default() -> Self {
        Self {
            max_iterations: 10,
            step_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(300),
            max_concurrent_tools: 4,
            context_budget: 256 * 1024,
            max_tool_output: 32 * 1024,
            temperature: 0.0,
            top_p: 1.0,
            max_tokens: 4096,
        }
    }
}

/// 一次运行的终态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 正常结束 —— 模型给出了最终答案。
    Finished {
        /// 最终答案。
        answer: String,
        /// 实际执行的迭代次数。
        iterations: usize,
    },
    /// 被取消 —— **不是失败**。
    Canceled {
        /// 已执行的迭代次数。
        iterations: usize,
    },
    /// 失败。
    Failed {
        /// 失败的业务码。
        ///
        /// ⚠️ 存**码**而不是 `AppError` ——
        /// `AppError` 不实现 `Clone` / `Eq`，而终态需要能被**比较**。
        code: ErrorCode,
        /// 面向用户的失败消息。
        message: String,
        /// 已执行的迭代次数。
        iterations: usize,
    },
}

impl Outcome {
    /// 失败时的业务码。
    pub fn error_code(&self) -> Option<ErrorCode> {
        match self {
            Outcome::Failed { code, .. } => Some(*code),
            _ => None,
        }
    }

    /// 失败时的消息。
    pub fn error_message(&self) -> Option<&str> {
        match self {
            Outcome::Failed { message, .. } => Some(message),
            _ => None,
        }
    }

    /// 是否是失败终态。
    pub fn is_failed(&self) -> bool {
        matches!(self, Outcome::Failed { .. })
    }

    /// 失败终态的构造辅助。
    fn failed(error: AppError, iterations: usize) -> Self {
        Outcome::Failed {
            code: error.user_facing().code,
            message: error.user_facing().message.to_string(),
            iterations,
        }
    }
}

/// 工具执行的结果。
#[derive(Clone)]
struct ToolExecution {
    /// 对应的调用 ID。
    call_id: String,
    /// 工具名。
    #[allow(dead_code)]
    name: String,
    /// 执行结果（成功时是输出，失败时是**结构化错误**）。
    content: String,
    /// 执行状态。
    status: ToolStatus,
    /// 耗时（毫秒）。
    elapsed_ms: u64,
}

/// ReAct 循环。
///
/// # 泛型 + 静态分发
///
/// `M: ChatModel` 是**泛型参数** —— 零虚调用开销（架构文档 §2.3.2）。
/// 工具则是 `Arc<dyn InvokableTool>` —— 异构集合必须 `dyn`（架构文档 §2.3.3）。
#[derive(Debug)]
pub struct ReactLoop<'a, M: ChatModel> {
    /// 模型。
    model: &'a M,
    /// 事件 sink。
    sink: &'a EventSink,
    /// 配置。
    config: ReactConfig,
}

impl<'a, M: ChatModel> ReactLoop<'a, M> {
    /// 构造一个循环。
    pub fn new(model: &'a M, sink: &'a EventSink, config: ReactConfig) -> Self {
        Self {
            model,
            sink,
            config,
        }
    }

    /// 跑一次 ReAct 循环。
    ///
    /// # 终态事件由**调用方**决定
    ///
    /// 本方法**只**返回终态分类，**不**发终态事件 ——
    /// 因为「先固化错误，再取消」的顺序要求终态事件的**发送**发生在
    /// 错误固化**之后**，而发送需要调用方掌握取消令牌。
    ///
    /// 调用方应：
    ///
    /// 1. 调用本方法，拿到 [`Outcome`]；
    /// 2. 按 `Outcome` 发出对应的终态事件（`done` / `error`）；
    /// 3. **然后**取消上游工作。
    pub async fn run(
        &self,
        messages: Vec<Message>,
        tools: &[Arc<dyn InvokableTool>],
        cancel: &CancellationToken,
    ) -> Outcome {
        // ⚠️ **总超时**包住整个循环。
        //
        // 用 `timeout` 而不是「每步检查」—— 后者需要额外的时钟注入。
        match tokio::time::timeout(
            self.config.total_timeout,
            self.run_inner(messages, tools, cancel),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                // 总超时 —— 归类为 `ModelTimeout`（50002）。
                Outcome::failed(AppError::new(ErrorCode::ModelTimeout, "运行超时"), 0)
            }
        }
    }

    /// 循环本体。
    async fn run_inner(
        &self,
        messages: Vec<Message>,
        tools: &[Arc<dyn InvokableTool>],
        cancel: &CancellationToken,
    ) -> Outcome {
        let mut history = messages;
        let mut iterations = 0usize;

        loop {
            // ── 取消检查 ──
            //
            // ⚠️ 取消的终态是 `done`（**不是** `error`）—— 取消**不是**失败。
            if cancel.is_cancelled() {
                return Outcome::Canceled { iterations };
            }

            // ── 安全阀 1：最大迭代 ──
            //
            // ⚠️ 检查在**进入**循环体**之前** —— 所以 `max_iterations = 0`
            // 意味着「一次都不许跑」，而不是「跑一次」。
            if iterations >= self.config.max_iterations {
                return Outcome::failed(
                    AppError::new(ErrorCode::MaxIteration, "超出最大迭代次数"),
                    iterations,
                );
            }

            // ── 协议完整性校验 → 裁剪（每步都重跑）──
            let request = match self.build_request(&history, tools) {
                Ok(request) => request,
                Err(error) => {
                    return Outcome::failed(error, iterations);
                }
            };

            // ── 模型调用（安全阀 2：单步超时）──
            let response = match self.call_model(&request, cancel).await {
                Ok(response) => response,
                Err(error) => {
                    // ⚠️ **先固化错误**，再返回。
                    // 调用方拿到这个错误后**才**取消 —— 顺序不能反。
                    return Outcome::failed(error, iterations);
                }
            };

            iterations += 1;

            // ── 输出校验 ──
            if let Err(error) = validate_response(&response) {
                return Outcome::failed(error, iterations);
            }

            // ── 无工具调用 → 终态 ──
            if response.tool_calls.is_empty() {
                // 发 `done` 事件 —— 必达。
                let _ = self
                    .sink
                    .emit_guaranteed(EventPayload::Done {
                        finish_reason: FinishReason::Stop,
                    })
                    .await;

                return Outcome::Finished {
                    answer: response.content,
                    iterations,
                };
            }

            // ── 有工具调用 → 并发执行 ──
            let assistant = assistant_message(&response.content, &response.tool_calls);
            history.push(assistant);

            let executions = self
                .execute_tools(&response.tool_calls, tools, cancel)
                .await;

            // ── 结果回灌（`tool` 角色）+ 发事件 ──
            for execution in executions {
                history.push(Message::tool_result(
                    execution.call_id.clone(),
                    execution.content.clone(),
                ));

                let _ = self
                    .sink
                    .emit_guaranteed(EventPayload::ToolResult {
                        call_id: execution.call_id.clone().into(),
                        result: execution.content,
                        elapsed_ms: execution.elapsed_ms,
                        status: execution.status,
                    })
                    .await;
            }

            // ── 回到循环开头 ──
            //
            // ⚠️ 裁剪在**每一步**都重跑 —— 不是只在第一步。
            // 工具结果会让历史**增长**，第二步的预算可能就不够了。
        }
    }

    /// 构造模型请求：协议校验 → 裁剪。
    fn build_request(
        &self,
        history: &[Message],
        tools: &[Arc<dyn InvokableTool>],
    ) -> Result<ChatRequest, AppError> {
        // ── 协议完整性校验（**先于**裁剪）──
        crate::protocol::validate_messages(history).map_err(|err| err.to_app_error())?;

        // ── 裁剪（整轮淘汰）──
        let tool_defs = tool_definitions(tools);
        let compacted = compact_input_history(history, &tool_defs, self.config.context_budget)?;

        Ok(ChatRequest {
            messages: compacted.messages,
            tools: tool_defs,
            temperature: self.config.temperature,
            top_p: self.config.top_p,
            max_tokens: self.config.max_tokens,
            stream: false,
        })
    }

    /// 调用模型（安全阀 2：单步超时）。
    async fn call_model(
        &self,
        request: &ChatRequest,
        cancel: &CancellationToken,
    ) -> Result<nexus_llm::types::ChatResponse, AppError> {
        match tokio::time::timeout(self.config.step_timeout, self.model.chat(request, cancel)).await
        {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(AppError::new(ErrorCode::ModelTimeout, "模型调用超时")),
        }
    }

    /// 并发执行工具（`Semaphore` 限流）。
    async fn execute_tools(
        &self,
        calls: &[ToolCall],
        tools: &[Arc<dyn InvokableTool>],
        cancel: &CancellationToken,
    ) -> Vec<ToolExecution> {
        // ⚠️ `Semaphore` 限流 —— 同一轮内同时执行的工具数不超过上限。
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent_tools));

        let mut handles = Vec::with_capacity(calls.len());

        for call in calls {
            // ⚠️ 必须**克隆 `Arc`** —— 不能把 `&Arc<...>` 移进 `tokio::spawn`
            // （那要求 `'static` 生命周期，而 `tools` 是借用）。
            let tool = find_tool(tools, &call.name).cloned();
            let semaphore = Arc::clone(&semaphore);
            let cancel = cancel.clone();
            let call = call.clone();
            let max_output = self.config.max_tool_output;

            handles.push(tokio::spawn(async move {
                // ⚠️ 先拿许可 —— 拿不到就**等**（限流，不是丢弃）。
                let Ok(_permit) = semaphore.acquire_owned().await else {
                    return ToolExecution {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: tool_error_content("工具执行被取消", 70002),
                        status: ToolStatus::Failed { code: 70002 },
                        elapsed_ms: 0,
                    };
                };

                let started = std::time::Instant::now();
                match tool {
                    Some(tool) => match tool.invoke(&call.args, &cancel).await {
                        Ok(output) => ToolExecution {
                            call_id: call.id.clone(),
                            name: call.name.clone(),
                            content: truncate_utf8(&output, max_output),
                            status: ToolStatus::Ok,
                            elapsed_ms: started.elapsed().as_millis() as u64,
                        },
                        Err(error) => {
                            // ⚠️ 工具失败**结构化回灌** —— 不终止循环。
                            let code = error.user_facing().code.code();
                            ToolExecution {
                                call_id: call.id.clone(),
                                name: call.name.clone(),
                                content: tool_error_content(&error.user_facing().message, code),
                                status: ToolStatus::Failed { code },
                                elapsed_ms: started.elapsed().as_millis() as u64,
                            }
                        }
                    },
                    None => ToolExecution {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        content: tool_error_content("工具不存在", 70001),
                        status: ToolStatus::Failed { code: 70001 },
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    },
                }
            }));
        }

        // ⚠️ 收集全部结果 —— 工具失败**不终止**循环。
        let mut executions = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(execution) => executions.push(execution),
                Err(join_error) => {
                    // task panic —— 也要变成**结构化观察**回灌。
                    executions.push(ToolExecution {
                        call_id: String::new(),
                        name: String::new(),
                        content: tool_error_content("工具执行异常", 70002),
                        status: ToolStatus::Failed { code: 70002 },
                        elapsed_ms: 0,
                    });
                    tracing::error!(error = %join_error, "工具执行 task 异常");
                }
            }
        }
        executions
    }
}

/// 校验模型本步输出。
fn validate_response(response: &nexus_llm::types::ChatResponse) -> Result<(), AppError> {
    let mut seen = std::collections::HashSet::new();
    for call in &response.tool_calls {
        if call.id.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidParam,
                "模型返回了空的工具调用 ID",
            ));
        }
        if !seen.insert(call.id.clone()) {
            return Err(AppError::new(
                ErrorCode::InvalidParam,
                format!("模型返回了重复的工具调用 ID：{}", call.id),
            ));
        }
    }
    Ok(())
}

/// 构造 assistant 消息（带工具调用）。
fn assistant_message(content: &str, calls: &[ToolCall]) -> Message {
    let mut message = Message::new(Role::Assistant, content);
    message.tool_calls = calls.to_vec();
    message
}

/// 找出工具定义。
///
/// # 为什么是 `pub`
///
/// 调用方需要用**同一个口径**算「工具定义占多少预算」——
/// 否则设定的 `context_budget` 与实际裁剪用的口径会对不上。
pub fn tool_definitions(tools: &[Arc<dyn InvokableTool>]) -> Vec<ToolDef> {
    tools
        .iter()
        .map(|tool| ToolDef {
            source_identity: String::new(),
            binding_version: 0,
            capability: String::new(),
            dangerous: false,
            replay_policy: String::new(),
            name: tool.name().to_string(),
            description: tool.description().to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        })
        .collect()
}

/// 按名字找工具。
fn find_tool<'a>(
    tools: &'a [Arc<dyn InvokableTool>],
    name: &str,
) -> Option<&'a Arc<dyn InvokableTool>> {
    tools.iter().find(|tool| tool.name() == name)
}

/// 工具失败的**结构化**回灌内容。
///
/// 形状：`{"error": ..., "message": ...}`。
///
/// 这是 Agent **自我修正**能力的前提 —— 模型看到错误可以换个参数重试。
pub fn tool_error_content(message: &str, code: i32) -> String {
    serde_json::json!({
        "error": code,
        "message": message,
    })
    .to_string()
}

/// 按 UTF-8 边界截断。
///
/// ⚠️ **必须**在字符边界截断 —— 直接 `String::truncate` 在非边界处会 panic。
///
/// 保留的字节数上限是 `max_bytes` —— 截断后可能**少于**该值
/// （因为不能切半个字符）。
pub fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    // ⚠️ 找**不超过** `max_bytes` 的最后一个字符边界。
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    let mut truncated = text[..end].to_string();
    truncated.push_str("…[已截断]");
    truncated
}
