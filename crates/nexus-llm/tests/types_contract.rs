//! 网关核心类型的**契约测试**。
//!
//! # 为什么需要契约测试
//!
//! 这些类型将来会被 `nexus-api` 序列化成对外 JSON ——
//! 一旦字段名变了，已上线的前端就失效了。
//!
//! 参考实现的 `internal/llm/types.go` 是**协议的事实来源** ——
//! 它的 JSON tag 是与上游模型供应商（OpenAI 兼容协议）协商出来的，
//! 改一个字就意味着模型不认我们的请求了。
//!
//! 所以本测试把 JSON tag **逐字段**钉死。
//!
//! # 参考实现对应关系
//!
//! | 本文件断言的类型 | 参考实现位置 |
//! |---|---|
//! | `Role` | `internal/llm/types.go:17-24` |
//! | `Message` | `internal/llm/types.go:14-25` |
//! | `ToolCall` | `internal/llm/types.go:26-31` |
//! | `ToolDef` | `internal/llm/types.go:33-42` |
//! | `ChatRequest` | `internal/llm/types.go:44-51` |
//! | `Usage` | `internal/llm/types.go:53-56` |
//! | `ChatResponse` | `internal/llm/types.go:60-68` |
//! | `StreamEvent` | `internal/llm/types.go:70-78` |

use nexus_llm::types::{
    ChatRequest, ChatResponse, Message, ModelBackend, ModelDescriptor, ModelType, Role,
    StreamEvent, ToolCall, ToolDef, Usage,
};
use serde_json::json;

/// 取 JSON 对象的键集合（排序后）。
fn keys_of(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("应是 JSON 对象")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// `Role` 的协议取值 —— `internal/llm/types.go:17-24`。
#[test]
fn role_values_match_the_protocol() {
    assert_eq!(Role::System.as_str(), "system");
    assert_eq!(Role::User.as_str(), "user");
    assert_eq!(Role::Assistant.as_str(), "assistant");
    assert_eq!(Role::Tool.as_str(), "tool");
}

/// `Role` 序列化后是**小写字符串** —— 不是变体名。
///
/// 这条很重要：`#[serde(rename_all = "lowercase")]` 漏了的话，
/// 上游会收到 `"System"` 而不是 `"system"`。
#[test]
fn role_serializes_to_lowercase_strings() {
    assert_eq!(serde_json::to_value(Role::System).unwrap(), json!("system"));
    assert_eq!(serde_json::to_value(Role::Tool).unwrap(), json!("tool"));
}

/// `Message` 的 JSON tag —— `internal/llm/types.go:14-25`。
#[test]
fn message_field_names_match_the_protocol() {
    let msg = Message::new(Role::User, "你好");
    let value = serde_json::to_value(&msg).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["content", "role"],
        "必填字段应只有 role 与 content"
    );
    assert_eq!(value["role"], json!("user"));
    assert_eq!(value["content"], json!("你好"));
}

/// `Message` 的可选字段**带正确的 tag** —— `internal/llm/types.go:17-22`。
#[test]
fn message_optional_field_tags_match_the_protocol() {
    let mut msg = Message::new(Role::Assistant, "好的");
    msg.reasoning_content = "思考中".into();
    msg.tool_calls = vec![ToolCall {
        id: "call_1".into(),
        name: "search".into(),
        args: r#"{"q":"x"}"#.into(),
    }];
    msg.tool_call_id = "call_1".into();
    msg.name = "assistant-1".into();

    let value = serde_json::to_value(&msg).unwrap();

    assert_eq!(
        keys_of(&value),
        vec![
            "content",
            "name",
            "reasoning_content",
            "role",
            "tool_call_id",
            "tool_calls",
        ]
    );
    assert_eq!(value["reasoning_content"], json!("思考中"));
    assert_eq!(value["tool_call_id"], json!("call_1"));
    assert_eq!(value["tool_calls"][0]["id"], json!("call_1"));
    assert_eq!(value["tool_calls"][0]["name"], json!("search"));
}

