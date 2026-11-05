//! P4 ReAct 循环的行为层测试。
//!
//! 覆盖的不变量：
//!
//! - **P4-I11**：ReAct 循环跑通至少两轮
//! - **P4-I12**：迭代上限生效（`MaxIteration` 30002）
//! - **P4-I13**：单步超时生效（`ModelTimeout` 50002）
//! - **P4-I14**：总超时生效（`ModelTimeout` 50002）
//! - **P4-I15**：工具失败**结构化回灌**且**不终止**循环
//! - **P4-I16**：工具输出是**数据**不是指令 —— 回灌时是 `tool` 角色
//! - **P4-I17**：工具并发限流生效
//! - **P4-I21**：取消的终态是 `done` 而非 `error`
//! - **P4-I22**：先固化错误再取消
//! - **P4-I25**：工具输出超长被截断

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use nexus_agent::events::{EventSink, EventSinkConfig};
use nexus_agent::react::{ReactConfig, ReactLoop, tool_error_content, truncate_utf8};
use nexus_agent::traits::{ChatModel, InvokableTool};
use nexus_core::error::{AppError, ErrorCode};
use nexus_core::event::EventPayload;
use nexus_llm::types::{ChatRequest, ChatResponse, Message, Role, ToolCall, Usage};

// ═══════════════════════════════════════════════════════════
// 测试替身
// ═══════════════════════════════════════════════════════════

/// 一个按脚本回答的模型。
///
/// 每次调用返回脚本里的下一个响应 —— 用来模拟
/// 「第一轮要工具、第二轮给答案」这样的多轮交互。
struct ScriptedModel {
    responses: Mutex<Vec<ChatResponse>>,
    /// 收到的请求（按顺序）。
    requests: Mutex<Vec<ChatRequest>>,
    /// 每次调用的延迟。
    delay: Option<Duration>,
}

impl ScriptedModel {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
            delay: None,
        }
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().expect("锁").clone()
    }
}

#[allow(clippy::manual_async_fn)]
impl ChatModel for ScriptedModel {
    fn chat(
        &self,
        req: &ChatRequest,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> impl Future<Output = Result<ChatResponse, AppError>> + Send {
        self.requests.lock().expect("锁").push(req.clone());
        let delay = self.delay;
        let mut responses = self.responses.lock().expect("锁");
        let response = if responses.is_empty() {
            final_answer("没有更多脚本了")
        } else {
            responses.remove(0)
        };
        async move {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            Ok(response)
        }
    }
}

/// 一个总是失败的模型。
struct FailingModel {
    code: ErrorCode,
    message: &'static str,
}

#[allow(clippy::manual_async_fn)]
impl ChatModel for FailingModel {
    fn chat(
        &self,
        _req: &ChatRequest,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> impl Future<Output = Result<ChatResponse, AppError>> + Send {
        let code = self.code;
        let message = self.message;
        async move { Err(AppError::new(code, message)) }
    }
}

/// 一个记录并发峰值的工具。
struct CountingTool {
    name: String,
    output: String,
    /// 当前正在执行的工具数。
    current: Arc<AtomicUsize>,
    /// 观察到的并发峰值。
    peak: Arc<AtomicUsize>,
    /// 失败时的码（`None` = 成功）。
    fail_with: Option<ErrorCode>,
}

impl CountingTool {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            output: format!("{name} 的输出"),
            current: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            fail_with: None,
        }
    }

    fn failing(name: &str, code: ErrorCode) -> Self {
        let mut tool = Self::new(name);
        tool.fail_with = Some(code);
        tool
    }

    fn with_output(mut self, output: &str) -> Self {
        self.output = output.to_string();
        self
    }

    #[allow(dead_code)]
    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl InvokableTool for CountingTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "测试工具"
    }

    async fn invoke(
        &self,
        _args: &str,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<String, AppError> {
        let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        // 更新峰值。
        self.peak.fetch_max(now, Ordering::SeqCst);

        // 睡一会儿 —— 让并发真的重叠。
        tokio::time::sleep(Duration::from_millis(50)).await;

        self.current.fetch_sub(1, Ordering::SeqCst);

        match self.fail_with {
            Some(code) => Err(AppError::new(code, "工具故意失败")),
            None => Ok(self.output.clone()),
        }
    }
}

