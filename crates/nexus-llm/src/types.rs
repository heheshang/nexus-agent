//! 网关核心类型 —— 消息、请求、响应、流事件、模型描述。
//!
//! # 这些类型为什么在 `nexus-llm` 而不是 `nexus-model`
//!
//! `nexus-model` 是「**跨 crate 共享的**纯 DTO」（架构文档 §4.2 差异 2）——
//! 请求/响应结构、领域值对象，**不带任何序列化到数据库的痕迹**。
//!
//! 而这里的 `ChatRequest` / `ChatResponse` 只在 `nexus-llm` 与它的调用方
//! （`nexus-agent` / `nexus-service`）之间流转，**不需要** `nexus-model` 承载。
//!
//! 依赖方向因此是 `service → llm`（调用方依赖能力层），
//! 与 §4.4 规则 1 一致。
//!
//! # 与参考实现的对应关系
//!
//! | 本模块 | 参考实现 |
//! |---|---|
//! | [`Message`] | `internal/llm/types.go:14` |
//! | [`Role`] | `internal/llm/types.go:17` |
//! | [`ToolCall`] | `internal/llm/types.go:26` |
//! | [`ToolDef`] | `internal/llm/types.go:33` |
//! | [`ChatRequest`] | `internal/llm/types.go:44` |
//! | [`Usage`] | `internal/llm/types.go:53` |
//! | [`ChatResponse`] | `internal/llm/types.go:60` |
//! | [`StreamEvent`] | `internal/llm/types.go:70` |
//! | [`ModelDescriptor`] | `internal/llm/types.go:81` |
//!
//! # 三处**刻意**的安全设计
//!
//! 1. [`StreamEvent::err`] **不参与序列化** —— 内部错误原因绝不出现在对外响应（§2.6.5）。
//! 2. [`ToolDef`] 的**权威执行元数据**（`dangerous` / `replay_policy` / `source_identity` /
//!    `binding_version`）**不参与序列化** —— 那是控制面的东西，不是给模型看的。
//! 3. [`ModelDescriptor`] 的 `api_key` 用 [`SecretString`] 承载 —— `Debug` 输出自动脱敏。

use rust_decimal::Decimal;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 消息角色。
///
/// # 为什么是 `enum` 而不是 `String`
///
/// 角色取值只有四种 —— 用 `String` 会让「拼错角色名」变成**运行期**错误，
/// 而 `enum` 让它在**编译期**就暴露。
/// 而且角色会出现在协议判别上（`tool` 角色必须带 `tool_call_id`），
/// 类型化便于写穷尽匹配。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// 系统提示。
    System,
    /// 用户输入。
    User,
    /// 模型回复。
    Assistant,
    /// 工具执行结果。
    Tool,
}

impl Role {
    /// 协议里的字符串形式。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// 模型发起的一次工具调用。
///
/// # 为什么 `args` 是 `String` 而不是 `Value`
///
/// **协议如此** —— OpenAI 兼容协议把工具参数序列化成**字符串**，
/// 不是嵌套对象。参考实现 `internal/llm/types.go:29` 的注释写明「参数为 JSON 字符串（协议如此）」。
///
/// 这不是设计缺陷，是上游协议的约束 ——
/// 我们在**边界上**把它转成 `Value`（见 `ToolCall::parsed_args`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 ID。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// 参数（JSON 字符串）。
    pub args: String,
}

impl ToolCall {
    /// 把 `args` 解析成 [`Value`]。
    ///
    /// # 错误
    ///
    /// `args` 不是合法 JSON 时返回 `Err` —— 上游给了非法 JSON 是**协议违规**，
    /// 不能静默当成空参数。
    pub fn parsed_args(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(&self.args)
    }
}

