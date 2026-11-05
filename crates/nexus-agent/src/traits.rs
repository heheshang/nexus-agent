//! 运行时 trait 边界：与模型、与工具说话。
//!
//! # 为什么 trait 由**本 crate** 定义（决策 D4）
//!
//! 架构文档 §4.1.2 确立的 **consumer-defined trait** 手法：
//! trait 由**消费者**定义，实现在**提供方**crate。
//!
//! ReAct 循环是消费者，网关是实现方 —— 所以 `ChatModel` 定义在这里。
//!
//! # 为什么签名复用 `nexus-llm` 的数据类型
//!
//! 参考架构 §2.2 的禁令对象是**具体 `Gateway` 实现类型**
//! （「**不要**让 `nexus-agent::host` 依赖 `nexus-llm` 的具体 `Gateway` 类型」），
//! **不是**协议数据类型 —— 复用数据类型**不违反**该禁令。
//!
//! 自建一套 `Message` / `ToolDef` 会导致两套语义漂移 ——
//! 改一处忘另一处是**必然**的，不是「可能」。
//!
//! 所以本 crate 的形态是：**依赖数据、依赖接口，不依赖实现**。
//!
//! # 两个 trait 的 `dyn` 策略**相反**
//!
//! | trait | 策略 | 理由 |
//! |---|---|---|
//! | [`ChatModel`] | **泛型 + 静态分发** | 架构文档 §2.3.2：「绝大多数能力接口用泛型」；参考架构 §2.2：「内部用泛型 + AFIT 获得静态分发」 |
//! | [`InvokableTool`] | **`dyn` 兼容** | 架构文档 §2.3.3 明确保留的**异构集合**场景 —— 工具注册表里混着内置 / MCP / 用户自定义工具 |
//!
//! 这个区别是**最容易做错**的地方 —— 两个 trait 长得像，用途却完全不同。

use async_trait::async_trait;
use nexus_core::error::AppError;
use nexus_llm::types::{ChatRequest, ChatResponse};
use std::future::Future;

/// 与模型对话的能力。
///
/// # 为什么不 `dyn`
///
/// 架构文档 §2.3.2 的分流判据是「这个能力需要运行时替换吗」——
/// 模型实现由**配置**决定，但**在一次运行内是固定的**。
/// 参考架构 §2.2 的性能提示更直接：
/// 「`nexus-agent` 内部用**泛型 + AFIT** 获得静态分发，
/// 只有在 `nexus-service` 装配层才 `dyn` 化」。
///
/// 所以本 trait 用 **AFIT**（`async fn` in trait），
/// 调用方用泛型参数 `<M: ChatModel>` —— **零虚调用开销**，
/// 且编译器能做单态化优化。
///
/// # ⚠️ AFIT 不能 `dyn` 化
///
/// 架构文档 §2.3.2 的实测记录：
/// `trait ChatModel { async fn chat(&self, ..) }` 标注 `Box<dyn ChatModel>`
/// 会报 `not dyn compatible ... because method 'chat' is 'async'`。
///
/// 需要 `dyn` 时用 `async_trait` —— 但**不要**在热路径上这么做。
///
/// # 取消
///
/// 取消通过 [`tokio_util::sync::CancellationToken`] 传递 ——
/// 架构文档 §2.1.5 确认 `CancellationToken` 是「取消信号的**实现细节**」。
///
/// # 错误语义
///
/// 返回 [`AppError`]，失败分类由**码值**决定
/// （架构文档 §2.6.3）—— 实现方**不另写一套**重试判据。
///
/// # 为什么不用 `async fn`（而是脱糖成 `impl Future`）
///
/// `async fn` in trait 会触发 `async_fn_in_trait` 警告 ——
/// 它的**根本问题**是：trait 方法返回的 future **不能声明 `Send` 约束**。
///
/// 而本 crate 的 `ChatModel` 实现会被放进 `tokio::spawn` ——
/// 那里**必须**要求 `Send`。
///
/// 脱糖成 `-> impl Future<Output = ...> + Send` 解决了三件事：
///
/// 1. **能声明 `Send`** —— 实现方可以自由地跨线程。
/// 2. **零警告** —— 不需要 `#[allow]`。
/// 3. **保持静态分发** —— 泛型调用方依然单态化，零虚调用开销。
///
/// # 为什么 `#[allow(clippy::manual_async_fn)]`
///
/// clippy 的 `manual_async_fn` 建议把 `-> impl Future + Send` 改回 `async fn`。
/// 但那会**丢掉显式的 `Send` 约束** ——
///
/// AFIT（`async fn` in trait）返回的 future **不能声明 `Send`**，
/// 而 rustc 的 `async_fn_in_trait` lint 正是为此而生。
///
/// 本 trait 的实现会被放进 `tokio::spawn` —— 那里**必须**要求 `Send`。
///
/// 所以这里**故意保留** `impl Future + Send`，并显式豁免 clippy 的建议。
/// 豁免是**显式**的（不是全局 `allow`），并在上面写明了理由。
#[allow(clippy::manual_async_fn)]
pub trait ChatModel: Send + Sync {
    /// 非流式对话。
    ///
    /// # 错误
    ///
    /// - `ErrorCode::ModelUnavailable`（50001）—— 上游明确不可用，**可重试**。
    /// - `ErrorCode::ModelTimeout`（50002）—— 上游超时，**结果未知**，
    ///   **禁止**自动重试（请求可能已到达上游并产生副作用）。
    /// - `ErrorCode::ServiceUnavailable`（90003）—— 调用被取消。
    fn chat(
        &self,
        req: &ChatRequest,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> impl Future<Output = Result<ChatResponse, AppError>> + Send;
}

/// 可调用的工具。
///
/// # 为什么必须 `dyn` 兼容
///
/// 工具注册表是**异构集合** —— 内置工具、MCP 工具、用户自定义工具
/// 是**不同的类型**，必须放进同一个集合。
///
/// 架构文档 §2.3.3 把「异构集合」列为明确保留 `dyn` 的场景。
///
/// # 为什么用 `#[async_trait]` 而不是 AFIT
///
/// AFIT 不能 `dyn` 化（架构文档 §2.3.2 的实测记录），
/// 而本 trait **必须**能放进异构集合。
///
/// # 工具输出是**不可信输入**
///
/// 返回的字符串会被回灌给模型 —— 它是**数据**，不是**指令**。
/// 调用方负责截断（超长输出会撑爆上下文预算）。
///
/// # 失败**不抛异常**
///
/// 工具失败**必须**返回 `Err`，但**不能**用它终止 ReAct 循环 ——
/// 调用方会把错误**结构化回灌**给模型，让模型自己纠正。
/// 这是 Agent **自我修正**能力的前提（参考架构 Phase 1「必抄的不变量」①）。
///
/// # 取消
///
/// 同 [`ChatModel`] —— `CancellationToken` 是实现细节。
#[async_trait]
pub trait InvokableTool: Send + Sync {
    /// 工具名 —— 模型据此选择工具。
    fn name(&self) -> &str;