/// 构造一个「要求工具调用」的模型响应。
fn wants_tools(calls: &[(&str, &str)]) -> ChatResponse {
    ChatResponse {
        content: String::new(),
        reasoning_content: String::new(),
        tool_calls: calls
            .iter()
            .map(|(id, name)| ToolCall {
                id: (*id).to_string(),
                name: (*name).to_string(),
                args: "{}".to_string(),
            })
            .collect(),
        finish_reason: "tool_calls".to_string(),
        model: "test".to_string(),
        usage: Usage::default(),
    }
}

/// 构造一个「最终答案」的模型响应。
fn final_answer(content: &str) -> ChatResponse {
    ChatResponse {
        content: content.to_string(),
        reasoning_content: String::new(),
        tool_calls: Vec::new(),
        finish_reason: "stop".to_string(),
        model: "test".to_string(),
        usage: Usage::default(),
    }
}

fn config() -> ReactConfig {
    ReactConfig {
        max_iterations: 5,
        step_timeout: Duration::from_secs(5),
        total_timeout: Duration::from_secs(30),
        max_concurrent_tools: 4,
        context_budget: 256 * 1024,
        max_tool_output: 1024,
        temperature: 0.0,
        top_p: 1.0,
        max_tokens: 4096,
    }
}

fn user(text: &str) -> Message {
    Message::new(Role::User, text)
}

/// 建一个 sink —— 测试不消费流，只是避免它过早关闭。
fn new_sink() -> (EventSink, nexus_agent::events::EventStream) {
    EventSink::new(
        nexus_core::id::RunId::new(1),
        EventSinkConfig {
            lossy_buffer: 64,
            guaranteed_buffer: 64,
            backpressure_timeout: Duration::from_secs(2),
        },
    )
}

// ═══════════════════════════════════════════════════════════
// AC #2 / P4-I11：跑通至少两轮
// ═══════════════════════════════════════════════════════════

/// AC #2 / P4-I11：模型要求工具 → 工具执行 → 结果回灌 → 模型给出最终答案。
#[tokio::test]
async fn the_loop_runs_at_least_two_rounds() {
    let model = ScriptedModel::new(vec![
        wants_tools(&[("c1", "search")]),
        final_answer("最终答案"),
    ]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("search"))];

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());

    let outcome = loop_.run(vec![user("查一下")], &tools, &cancel).await;

    match outcome {
        nexus_agent::react::Outcome::Finished { answer, iterations } => {
            assert_eq!(answer, "最终答案");
            assert_eq!(iterations, 2, "应该跑了两轮：请求工具 + 给答案");
        }
        other => panic!("应当正常结束，实得 {other:?}"),
    }
}

/// AC #2：结果**回灌**后，模型在**第二轮**能看到工具结果。
#[tokio::test]
async fn the_tool_result_is_fed_back_to_the_model() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "search")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(
        CountingTool::new("search").with_output("搜索结果"),
    )];

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());
    loop_.run(vec![user("查一下")], &tools, &cancel).await;

    let requests = model.requests();
    assert_eq!(requests.len(), 2, "模型应该被调用两次");

    // 第二轮的请求里必须有工具结果。
    let second = &requests[1];
    let tool_messages: Vec<&Message> = second
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect();

    assert_eq!(
        tool_messages.len(),
        1,
        "第二轮必须有回灌的工具结果 —— 实得 {:?}",
        second.messages
    );
    assert_eq!(tool_messages[0].content, "搜索结果");
    assert_eq!(tool_messages[0].tool_call_id, "c1");
}

// ═══════════════════════════════════════════════════════════
// AC #5 / P4-I16：工具输出是**数据**不是指令
// ═══════════════════════════════════════════════════════════