/// 工具定义（传给模型的功能描述）。
///
/// # ⚠️ 权威执行元数据**不参与序列化**
///
/// `dangerous` / `replay_policy` / `source_identity` / `binding_version`
/// 是**控制面**的信息 —— 它们决定「这个工具能不能被调用、能不能重放」，
/// 是给**执行层**看的，**不是给模型看的**。
///
/// 把「这个工具是危险的」告诉模型，模型可能据此改变行为
/// （比如更谨慎地调用它，或者干脆不调用）——
/// 那是把控制面的判断泄漏给不可信的推理方。
///
/// 参考实现 `internal/llm/types.go:36-37` 的注释写明：
/// 「authoritative execution metadata, never sent to the model」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    /// 来源标识（控制面，不发给模型）。
    #[serde(skip)]
    pub source_identity: String,
    /// 绑定版本（控制面，不发给模型）。
    #[serde(skip)]
    pub binding_version: i64,
    /// 能力标识（控制面，不发给模型）。
    #[serde(skip)]
    pub capability: String,
    /// 是否危险（控制面，不发给模型）。
    #[serde(skip)]
    pub dangerous: bool,
    /// 重放策略（控制面，不发给模型）。
    #[serde(skip)]
    pub replay_policy: String,
    /// 工具名（发给模型）。
    pub name: String,
    /// 描述（发给模型）。
    pub description: String,
    /// 参数 JSON Schema（发给模型）。
    pub parameters: Value,
}

/// 单次对话请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    /// 消息列表。
    pub messages: Vec<Message>,
    /// 工具定义（**有工具时才传**）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    /// 采样温度。
    pub temperature: f32,
    /// 核采样。
    pub top_p: f32,
    /// 本次请求的输出 token 上限。
    pub max_tokens: i32,
    /// 是否流式。
    pub stream: bool,
}

/// 对话消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// 角色。
    pub role: Role,
    /// 文本内容。
    pub content: String,
    /// 推理模型的思考过程（`deepseek-r1` 等）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning_content: String,
    /// `assistant` 发起的工具调用。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// `tool` 角色对应的调用 ID。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_call_id: String,
    /// 可选名称。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Message {
    /// 构造一条普通消息（system / user / assistant）。
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            tool_call_id: String::new(),
            name: String::new(),
        }
    }

    /// 构造一条 `tool` 角色的消息。
    ///
    /// `tool_call_id` **必须**带上 —— 它是「这条结果对应哪次调用」的关联键，
    /// 缺失会让模型无法把结果与请求对上。
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            tool_call_id: tool_call_id.into(),
            name: String::new(),
        }
    }
}

/// Token 用量。
///
/// # `cost` 为什么是**十进制字符串**
///
/// 架构文档 §2.2.4.1 的 `usage` 事件负载里，`cost` 是 `Option<String>` ——
///
/// 注释写明：「用**十进制字符串**而不是 `f64` —— 浮点在跨系统传递中会丢精度，
/// 而成本是要**累加**并对账的数字」。
///
/// `model_configs.input_price` 是 `NUMERIC(12,6)` —— PG 的 NUMERIC 不是 IEEE754，
/// 只有 `rust_decimal::Decimal` 能解码（`f64` 在 sqlx 的类型兼容性检查处直接失败）。
///
/// 所以这里用 [`Decimal`] 承载计算精度，用**字符串**承载对外形状 —— 两者都不是多余的。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    /// 输入 token 数。
    pub prompt_tokens: i64,
    /// 输出 token 数。
    pub completion_tokens: i64,
    /// 成本（十进制字符串）。
    ///
    /// `None` 表示**未知** —— 上游没返回用量、或本次调用不计费。
    /// **不**默认成 `"0"` —— 那会把「没算出来」和「确实免费」混为一谈。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
}

impl Usage {
    /// 总 token 数。
    pub fn total(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }

    /// 是否**没有**任何用量。
    ///
    /// 用于「上游没返回 usage」的情况 —— 那时不该计入 token 指标
    /// （参考实现 `internal/llm/gateway.go:399` 的判断口径：
    /// `u.PromptTokens+u.CompletionTokens > 0`）。
    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// 非流式响应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    /// 回答文本。
    pub content: String,
    /// 推理过程（可选）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning_content: String,
    /// 模型要求的工具调用。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// 结束原因（`stop` / `tool_calls` / `length`）。
    pub finish_reason: String,
    /// 实际上游模型名。
    pub model: String,
    /// Token 用量。
    pub usage: Usage,
}