/// `ToolCall.args` 是 **JSON 字符串**，不是嵌套对象 —— `internal/llm/types.go:29`。
///
/// 注释原文：「参数为 JSON 字符串（协议如此）」。
/// 这条钉死了就不会有人「顺手」把它改成 `Value`。
#[test]
fn tool_call_args_is_a_json_string_not_an_object() {
    let call = ToolCall {
        id: "call_1".into(),
        name: "search".into(),
        args: r#"{"q":"rust"}"#.into(),
    };

    let value = serde_json::to_value(&call).unwrap();

    assert!(value["args"].is_string(), "args 必须是字符串");
    assert_eq!(value["args"], json!(r#"{"q":"rust"}"#));

    // 但边界上能解析回 `Value`。
    let parsed = call.parsed_args().unwrap();
    assert_eq!(parsed["q"], json!("rust"));
}

/// `ToolCall.args` 不是合法 JSON 时**报错**，不静默当成空参数。
#[test]
fn tool_call_with_invalid_args_returns_an_error() {
    let call = ToolCall {
        id: "call_1".into(),
        name: "search".into(),
        args: "{not json".into(),
    };

    assert!(call.parsed_args().is_err(), "非法 JSON 必须报错");
}

/// `ToolDef` 的**权威执行元数据不参与序列化** —— `internal/llm/types.go:36-37`。
///
/// 参考实现注释：「authoritative execution metadata, never sent to the model」。
///
/// 这是**安全要求** —— 把「这个工具危险」告诉模型，模型可能据此改变行为。
#[test]
fn tool_def_control_plane_fields_are_not_serialized() {
    let def = ToolDef {
        source_identity: "mcp:server-1".into(),
        binding_version: 42,
        capability: "web.search".into(),
        dangerous: true,
        replay_policy: "readonly".into(),
        name: "search".into(),
        description: "搜索网页".into(),
        parameters: json!({"type": "object"}),
    };

    let value = serde_json::to_value(&def).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["description", "name", "parameters"],
        "只有 name / description / parameters 该发给模型"
    );

    // 反序列化时控制面字段取默认值 —— 它们从来不在 wire 上。
    let back: ToolDef = serde_json::from_value(value).unwrap();
    assert!(!back.dangerous, "dangerous 不该从 wire 上恢复");
    assert_eq!(back.source_identity, "");
    assert_eq!(back.binding_version, 0);
    assert_eq!(back.replay_policy, "");
}

/// `ChatRequest` 的字段 —— `internal/llm/types.go:44-51`。
#[test]
fn chat_request_field_names_match_the_protocol() {
    let req = ChatRequest {
        messages: vec![Message::new(Role::User, "你好")],
        tools: vec![],
        temperature: 0.7,
        top_p: 0.9,
        max_tokens: 1024,
        stream: false,
    };

    let value = serde_json::to_value(&req).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["max_tokens", "messages", "stream", "temperature", "top_p"],
        "空的 tools 列表应被跳过 —— 有工具时才传"
    );
    // 温度是 `f32` —— 浮点不能精确比较，只能近似。
    // 这与参考实现 `internal/llm/types.go:48` 的 `float32` 一致。
    let temp = value["temperature"].as_f64().unwrap();
    assert!(
        (temp - 0.7).abs() < 1e-6,
        "temperature 应约为 0.7，实际 {temp}"
    );
    assert_eq!(value["max_tokens"], json!(1024));
}

/// `Usage` 的字段 —— `internal/llm/types.go:53-56`。
#[test]
fn usage_field_names_match_the_protocol() {
    let usage = Usage {
        prompt_tokens: 10,
        completion_tokens: 20,
        cost: Some("0.0012".into()),
    };

    let value = serde_json::to_value(&usage).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["completion_tokens", "cost", "prompt_tokens"]
    );
    assert_eq!(usage.total(), 30);
}

/// `Usage.cost` 是**十进制字符串**，不是 `f64` —— 架构文档 §2.2.4.1。
///
/// 这是**前后端契约**（`usage` 事件的负载形状）。
/// 浮点在跨系统传递中会丢精度，而成本是要**累加并对账**的数字。
#[test]
fn usage_cost_is_a_decimal_string_not_a_float() {
    let usage = Usage {
        prompt_tokens: 1,
        completion_tokens: 1,
        cost: Some("0.0000005".into()),
    };

    let value = serde_json::to_value(&usage).unwrap();

    assert!(
        value["cost"].is_string(),
        "cost 必须是字符串，不能是数字：{value}"
    );
    assert_eq!(value["cost"], json!("0.0000005"));
}

/// `Usage.cost` 为 `None` 时**整个字段被跳过** —— 形状与 `nexus-core` 的
/// `usage` 事件负载一致（那里 `cost` 是 `Option<String>`）。
#[test]
fn usage_cost_none_is_omitted_from_the_payload() {
    let usage = Usage {
        prompt_tokens: 1,
        completion_tokens: 1,
        cost: None,
    };

    let value = serde_json::to_value(&usage).unwrap();

    assert!(
        !value.as_object().unwrap().contains_key("cost"),
        "cost 为 None 时应被跳过：{value}"
    );
}

/// `Usage::is_empty` —— 「上游没返回 usage」时不该计入指标。
#[test]
fn usage_is_empty_when_no_tokens_were_reported() {
    assert!(Usage::default().is_empty());
    assert!(
        !Usage {
            prompt_tokens: 1,
            completion_tokens: 0,
            cost: None,
        }
        .is_empty()
    );
}

/// `ChatResponse` 的字段 —— `internal/llm/types.go:60-68`。
#[test]
fn chat_response_field_names_match_the_protocol() {
    let resp = ChatResponse {
        content: "答案".into(),
        reasoning_content: String::new(),
        tool_calls: vec![],
        finish_reason: "stop".into(),
        model: "deepseek-chat".into(),
        usage: Usage::default(),
    };

    let value = serde_json::to_value(&resp).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["content", "finish_reason", "model", "usage"],
        "空的 reasoning_content / tool_calls 应被跳过"
    );
    assert_eq!(value["finish_reason"], json!("stop"));
}