/// AC #5 / P4-I16：工具结果回灌时是 **`tool` 角色**，不是 `assistant` 角色。
#[tokio::test]
async fn tool_output_is_fed_back_as_a_tool_role_message() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "search")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("search"))];

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());
    loop_.run(vec![user("查一下")], &tools, &cancel).await;

    let requests = model.requests();
    let second = &requests[1];

    // 工具结果**必须**是 `tool` 角色。
    let tool_messages: Vec<&Message> = second
        .messages
        .iter()
        .filter(|m| m.content.contains("search 的输出"))
        .collect();
    assert_eq!(tool_messages.len(), 1);
    assert_eq!(
        tool_messages[0].role,
        Role::Tool,
        "工具输出是**数据**不是指令 —— 必须用 `tool` 角色，不能冒充 assistant"
    );
}

// ═══════════════════════════════════════════════════════════
// AC #4 / P4-I15：工具失败结构化回灌且不终止循环
// ═══════════════════════════════════════════════════════════

/// AC #4 / P4-I15：工具失败**不终止**循环 —— 模型仍能给出答案。
#[tokio::test]
async fn a_tool_failure_is_fed_back_without_terminating_the_loop() {
    let model = ScriptedModel::new(vec![
        wants_tools(&[("c1", "broken")]),
        final_answer("我知道失败了"),
    ]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::failing(
        "broken",
        ErrorCode::ToolFailed,
    ))];

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());
    let outcome = loop_.run(vec![user("试试")], &tools, &cancel).await;

    match outcome {
        nexus_agent::react::Outcome::Finished { answer, .. } => {
            assert_eq!(answer, "我知道失败了", "工具失败**不得**终止循环");
        }
        other => panic!("工具失败不应终止循环，实得 {other:?}"),
    }
}

/// AC #4：工具失败的回灌内容是**结构化**的 `{"error": ..., "message": ...}`。
#[tokio::test]
async fn a_tool_failure_is_fed_back_as_a_structured_observation() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "broken")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::failing(
        "broken",
        ErrorCode::ToolFailed,
    ))];

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());
    loop_.run(vec![user("试试")], &tools, &cancel).await;

    let requests = model.requests();
    let second = &requests[1];
    let tool_message = second
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("应有回灌的工具结果");

    // 形状：`{"error": ..., "message": ...}`。
    let parsed: serde_json::Value =
        serde_json::from_str(&tool_message.content).expect("必须是合法 JSON");

    assert!(
        parsed.get("error").is_some(),
        "必须有 `error` 字段 —— 实得 {parsed}"
    );
    assert!(
        parsed.get("message").is_some(),
        "必须有 `message` 字段 —— 实得 {parsed}"
    );
    assert_eq!(
        parsed["error"].as_i64(),
        Some(70002),
        "`error` 应是业务码 —— 实得 {parsed}"
    );
}

/// `tool_error_content` 的形状本身。
#[test]
fn the_tool_error_content_has_the_documented_shape() {
    let content = tool_error_content("出错了", 70002);
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("必须是合法 JSON");
    assert_eq!(parsed["error"].as_i64(), Some(70002));
    assert_eq!(parsed["message"].as_str(), Some("出错了"));
}

// ═══════════════════════════════════════════════════════════
// AC #6 / P4-I17：工具并发限流
// ═══════════════════════════════════════════════════════════