/// 流式事件（网关到调用方的统一流协议）。
///
/// # ⚠️ `err` **不参与序列化**
///
/// 内部错误原因绝不出现在对外响应（§2.6.5）。
/// 参考实现 `internal/llm/types.go:72` 的注释写明「never serialized to clients」。
///
/// 它用 `Option` 而不是裸字段 —— `None` 表示「没有错误」，
/// 序列化时整个字段被跳过。
///
/// # ⚠️ **不实现 `Deserialize`**
///
/// `#[serde(skip)]` 有一个副作用：反序列化时该字段取默认值（`None`）。
/// 那意味着「从 JSON 里塞一个错误进来」在**运行期静默失效** ——
///
/// 而**静默失效比编译失败更危险**（你以为它失败了，其实没有）。
///
/// 所以本类型只实现 `Serialize` —— 「从 wire 上还原一个带错误的流事件」
/// 在**编译期就不可能**。负向测试：
/// `tests/compile_fail/stream_event_cannot_be_deserialized.rs`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StreamEvent {
    /// 终态的 provider / 传输层错误。
    ///
    /// **不序列化** —— 见模块文档。
    #[serde(skip)]
    pub err: Option<String>,
    /// 增量文本。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    /// 增量推理内容。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning_content: String,
    /// 聚合完成的工具调用（最后一次出现）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// 结束原因（**仅最后一帧携带**）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub finish_reason: String,
    /// 用量（部分上游在最后一帧携带）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// 实际上游模型名。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

impl StreamEvent {
    /// 构造一个增量文本事件。
    pub fn content(content: impl Into<String>) -> Self {
        Self {
            err: None,
            content: content.into(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            finish_reason: String::new(),
            usage: None,
            model: String::new(),
        }
    }

    /// 构造一个终态错误事件。
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            err: Some(message.into()),
            content: String::new(),
            reasoning_content: String::new(),
            tool_calls: Vec::new(),
            finish_reason: String::new(),
            usage: None,
            model: String::new(),
        }
    }

    /// 是否是**终态错误**事件。
    pub fn is_error(&self) -> bool {
        self.err.is_some()
    }
}

/// 模型描述（网关按它实例化 provider 请求）。
///
/// # `api_key` 为什么是 [`SecretString`]
///
/// 它承载**解密后的明文口令**。参考实现里它是裸 `string`
/// （`internal/llm/types.go:86`）—— 那意味着任何一次 `fmt.Println` 都会泄露它。
///
/// Rust 侧用 [`SecretString`] 承载：
///
/// - `Debug` 输出自动脱敏（`SecretBox` 的 `Debug` 实现不打印内容）
/// - 需要真正使用时显式调 [`ExposeSecret::expose_secret`]
///
/// 这比「自己记得别打印」可靠 —— **类型上**就不容易泄露。
/// 与 `nexus-infra` 的 PG URL 同一纪律。
#[derive(Clone)]
pub struct ModelDescriptor {
    /// 不可变的输出上限（配置值）；`max_tokens` 是每次请求的值。
    pub max_output_tokens: i32,
    /// 上下文窗口大小（token 数）。`0` 表示**未知** —— 此时不做预算裁剪。
    pub context_window: i32,
    /// 供应商代号（`deepseek` / `qwen` / `ollama` / ...）。
    pub provider_code: String,
    /// 供应商 API 地址。
    pub base_url: String,
    /// 解密后的密钥。**secret**。
    api_key: SecretString,
    /// 上游模型名。
    pub model_name: String,
    /// 采样温度。
    pub temperature: f32,
    /// 核采样。
    pub top_p: f32,
    /// 本次请求的输出 token 上限。
    pub max_tokens: i32,
    /// 输入单价（每 1M token）。`Decimal` —— 不是 `f64`（见 [`Usage::cost`]）。
    pub input_price: Decimal,
    /// 输出单价（每 1M token）。
    pub output_price: Decimal,
    /// 模型类型（`chat` / `embedding` / `rerank`）。
    pub model_type: ModelType,
    /// 模型能力标记（`vision` / `function_call` / `reasoning`）。
    pub capabilities: Vec<String>,
}

/// 模型类型。
///
/// 对应参考实现 `internal/model/llm.go:22-26` 的三个常量。
///
/// # 为什么是 `enum` 而不是 `String`
///
/// 同 [`Role`] 的理由 —— 取值封闭，类型上就能穷尽匹配。
/// 而且「这个模型是聊天模型还是向量模型」决定了**能不能挂工具**
/// （`function_call` 能力），用 `String` 会让拼错的类型静默通过。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    /// 对话模型。
    #[default]
    Chat,
    /// 向量模型。
    Embedding,
    /// 重排模型。
    Rerank,
}

