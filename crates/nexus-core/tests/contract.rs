//! **跨模块契约测试** —— P1 收口。
//!
//! 把三份契约放在一起，便于**一眼看出契约是否齐备**：
//!
//! | 契约 | 来源 | 断言方式 |
//! |---|---|---|
//! | ID 类型清单（18 种） | 架构文档 §2.8.1 | `TypeId` 互不相同 + 数量 |
//! | 事件名清单（12 个） | 架构文档 §2.2.4.1 | 序列化后的 `type` 字段 |
//! | 业务码表（42 个） | 架构文档 §2.6.4.1 | 码值 + HTTP + 失败分类 |
//!
//! # 为什么要有这个文件
//!
//! 三份契约原本分散在三个模块的 `#[cfg(test)]` 里。分散的问题：
//!
//! 1. **看不到全貌** —— 想确认「P1 的三份契约齐了吗」，得翻三个文件。
//! 2. **看不出缺口** —— 若某个模块少了一个变体，本模块的测试照样绿。
//!
//! 本文件把三份契约**并列**放在一处，并额外断言它们之间的**交叉一致性** ——
//! 比如「事件里的 `error` 变体的 `code` 字段，必须是 `ErrorCode` 能表示的码」。
//!
//! # 契约不可改
//!
//! ID 底层类型、事件名、错误码值是**前后端契约**。实现与契约不符时，
//! 是**实现**错了，不是契约错了（架构文档 §6.3）。
//!
//! # 命名约定
//!
//! 测试函数名用英文（`.cursor/rules/rust-code-standards.mdc` §11：
//! 标识符不得包含中文）—— 中文只出现在注释与文档字符串里。

use std::any::TypeId;

use nexus_core::error::{ALL_CODES, AppError, ErrorCode, ModelError, RetryPolicy, UserFacing};
use nexus_core::event::{AgentEventKind, EVENT_NAMES, EventPayload, FinishReason, ToolStatus};
use nexus_core::id::{
    AgentId, CaseId, DatasetId, DocumentId, EmbeddingConfigId, ID_TYPE_NAMES, InterruptId, KbId,
    MessageId, ModelConfigId, ProviderId, RunId, SessionId, ToolCallId, ToolId, TraceId, UserId,
    VectorId, WorkflowId,
};

/// 12 个事件的样例负载。
fn event_samples() -> Vec<EventPayload> {
    vec![
        EventPayload::Meta {
            session_id: SessionId::new_v4(),
            agent_id: AgentId::new(7),
        },
        EventPayload::Delta {
            content: "x".into(),
        },
        EventPayload::Reasoning {
            content: "y".into(),
        },
        EventPayload::Plan {
            steps: vec!["a".into()],
            current: 0,
        },
        EventPayload::ToolCall {
            call_id: ToolCallId::new("c1"),
            name: "search".into(),
            arguments: serde_json::Value::Null,
        },
        EventPayload::ToolResult {
            call_id: ToolCallId::new("c1"),
            result: "ok".into(),
            elapsed_ms: 12,
            status: ToolStatus::Ok,
        },
        EventPayload::AgentEvent {
            kind: AgentEventKind::Delegate,
            expert: "researcher".into(),
            detail: "d".into(),
        },
        EventPayload::Refs {
            items: vec![serde_json::Value::Null],
        },
        EventPayload::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            cost: Some("0.0012".into()),
            model: "gpt-4".into(),
        },
        EventPayload::Approval {
            approval_id: "ap-1".into(),
            kind: "tool_write".into(),
            payload: serde_json::Value::Null,
        },
        EventPayload::Done {
            finish_reason: FinishReason::ToolCalls,
        },
        EventPayload::Error {
            code: 90001,
            message: "boom".into(),
        },
    ]
}