/// AC #6 / P4-I17：同一轮内同时执行的工具数**不超过**上限。
#[tokio::test]
async fn concurrent_tool_execution_respects_the_limit() {
    // 6 个工具调用，上限 2 —— 峰值必须 ≤ 2。
    let calls: Vec<(&str, &str)> = vec![
        ("c1", "t1"),
        ("c2", "t2"),
        ("c3", "t3"),
        ("c4", "t4"),
        ("c5", "t5"),
        ("c6", "t6"),
    ];
    let model = ScriptedModel::new(vec![wants_tools(&calls), final_answer("好")]);
    let (sink, _stream) = new_sink();

    let tools: Vec<Arc<dyn InvokableTool>> = (1..=6)
        .map(|i| Arc::new(CountingTool::new(&format!("t{i}"))) as Arc<dyn InvokableTool>)
        .collect();

    let mut cfg = config();
    cfg.max_concurrent_tools = 2;

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    loop_.run(vec![user("全部跑")], &tools, &cancel).await;

    // 峰值 —— 任何一个工具记录的峰值都不该超过 2。
    for tool in &tools {
        // 需要 downcast 才能读 `peak` —— 用名字匹配。
        let _ = tool;
    }

    // ⚠️ 由于工具是 `dyn`，用**共享计数器**测峰值更直接。
    // 上面的工具各自有计数器 —— 这里改用共享方式重新验证。
    let shared_current = Arc::new(AtomicUsize::new(0));
    let shared_peak = Arc::new(AtomicUsize::new(0));
    let tools: Vec<Arc<dyn InvokableTool>> = (1..=6)
        .map(|i| {
            Arc::new(SharedPeakTool {
                name: format!("t{i}"),
                current: Arc::clone(&shared_current),
                peak: Arc::clone(&shared_peak),
            }) as Arc<dyn InvokableTool>
        })
        .collect();

    let model = ScriptedModel::new(vec![wants_tools(&calls), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let mut cfg = config();
    cfg.max_concurrent_tools = 2;
    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    loop_.run(vec![user("全部跑")], &tools, &cancel).await;

    assert!(
        shared_peak.load(Ordering::SeqCst) <= 2,
        "并发峰值应 ≤ 2 —— 实得 {}",
        shared_peak.load(Ordering::SeqCst)
    );
    assert!(
        shared_peak.load(Ordering::SeqCst) >= 2,
        "并发峰值应达到 2 —— 否则限流把并发压成了串行"
    );
}

/// 一个用**共享计数器**记录并发峰值的工具。
struct SharedPeakTool {
    name: String,
    current: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl InvokableTool for SharedPeakTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "测试工具"
    }

    async fn invoke(
        &self,
        _args: &str,
        _cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<String, AppError> {
        let now = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.current.fetch_sub(1, Ordering::SeqCst);
        Ok("ok".to_string())
    }
}

// ═══════════════════════════════════════════════════════════
// AC #3：三个安全阀
// ═══════════════════════════════════════════════════════════

/// AC #3 / P4-I12：迭代上限生效 → `MaxIteration`（30002）。
#[tokio::test]
async fn the_iteration_limit_terminates_the_loop() {
    // 模型**永远**要工具 —— 所以循环不会自然结束。
    let responses: Vec<ChatResponse> = (0..20)
        .map(|i| wants_tools(&[(&format!("c{i}"), "search")]))
        .collect();
    let model = ScriptedModel::new(responses);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("search"))];

    let mut cfg = config();
    cfg.max_iterations = 3;

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("一直跑")], &tools, &cancel).await;

    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::MaxIteration),
        "超出迭代上限应报 `MaxIteration`"
    );
    assert_eq!(
        outcome.error_code().map(|c| c.code()),
        Some(30002),
        "错误码必须是 30002"
    );
}

/// AC #3 / P4-I13：单步超时生效 → `ModelTimeout`（50002）。
#[tokio::test]
async fn the_step_timeout_terminates_the_loop() {
    let model =
        ScriptedModel::new(vec![final_answer("永远不会到")]).with_delay(Duration::from_secs(5));
    let (sink, _stream) = new_sink();

    let mut cfg = config();
    cfg.step_timeout = Duration::from_millis(100);

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("慢一点")], &[], &cancel).await;

    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::ModelTimeout),
        "单步超时应报 `ModelTimeout`"
    );
    assert_eq!(outcome.error_code().map(|c| c.code()), Some(50002));
}