impl ModelType {
    /// 协议里的字符串形式。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Embedding => "embedding",
            Self::Rerank => "rerank",
        }
    }
}

impl ModelDescriptor {
    /// 构造模型描述。
    ///
    /// `api_key` 传明文 —— 内部用 [`SecretString`] 承载。
    pub fn new(
        provider_code: impl Into<String>,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model_name: impl Into<String>,
    ) -> Self {
        Self {
            max_output_tokens: 0,
            context_window: 0,
            provider_code: provider_code.into(),
            base_url: base_url.into(),
            api_key: SecretString::from(api_key.into()),
            model_name: model_name.into(),
            temperature: 0.0,
            top_p: 1.0,
            max_tokens: 0,
            input_price: Decimal::ZERO,
            output_price: Decimal::ZERO,
            model_type: ModelType::Chat,
            capabilities: Vec::new(),
        }
    }

    /// 是否具备某个能力。
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }

    /// API 密钥（**明文**）。
    ///
    /// 刻意命名得啰嗦 —— 调用它意味着「我知道我在做什么」。
    ///
    /// 只在**真正要发请求**的地方用。不要把返回值打日志或存起来。
    pub fn expose_api_key(&self) -> &str {
        use secrecy::ExposeSecret;
        self.api_key.expose_secret()
    }
}

/// 手写 `Debug` —— `api_key` 永远显示为 `[REDACTED]`。
///
/// # 为什么不用 `#[derive(Debug)]`
///
/// 派生会原样打印内部字段。而 `SecretString` 的 `Debug` 确实会脱敏 ——
/// 但**依赖被依赖类型的 `Debug` 实现**是脆弱的：
/// 将来 `secrecy` 改行为，或有人把字段类型换成 `String`，泄露就发生了。
///
/// 手写才能**保证**这个类型在任何情况下都不打印口令。
impl std::fmt::Debug for ModelDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelDescriptor")
            .field("provider_code", &self.provider_code)
            .field("base_url", &self.base_url)
            .field("model_name", &self.model_name)
            .field("api_key", &"[REDACTED]")
            .field("context_window", &self.context_window)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("max_tokens", &self.max_tokens)
            .field("temperature", &self.temperature)
            .field("top_p", &self.top_p)
            .field("input_price", &self.input_price)
            .field("output_price", &self.output_price)
            .field("model_type", &self.model_type)
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

/// 供应商后端 —— **enum + match**，不是 `Box<dyn>`。
///
/// # 为什么不是 `dyn`
///
/// 架构文档 §2.3.2 明确「**enum 优于 `Box<dyn>`**」，理由有三：
///
/// 1. **无虚调用开销**（热路径：每次模型调用）
/// 2. **错误类型不变宽** —— `dyn` 会让错误类型从具体变成 `Box<dyn Error>`
/// 3. **不需要 `async_trait` 的 boxing** —— 可以用原生 `async fn` in trait
///
/// # 那 §2.3.3 的「`dyn` 保留场景」呢
///
/// §2.3.3 说 `dyn` 只在**异构集合**时才必需。
/// 供应商**不是**异构集合 —— 它们是「同质的多个实现」，用 `enum` 装下就行。
///
/// 真正的异构集合是 `ToolRegistry`（内置 / MCP / 用户自定义的工具）——
/// 那是 P8 的事。
///
/// # 代价
///
/// 新增供应商要改这个 `enum`。但那本来就是「新增一种模型接入方式」，
/// 本来就需要改代码（§2.3.2 的原话）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelBackend {
    /// OpenAI 兼容协议家族 —— deepseek / openai / qwen / zhipu / moonshot /
    /// doubao，以及 ollama 的 `/v1` 兼容层。
    OpenAiCompatible,
}

impl ModelBackend {
    /// 按供应商代号选后端。
    ///
    /// # 当前只有一个变体
    ///
    /// 参考实现 `internal/llm/provider.go:33-43` 也是这个形状 ——
    /// 所有 OpenAI 兼容供应商共用同一个实现，只有配置差异；
    /// 豆包 Ark 与 ollama 也是同一协议。
    ///
    /// 等真正出现**协议形状不同**的供应商时再拆变体 ——
    /// 那时拆是必要的，现在拆是**过早泛化**。
    pub fn for_provider(_provider_code: &str) -> Self {
        Self::OpenAiCompatible
    }
}