    /// 工具描述 —— 发给模型看。
    fn description(&self) -> &str;

    /// 执行工具。
    ///
    /// `args` 是**原始 JSON 字符串**（OpenAI 兼容协议形状）。
    ///
    /// # 错误
    ///
    /// - `ErrorCode::ToolNotFound`（70001）—— 工具不存在。
    /// - `ErrorCode::ToolFailed`（70002）—— 执行失败，**结果未知**
    ///   （可能已产生副作用），**禁止**自动重试。
    /// - `ErrorCode::ToolTimeout`（70003）—— 执行超时，**结果未知**。
    async fn invoke(
        &self,
        args: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<String, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;

    struct Echo;

    #[async_trait]
    impl InvokableTool for Echo {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "回显输入"
        }

        async fn invoke(
            &self,
            args: &str,
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<String, AppError> {
            Ok(args.to_string())
        }
    }

    struct Static;

    // 与 trait 定义同理 —— 测试实现也保留 `impl Future + Send`，
    // 以便验证该形态**真的**能用（而不是靠 trait 声明推断）。
    #[allow(clippy::manual_async_fn)]
    impl ChatModel for Static {
        fn chat(
            &self,
            _req: &ChatRequest,
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> impl Future<Output = Result<ChatResponse, AppError>> + Send {
            async move {
                Ok(ChatResponse {
                    content: "ok".to_string(),
                    reasoning_content: String::new(),
                    tool_calls: Vec::new(),
                    finish_reason: "stop".to_string(),
                    model: "static".to_string(),
                    usage: Default::default(),
                })
            }
        }
    }

    #[test]
    fn a_tool_can_be_stored_as_a_trait_object() {
        // 异构集合是 `InvokableTool` 必须 `dyn` 兼容的**原因** ——
        // 这条测试守住那个前提。
        let tools: Vec<Box<dyn InvokableTool>> = vec![Box::new(Echo)];
        assert_eq!(tools[0].name(), "echo");
    }

    #[test]
    fn the_chat_model_works_with_a_generic_caller() {
        // 泛型调用 —— **静态分发**，零虚调用开销。
        // 这条测试守住「`ChatModel` 不被 `dyn` 化」的前提。
        let model = Static;
        let cancel = tokio_util::sync::CancellationToken::new();
        let req = ChatRequest {
            messages: Vec::new(),
            tools: Vec::new(),
            temperature: 0.0,
            top_p: 1.0,
            max_tokens: 16,
            stream: false,
        };
        let response = futures::executor::block_on(model.chat(&req, &cancel)).expect("必须成功");
        assert_eq!(response.content, "ok");
    }

    #[test]
    fn a_tool_failure_keeps_its_business_code() {
        // 工具失败是 `UnknownOutcome` —— 工具可能已产生副作用。
        let err = AppError::new(ErrorCode::ToolFailed, "工具执行失败");
        assert_eq!(
            err.retry_policy(),
            nexus_core::error::RetryPolicy::UnknownOutcome
        );
    }
}