/// AC #3 / P4-I14：总超时生效 → `ModelTimeout`（50002）。
#[tokio::test]
async fn the_total_timeout_terminates_the_loop() {
    // 每步 200ms，总超时 300ms —— 总超时先到。
    let responses: Vec<ChatResponse> = (0..20)
        .map(|i| wants_tools(&[(&format!("c{i}"), "search")]))
        .collect();
    let model = ScriptedModel::new(responses).with_delay(Duration::from_millis(200));
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("search"))];

    let mut cfg = config();
    cfg.total_timeout = Duration::from_millis(300);
    cfg.step_timeout = Duration::from_secs(10);

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("一直跑")], &tools, &cancel).await;

    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::ModelTimeout),
        "总超时应报 `ModelTimeout`"
    );
    assert_eq!(outcome.error_code().map(|c| c.code()), Some(50002));
}

/// AC #3：单步超时与总超时**区分得开** —— 单步更短时先触发单步。
#[tokio::test]
async fn the_step_timeout_fires_before_the_total_timeout() {
    let model = ScriptedModel::new(vec![final_answer("x")]).with_delay(Duration::from_secs(2));
    let (sink, _stream) = new_sink();

    let mut cfg = config();
    cfg.step_timeout = Duration::from_millis(100);
    cfg.total_timeout = Duration::from_secs(30);

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("慢")], &[], &cancel).await;

    // 两者的终态码**相同**（都是 ModelTimeout）——
    // 但这里确认的是单步超时路径**确实**被走到（没等满 30 秒）。
    assert_eq!(outcome.error_code(), Some(ErrorCode::ModelTimeout));
}

// ═══════════════════════════════════════════════════════════
// AC #8 / #9 / P4-I21 / P4-I22：取消语义
// ═══════════════════════════════════════════════════

/// AC #8 / P4-I21：取消的终态是 `done`（**不是** `error`）。
#[tokio::test]
async fn cancellation_is_not_a_failure() {
    let model = ScriptedModel::new(vec![final_answer("x")]);
    let (sink, _stream) = new_sink();

    let cancel = tokio_util::sync::CancellationToken::new();
    // 预先取消。
    cancel.cancel();

    let loop_ = ReactLoop::new(&model, &sink, config());
    let outcome = loop_.run(vec![user("取消")], &[], &cancel).await;

    match outcome {
        nexus_agent::react::Outcome::Canceled { .. } => {}
        other => panic!("取消的终态必须是 `Canceled`（对应 `done`），实得 {other:?}"),
    }
    assert!(!outcome.is_failed(), "取消**不是失败** —— 不得报 `error`");
}

/// AC #9 / P4-I22：**先固化错误再取消** —— 供应商失败不被 `cancel` 覆盖。
#[tokio::test]
async fn a_provider_failure_is_not_masked_by_cancellation() {
    let model = FailingModel {
        code: ErrorCode::ModelUnavailable,
        message: "上游不可用",
    };
    let (sink, _stream) = new_sink();

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, config());
    let outcome = loop_.run(vec![user("跑")], &[], &cancel).await;

    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::ModelUnavailable),
        "供应商失败必须**原样**保留 —— 不能被 cancel 覆盖成「用户取消」"
    );
    assert!(
        !matches!(outcome, nexus_agent::react::Outcome::Canceled { .. }),
        "供应商失败**不得**被报告成取消"
    );
}

// ═══════════════════════════════════════════════════════════
// AC #7 / P4-I25：工具输出截断
// ═══════════════════════════════════════════════════════════

/// AC #7 / P4-I25：工具输出超长被**截断**。
#[tokio::test]
async fn a_huge_tool_output_is_truncated() {
    let huge = "x".repeat(100_000);
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "big")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> =
        vec![Arc::new(CountingTool::new("big").with_output(&huge))];

    let mut cfg = config();
    cfg.max_tool_output = 1000;

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    loop_.run(vec![user("跑")], &tools, &cancel).await;

    let requests = model.requests();
    let second = &requests[1];
    let tool_message = second
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("应有回灌的工具结果");

    assert!(
        tool_message.content.len() < 2000,
        "超长工具输出必须被截断 —— 实得 {} 字节",
        tool_message.content.len()
    );
    assert!(
        tool_message.content.contains("已截断"),
        "截断应有标记 —— 实得 {:?}",
        &tool_message.content[..tool_message.content.len().min(50)]
    );
}