/// **契约 1** —— ID 类型清单（§2.8.1，18 种）。
///
/// 断言「它们是 18 个**互不相同**的类型」—— 这是 §2.8 设计意图的核心：
/// 别名（`type UserId = i64`）传错不报错，独立 newtype 才传错就报错。
#[test]
fn contract_1_eighteen_id_types_are_all_distinct() {
    let ids: Vec<TypeId> = vec![
        TypeId::of::<UserId>(),
        TypeId::of::<SessionId>(),
        TypeId::of::<AgentId>(),
        TypeId::of::<MessageId>(),
        TypeId::of::<RunId>(),
        TypeId::of::<WorkflowId>(),
        TypeId::of::<ToolId>(),
        TypeId::of::<ToolCallId>(),
        TypeId::of::<ModelConfigId>(),
        TypeId::of::<ProviderId>(),
        TypeId::of::<EmbeddingConfigId>(),
        TypeId::of::<KbId>(),
        TypeId::of::<DocumentId>(),
        TypeId::of::<DatasetId>(),
        TypeId::of::<CaseId>(),
        TypeId::of::<VectorId>(),
        TypeId::of::<TraceId>(),
        TypeId::of::<InterruptId>(),
    ];

    assert_eq!(ids.len(), 18, "ID 类型数量与 §2.8.1 不符");
    assert_eq!(
        ID_TYPE_NAMES.len(),
        18,
        "ID 契约清单长度应为 18（清单与实现必须同步）"
    );

    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        18,
        "存在重复的 ID 类型 —— 某些是别名而非独立类型，违背 §2.8"
    );
}