/// `StreamEvent` 的**错误不参与序列化** —— `internal/llm/types.go:72`。
///
/// 参考实现注释：「never serialized to clients」。
/// 内部错误原因绝不出现在对外响应（§2.6.5）。
#[test]
fn stream_event_error_is_never_serialized() {
    let ev = StreamEvent::error("upstream failed: token=secret123");

    let value = serde_json::to_value(&ev).unwrap();

    assert!(
        !value.as_object().unwrap().contains_key("err"),
        "err 绝不能出现在序列化结果里：{value}"
    );
    let text = value.to_string();
    assert!(
        !text.contains("secret123"),
        "错误原因绝不能泄露到序列化结果：{text}"
    );
}

/// `StreamEvent` 的协议字段 —— `internal/llm/types.go:70-78`。
#[test]
fn stream_event_field_names_match_the_protocol() {
    let mut ev = StreamEvent::content("增量");
    ev.finish_reason = "stop".into();
    ev.usage = Some(Usage {
        prompt_tokens: 5,
        completion_tokens: 7,
        cost: None,
    });
    ev.model = "deepseek-chat".into();

    let value = serde_json::to_value(&ev).unwrap();

    assert_eq!(
        keys_of(&value),
        vec!["content", "finish_reason", "model", "usage"]
    );
    assert_eq!(value["usage"]["prompt_tokens"], json!(5));
}

/// `StreamEvent` 的错误状态可判别。
#[test]
fn stream_event_error_state_is_detectable() {
    assert!(StreamEvent::error("boom").is_error());
    assert!(!StreamEvent::content("hi").is_error());
}

/// `ModelDescriptor` 的 `Debug` **不含**明文口令。
#[test]
fn model_descriptor_debug_never_leaks_the_api_key() {
    let desc = ModelDescriptor::new(
        "deepseek",
        "https://api.deepseek.com/v1",
        "sk-super-secret-value",
        "deepseek-chat",
    );

    let debug = format!("{desc:?}");

    assert!(!debug.contains("super-secret"), "Debug 泄露了口令：{debug}");
    assert!(debug.contains("[REDACTED]"), "应显式标出已脱敏：{debug}");
    // 非敏感字段应保留，便于排查。
    assert!(debug.contains("deepseek"), "provider_code 应保留：{debug}");
    assert!(debug.contains("https://api.deepseek.com/v1"));
}

/// `ModelDescriptor` 的显式取用接口能拿到明文 —— 否则它就完全不可用了。
#[test]
fn model_descriptor_exposes_the_api_key_on_purpose() {
    let desc = ModelDescriptor::new("ollama", "http://localhost:11434", "k", "llama3");

    assert_eq!(desc.expose_api_key(), "k");
}

/// `ModelType` 的协议取值 —— `internal/model/llm.go:22-26`。
#[test]
fn model_type_values_match_the_protocol() {
    assert_eq!(ModelType::Chat.as_str(), "chat");
    assert_eq!(ModelType::Embedding.as_str(), "embedding");
    assert_eq!(ModelType::Rerank.as_str(), "rerank");
}

/// `ModelType` 默认是 `Chat` —— 绝大多数模型是聊天模型。
#[test]
fn model_type_defaults_to_chat() {
    assert_eq!(ModelType::default(), ModelType::Chat);
    assert_eq!(
        serde_json::to_value(ModelType::Chat).unwrap(),
        json!("chat"),
        "序列化应是协议字符串，不是变体名"
    );
}

/// `ModelDescriptor` 的能力查询 —— `internal/model/llm.go:56-61` 的 `HasCapability`。
#[test]
fn model_descriptor_capability_check_works() {
    let mut desc = ModelDescriptor::new("deepseek", "https://api.deepseek.com/v1", "k", "chat");
    assert!(!desc.has_capability("function_call"));

    desc.capabilities = vec!["function_call".into(), "reasoning".into()];
    assert!(desc.has_capability("function_call"));
    assert!(desc.has_capability("reasoning"));
    assert!(!desc.has_capability("vision"));
}

/// `ModelBackend` 是 **enum**，不是 `Box<dyn>` —— 架构文档 §2.3.2。
///
/// 供应商**不是**异构集合 —— §2.3.3 说 `dyn` 只在异构集合时才必需。
#[test]
fn model_backend_is_an_enum_not_a_trait_object() {
    // enum 的变体可以被穷尽匹配 —— 这是 `dyn` 做不到的。
    let backend = ModelBackend::for_provider("deepseek");
    match backend {
        ModelBackend::OpenAiCompatible => {}
    }
    assert_eq!(backend, ModelBackend::OpenAiCompatible);
}

/// 所有 OpenAI 兼容协议家族映射到同一个后端。
///
/// 参考实现 `internal/llm/provider.go:33-43` 就是这个形状 ——
/// deepseek / openai / qwen / zhipu / moonshot / doubao / ollama 全部共用。
#[test]
fn all_openai_compatible_providers_share_one_backend() {
    for code in [
        "deepseek", "openai", "qwen", "zhipu", "moonshot", "doubao", "ollama",
    ] {
        assert_eq!(
            ModelBackend::for_provider(code),
            ModelBackend::OpenAiCompatible,
            "{code} 应该是 OpenAI 兼容后端"
        );
    }
}