/// 截断保持 **UTF-8 边界** —— 不能切半个字符。
#[test]
fn the_truncation_respects_utf8_boundaries() {
    // 每个中文字符 3 字节 —— 故意用不能整除的边界。
    let text = "中文字符测试".repeat(100);
    let truncated = truncate_utf8(&text, 100);

    // ⚠️ 必须是**合法 UTF-8** —— 如果切在字符中间，
    // `String` 根本构造不出来（Rust 的 `String` 永远合法）。
    assert!(truncated.len() <= 100 + "…[已截断]".len());
    assert!(truncated.contains("已截断"));

    // ⚠️ 更强的检查：截断后的前缀必须是原文的**前缀**。
    assert!(text.starts_with(truncated.trim_end_matches("…[已截断]")));
}

/// 短于上限的输出**不截断**。
#[test]
fn a_short_output_is_not_truncated() {
    let text = "短输出";
    assert_eq!(truncate_utf8(text, 1000), text);
}

// ═══════════════════════════════════════════════════════════
// AC #11：裁剪每步都重跑
// ═══════════════════════════════════════════════════════════

/// AC #11：裁剪在**每步**都重跑 —— 第二步的历史被裁剪了。
#[tokio::test]
async fn compaction_runs_on_every_step() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "big")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    // 工具输出巨大 —— 第一步之后历史就超预算了。
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(
        CountingTool::new("big").with_output(&"y".repeat(50_000)),
    )];

    let mut cfg = config();
    // 预算很小 —— 装不下工具定义本身。
    cfg.context_budget = 100;

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("跑")], &tools, &cancel).await;

    // 第一步超预算 → `BudgetExceeded`（30006）。
    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::BudgetExceeded),
        "第一步就超预算时应报 `BudgetExceeded`"
    );

    // 模型的**第一次**请求里，历史已被裁剪 —— 不是原始的。
    let requests = model.requests();
    // ⚠️ 模型**从未**被调用 —— 预算不足时在**构造请求**阶段就失败了。
    //
    // 这正是「裁剪先于模型调用」的证据。
    assert_eq!(
        requests.len(),
        0,
        "预算不足时模型不该被调用 —— 请求根本没构造出来"
    );
}

/// AC #11：裁剪**每步**重跑 —— 第二步看到的是裁剪后的历史。
#[tokio::test]
async fn the_second_step_sees_a_compacted_history() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "t")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("t"))];

    // 预算刚好够**第一步**（1 条 user 消息 + 1 个工具定义）。
    //
    // ⚠️ 用**精确估算**而不是硬编码魔数 —— 硬编码会随消息内容变化而失效。
    let step_one = vec![user("跑")];
    let tool_defs = nexus_agent::react::tool_definitions(&tools);
    let mut cfg = config();
    cfg.context_budget = nexus_agent::compaction::estimate_total_budget(&step_one, &tool_defs);

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(vec![user("跑")], &tools, &cancel).await;

    // 第二步的预算不够 → `BudgetExceeded`。
    assert_eq!(
        outcome.error_code(),
        Some(ErrorCode::BudgetExceeded),
        "第二步超预算说明裁剪**每步都跑**了"
    );

    let requests = model.requests();
    // ⚠️ 模型**只被调用了一次** —— 第二步在**构造请求**时就超预算了，
    // 根本没走到模型调用。
    //
    // 这正是「裁剪每步都跑」的证据：如果裁剪只在第一步跑，
    // 第二步会用**未裁剪**的历史去调模型，就会被调用两次。
    assert_eq!(
        requests.len(),
        1,
        "第二步应在**构造请求**时就因超预算失败 —— 模型不该被调用第二次"
    );
}