/// **契约 2** —— 事件名清单（§2.2.4.1，12 个）。
#[test]
fn contract_2_event_name_list_matches_spec() {
    let actual: Vec<String> = event_samples()
        .iter()
        .map(|s| {
            serde_json::to_value(s).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();

    assert_eq!(actual, EVENT_NAMES, "事件名清单与 §2.2.4.1 不符");
    assert_eq!(EVENT_NAMES.len(), 12, "事件类型数量应为 12");
}

/// **契约 3** —— 业务码表（§2.6.4.1，42 个）。
///
/// 这里只断言**数量**与**码段分区**。逐条的码值 + HTTP + 分类对照表在
/// `error` 模块的单元测试里 —— 那里有完整的三元组，更适合逐条核对。
#[test]
fn contract_3_error_code_table_is_complete_and_partitioned() {
    assert_eq!(ALL_CODES.len(), 42, "业务码数量应为 42");

    // 每个码段都存在，且码值落在自己的段里
    for segment in 1..=9u32 {
        let in_segment: Vec<&ErrorCode> = ALL_CODES
            .iter()
            .filter(|c| c.segment() == segment)
            .collect();
        assert!(!in_segment.is_empty(), "码段 {segment} 为空");
        for c in in_segment {
            assert_eq!(c.code() / 10000, segment as i32, "{c:?} 码值与码段不一致");
        }
    }
}

/// **交叉一致性 1** —— 事件 `error` 变体的 `code` 必须是合法业务码。
///
/// `EventPayload::Error { code }` 用的是裸 `i32`，而 `ErrorCode` 是枚举。
/// 两者必须对得上 —— 否则前端拿到一个码段解析不了的数字。
#[test]
fn cross_consistency_event_error_code_is_a_valid_business_code() {
    let samples = [
        ErrorCode::InvalidParam,
        ErrorCode::ModelTimeout,
        ErrorCode::ToolFailed,
        ErrorCode::Internal,
    ];

    for code in samples {
        let payload = EventPayload::Error {
            code: code.code(),
            message: "boom".into(),
        };
        let v = serde_json::to_value(&payload).unwrap();

        assert_eq!(v["type"], "error");
        assert_eq!(v["code"], code.code());

        // 数字码必须能被业务码反查回来
        assert_eq!(
            ErrorCode::from_code(v["code"].as_i64().unwrap() as i32),
            Some(code),
            "{code:?} 的码值无法反查 —— 事件与错误码表脱节"
        );
    }
}

/// **交叉一致性 2** —— 事件 `tool_result` 的失败状态码必须是合法业务码。
///
/// `ToolStatus::Failed { code }` 携带业务码 —— 必须能反查。
#[test]
fn cross_consistency_tool_failure_status_carries_a_valid_business_code() {
    let code = ErrorCode::ToolFailed;
    let payload = EventPayload::ToolResult {
        call_id: ToolCallId::new("c1"),
        result: "boom".into(),
        elapsed_ms: 100,
        status: ToolStatus::Failed { code: code.code() },
    };

    let v = serde_json::to_value(&payload).unwrap();
    assert_eq!(v["type"], "tool_result");
    assert_eq!(v["status"]["failed"]["code"], code.code());
    assert_eq!(ErrorCode::from_code(code.code()), Some(code));
}

/// **交叉一致性 3** —— 事件里没有业务码的地方，不该出现业务码语义。
///
/// `Usage` 携带 token 数与成本，不带业务码；`Done` 携带结束原因，不带业务码。
/// 这条断言防止「顺手往 `usage` 里加个 `code` 字段」这类协议污染。
#[test]
fn cross_consistency_non_error_events_carry_no_business_code() {
    let usage = serde_json::to_value(EventPayload::Usage {
        prompt_tokens: 1,
        completion_tokens: 2,
        cost: None,
        model: "m".into(),
    })
    .unwrap();

    assert!(usage.get("code").is_none(), "usage 事件不应携带业务码");

    let done = serde_json::to_value(EventPayload::Done {
        finish_reason: FinishReason::Stop,
    })
    .unwrap();

    assert!(done.get("code").is_none(), "done 事件不应携带业务码");
}

/// **交叉一致性 4** —— `AppError` 与 `ErrorCode` 对同一码的分类一致。
///
/// 分类是**码的属性**（§2.6.3）—— `AppError::retry_policy()` 必须与
/// `ErrorCode::retry_policy()` 一致，不能各说各话。
#[test]
fn cross_consistency_app_error_and_code_agree_on_retry_policy() {
    for code in ALL_CODES {
        let err = AppError::new(*code, "x");
        assert_eq!(
            err.retry_policy(),
            code.retry_policy(),
            "{code:?} 的分类在 AppError 与 ErrorCode 之间不一致"
        );
    }
}

/// **交叉一致性 5** —— `UserFacing` 序列化出的码必须能被反查回来。
///
/// 客户端拿到 `{"code": 50002}` 要能定位到具体错误 —— 反查失败说明码表有缺口。
#[test]
fn cross_consistency_user_facing_code_roundtrips_back() {
    for code in ALL_CODES {
        let facing = UserFacing::new(*code, "x");
        let v = serde_json::to_value(&facing).unwrap();

        assert_eq!(v["code"], code.code());
        assert_eq!(
            ErrorCode::from_code(v["code"].as_i64().unwrap() as i32),
            Some(*code),
            "{code:?} 反查失败"
        );
    }
}

/// **交叉一致性 6** —— 领域错误的转换不改变语义。
///
/// §2.6.2 的转换模式要求：领域错误的**失败性质**在转换后不能被抹平。
/// 尤其超时 → 必须仍是 `UnknownOutcome`。
#[test]
fn cross_consistency_domain_error_conversion_preserves_policy() {
    let timeout = AppError::from(ModelError::Timeout);
    assert_eq!(timeout.retry_policy(), RetryPolicy::UnknownOutcome);

    let rate_limited = AppError::from(ModelError::RateLimited { retry_after: None });
    assert_eq!(rate_limited.retry_policy(), RetryPolicy::Retryable);

    // 上游明确报错 —— 明确失败，可重试
    let upstream = AppError::from(ModelError::Upstream { status: 500 });
    assert_eq!(upstream.retry_policy(), RetryPolicy::Retryable);

    // 响应格式非法 —— 终端错误
    let malformed = AppError::from(ModelError::Malformed("bad".into()));
    assert_eq!(malformed.retry_policy(), RetryPolicy::Terminal);
}