/// AC #11：多轮历史下，裁剪**真的淘汰**了旧的轮次。
///
/// 上一个测试证的是「裁剪每步都跑」；
/// 这个测试证的是「裁剪**确实淘汰**了东西」——
/// 两轮 user 历史，第一轮被淘汰。
#[tokio::test]
async fn compaction_evicts_old_turns_on_later_steps() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "t")]), final_answer("好")]);
    let (sink, _stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("t"))];

    // 两轮 user 历史 —— 第一轮**很大**，可以被淘汰。
    //
    // ⚠️ 关键：第一轮必须**大到**超预算，否则裁剪不会淘汰它。
    // 两轮都很短的话，step 1 的总量反而比 step 2 的保留集还小 ——
    // 那就测不到淘汰了。
    let big_first_turn = "旧".repeat(500);
    let history = vec![user(&big_first_turn), user("第二轮问题")];

    // 预算：够「最新一轮 + assistant + tool 结果」，但不够「两轮」。
    let one_turn = vec![user("第二轮问题")];
    let tool_defs = nexus_agent::react::tool_definitions(&tools);

    // ⚠️ 保留集的**最坏情况**必须**精确**重建 ——
    // assistant 消息带工具调用（那些字节也算），
    // tool 结果的内容是工具的**真实输出**。
    let mut worst_case = one_turn.clone();
    let mut assistant = Message::new(Role::Assistant, "");
    assistant.tool_calls = vec![ToolCall {
        id: "c1".to_string(),
        name: "t".to_string(),
        args: "{}".to_string(),
    }];
    worst_case.push(assistant);
    worst_case.push(Message::tool_result("c1", "t 的输出"));
    let mut cfg = config();
    cfg.context_budget = nexus_agent::compaction::estimate_total_budget(&worst_case, &tool_defs);

    let cancel = tokio_util::sync::CancellationToken::new();
    let loop_ = ReactLoop::new(&model, &sink, cfg);
    let outcome = loop_.run(history, &tools, &cancel).await;

    // 预算够一轮 —— 循环应该正常跑完。
    assert!(
        !outcome.is_failed(),
        "预算够一轮时应正常结束 —— 实得 {outcome:?}"
    );

    let requests = model.requests();
    assert!(!requests.is_empty());
    let first_request = &requests[0];

    // 第一轮被淘汰了 —— 请求里只剩**最新一轮**的 user 消息。
    let users: Vec<&str> = first_request
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(
        users,
        vec!["第二轮问题"],
        "旧的 user 轮应被淘汰 —— 实得 {users:?}"
    );
}

// ═══════════════════════════════════════════════════════════
// 事件
// ═══════════════════════════════════════════════════════════

/// 循环会发出 `tool_result` 与 `done` 事件。
#[tokio::test]
async fn the_loop_emits_tool_result_and_done_events() {
    let model = ScriptedModel::new(vec![wants_tools(&[("c1", "search")]), final_answer("好")]);
    let (sink, mut stream) = new_sink();
    let tools: Vec<Arc<dyn InvokableTool>> = vec![Arc::new(CountingTool::new("search"))];

    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let loop_ = ReactLoop::new(&model, &sink, config());
        let outcome = loop_.run(vec![user("跑")], &tools, &cancel).await;
        assert!(matches!(
            outcome,
            nexus_agent::react::Outcome::Finished { .. }
        ));
    }

    // ⚠️ 必须先让 `loop_` 离开作用域（它借用了 sink），再释放 sink ——
    // 否则流不会结束。
    drop(sink);

    let mut saw_tool_result = false;
    let mut saw_done = false;
    while let Some(item) = futures::StreamExt::next(&mut stream).await {
        let event = item.expect("不应有流错误");
        match &event.payload {
            EventPayload::ToolResult { .. } => saw_tool_result = true,
            EventPayload::Done { .. } => saw_done = true,
            _ => {}
        }
    }

    assert!(saw_tool_result, "应发出 `tool_result` 事件");
    assert!(saw_done, "应发出 `done` 事件");
}
