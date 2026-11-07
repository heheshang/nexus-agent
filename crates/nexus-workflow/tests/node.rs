//! 节点执行契约的行为层测试。
//!
//! 覆盖 NEX-39 的验收条件：
//!
//! | AC | 测试 |
//! |---|---|
//! | #1 | `a_node_can_be_invoked_and_returns_its_output` |
//! | #2 | `the_node_error_has_three_distinct_classes` |
//! | #3 | `no_output_is_distinct_from_an_error` |
//! | #4 | `a_state_write_beyond_the_limit_is_rejected_and_reverted` |
//! | #5 | `the_concurrency_gate_limits_parallel_nodes` |
//! | #6 | `a_non_boolean_condition_result_is_an_error` |
//! | #7 | `a_failed_condition_evaluation_is_an_error` |
//! | #8 | `a_missing_condition_variable_is_an_error` |
//! | #9 | `a_whole_string_template_keeps_the_original_type` |
//! | #10 | `an_unresolved_template_is_rejected` |
//! | #11 | `a_nested_template_is_rendered_recursively` |
//!
//! # ⚠️ 契约里点名的测试名**不能改**
//!
//! `docs/specs/p6/contracts/invariants.json` 的 `verified_by` 指向
//! 本文件的**具体测试函数名** —— `scripts/check-specs.sh` 规则 3
//! 会 grep 这些名字是否存在。改名 = 契约失联。

use std::sync::Arc;

use async_trait::async_trait;
use nexus_workflow::condition::{ConditionError, eval_condition};
use nexus_workflow::node::{
    ConcurrencyGate, DEFAULT_MAX_CONCURRENCY, ErrorCode, InterruptSignal, NodeContext, NodeError,
    NodeFailure, NodeInput, NodeInvoke, NodeOutput, NodeRegistry, invoke_node,
};
use nexus_workflow::state::{GraphState, StateError};
use nexus_workflow::template::{TemplateError, render_args};
use serde_json::json;

/// 一个**总是成功**的节点。
struct Echo;

#[async_trait]
impl NodeInvoke for Echo {
    async fn invoke(
        &self,
        _ctx: &NodeContext<'_>,
        input: &NodeInput,
    ) -> Result<Option<NodeOutput>, NodeError> {
        let mut out = NodeOutput::new();
        if let Some(value) = input.get("text") {
            out = out.with("echo", value.clone());
        }
        Ok(Some(out))
    }
}

/// 一个**总是失败**的节点 —— 失败分类由构造参数决定。
struct Failing {
    error: NodeError,
}

#[async_trait]
impl NodeInvoke for Failing {
    async fn invoke(
        &self,
        _ctx: &NodeContext<'_>,
        _input: &NodeInput,
    ) -> Result<Option<NodeOutput>, NodeError> {
        Err(self.error.clone())
    }
}

/// 一个**无产出但正常**的节点。
struct Silent;

#[async_trait]
impl NodeInvoke for Silent {
    async fn invoke(
        &self,
        _ctx: &NodeContext<'_>,
        _input: &NodeInput,
    ) -> Result<Option<NodeOutput>, NodeError> {
        Ok(None)
    }
}

/// 构造一个带一个键的状态。
fn state_with(key: &str, value: serde_json::Value) -> GraphState {
    let mut state = GraphState::new();
    state.insert(key, value).expect("状态写入必须成功");
    state
}

/// 一个「**恰好差一点点**就超限」的字符串。
///
/// # 为什么是 `- 20`
///
/// 序列化 `{"big":"xxx…"}` 的字节数 = 内容长度 + 10
/// （`{` + `"big"` + `:` + `"` + 内容 + `"` + `}`）。
/// 内容取 `4 MiB - 20` → 序列化后 `4 MiB - 10` —— **还差 10 字节**。
/// 再写一个 `"small":"y"`（`,"small":"y"` = 11 字节）→ **超限**。
///
/// ⚠️ 不能用「拍脑袋」的余量 —— 余量太大就**测不到**超限路径，
/// 余量太小则在写 `big` 时就超限（那测的是另一条路径）。
const NEAR_LIMIT: usize = 4 * 1024 * 1024 - 20;

// ────────────────────────────────────────────────────────────────────
// AC #1 —— 节点执行契约
// ────────────────────────────────────────────────────────────────────

/// **AC #1** —— 节点可以被调用并返回产出。
#[tokio::test]
async fn a_node_can_be_invoked_and_returns_its_output() {
    let state = GraphState::new();
    let ctx = NodeContext::new("node/echo", &state);
    let input = NodeInput::new().with("text", json!("你好"));

    let output = invoke_node(&Echo, &ctx, &input)
        .await
        .expect("节点必须成功")
        .expect("节点必须有产出");

    assert_eq!(output.get("echo"), Some(&json!("你好")));
}

/// **AC #1** —— 节点上下文能看见路径与状态。
#[tokio::test]
async fn the_node_context_exposes_the_path_and_the_state() {
    let state = state_with("count", json!(3));
    let ctx = NodeContext::new("node/a/b", &state);

    assert_eq!(ctx.path(), "node/a/b");
    assert_eq!(ctx.state().get("count"), Some(&json!(3)));
}

// ────────────────────────────────────────────────────────────────────
// AC #2 —— 三分类错误
// ────────────────────────────────────────────────────────────────────

/// **AC #2** —— 节点错误是**三分类** enum。
#[tokio::test]
async fn the_node_error_has_three_distinct_classes() {
    let retryable =
        NodeError::Retryable(NodeFailure::new(ErrorCode::ModelUnavailable, "上游不可用"));
    let permanent = NodeError::Permanent(NodeFailure::new(ErrorCode::InvalidParam, "参数非法"));
    let interrupt = NodeError::Interrupt(InterruptSignal::new("node/human", "确认？"));

    // ── 三类**互不相同** ──
    assert_ne!(retryable, permanent);
    assert_ne!(permanent, interrupt);
    assert_ne!(retryable, interrupt);

    // ── 处置**不同** ──
    assert!(retryable.is_retryable());
    assert!(!permanent.is_retryable());
    assert!(!interrupt.is_retryable());

    assert!(interrupt.is_interrupt());
    assert!(!retryable.is_interrupt());

    // ── 载荷**保留** ──
    assert_eq!(
        retryable.failure().map(|f| f.code),
        Some(ErrorCode::ModelUnavailable)
    );
    assert_eq!(
        permanent.failure().map(|f| f.message.as_str()),
        Some("参数非法")
    );
    // 中断**没有**失败载荷 —— 它不是失败。
    assert!(interrupt.failure().is_none());
}

/// **AC #2** —— 三分类在**节点返回**的路径上真的可区分。
#[tokio::test]
async fn a_node_can_return_each_of_the_three_error_classes() {
    let state = GraphState::new();
    let ctx = NodeContext::new("node/fail", &state);
    let input = NodeInput::new();

    let retryable = Failing {
        error: NodeError::Retryable(NodeFailure::new(ErrorCode::ModelUnavailable, "上游不可用")),
    };
    let permanent = Failing {
        error: NodeError::Permanent(NodeFailure::new(ErrorCode::InvalidParam, "参数非法")),
    };
    let interrupt = Failing {
        error: NodeError::Interrupt(InterruptSignal::new("node/fail", "确认？")),
    };

    assert!(
        invoke_node(&retryable, &ctx, &input)
            .await
            .expect_err("必须失败")
            .is_retryable()
    );
    assert!(
        invoke_node(&permanent, &ctx, &input)
            .await
            .expect_err("必须失败")
            .failure()
            .is_some()
    );
    assert!(
        invoke_node(&interrupt, &ctx, &input)
            .await
            .expect_err("必须失败")
            .is_interrupt()
    );
}

// ────────────────────────────────────────────────────────────────────
// AC #3 —— Ok(None) 与 Err 区分
// ────────────────────────────────────────────────────────────────────

/// **AC #3** —— `Ok(None)` 与 `Err` 是**两件事**。
#[tokio::test]
async fn no_output_is_distinct_from_an_error() {
    let state = GraphState::new();
    let ctx = NodeContext::new("node/silent", &state);
    let input = NodeInput::new();

    let result = invoke_node(&Silent, &ctx, &input).await;

    // ── 无产出是**成功** ──
    assert!(result.is_ok());
    assert!(result.expect("必须成功").is_none());

    // ── 空产出表**也**是无产出，但两者**语义**不同 ──
    // 「无产出但正常」与「产出了但内容是空的」不是同一件事。
    let empty = NodeOutput::new();
    assert!(empty.is_empty());
    assert_ne!(NodeOutput::new(), NodeOutput::new().with("k", json!(1)));
}

/// **AC #3** —— 错误**不是** `Ok(None)`。
#[tokio::test]
async fn an_error_is_not_reported_as_no_output() {
    let state = GraphState::new();
    let ctx = NodeContext::new("node/fail", &state);
    let input = NodeInput::new();

    let failing = Failing {
        error: NodeError::Permanent(NodeFailure::new(ErrorCode::InvalidParam, "参数非法")),
    };

    let result = invoke_node(&failing, &ctx, &input).await;
    assert!(result.is_err(), "失败必须走 Err，而不是 Ok(None)");
}

// ────────────────────────────────────────────────────────────────────
// AC #4 —— 状态大小上限
// ────────────────────────────────────────────────────────────────────

/// **AC #4** —— 状态超过 4 MiB → **错误**，且**回滚**。
#[test]
fn a_state_write_beyond_the_limit_is_rejected_and_reverted() {
    let mut state = GraphState::new();

    // 先写一个占满大部分配额的值。
    let big = "x".repeat(NEAR_LIMIT);
    state
        .insert("big", json!(big))
        .expect("未超限的写入必须成功");

    let before = state.clone();

    // 再写一个小的 —— 总量超限。
    let err = state.insert("small", json!("y")).expect_err("超限必须报错");

    assert!(
        matches!(err, StateError::SizeExceeded { .. }),
        "必须是 SizeExceeded，实际是 {err:?}"
    );

    // ── 关键：**回滚** ──
    // 超限的写入**没有生效** —— 状态与写入前**完全相同**。
    assert_eq!(state, before, "超限的写入必须完全回滚");
    assert!(state.get("small").is_none(), "超限的键不能留在状态里");
}

/// **AC #4** —— 超限**不截断**：状态内容一个字节都没少。
#[test]
fn an_oversized_state_is_not_truncated() {
    let mut state = GraphState::new();
    let big = "x".repeat(NEAR_LIMIT);
    state
        .insert("big", json!(big))
        .expect("未超限的写入必须成功");

    let before_len = state.len();
    let _ = state.insert("small", json!("y"));

    assert_eq!(state.len(), before_len, "键的数量不能变");
    assert_eq!(
        state.get("big").and_then(|v| v.as_str()).map(str::len),
        Some(NEAR_LIMIT),
        "已有内容不能被截断"
    );
}

/// **AC #4** —— 节点产出的合并也是**全有或全无**。
#[test]
fn an_oversized_merge_is_rejected_as_a_whole() {
    let mut state = GraphState::new();
    let big = "x".repeat(NEAR_LIMIT);
    state
        .insert("big", json!(big))
        .expect("未超限的写入必须成功");

    let before = state.clone();

    let output = NodeOutput::new()
        .with("a", json!(1))
        .with("b", json!(2))
        .with("c", json!(3));

    let err = state.apply(&output).expect_err("超限必须报错");
    assert!(matches!(err, StateError::SizeExceeded { .. }));

    assert_eq!(state, before, "超限的合并必须完全回滚");
    assert!(state.get("a").is_none(), "合并必须全有或全无");
}

/// **AC #4** —— 状态大小检查用的是**序列化后**的字节数。
#[test]
fn the_state_size_is_measured_on_the_serialized_form() {
    let state = state_with("k", json!("值"));
    let expected = serde_json::to_vec(state.as_map())
        .expect("必须可序列化")
        .len();
    assert_eq!(state.size_bytes().expect("必须可测量"), expected);
}

// ────────────────────────────────────────────────────────────────────
// AC #5 —— 并发闸门
// ────────────────────────────────────────────────────────────────────

/// **AC #5** —— 默认 **4** 个槽。
#[test]
fn the_concurrency_gate_defaults_to_four_slots() {
    let gate = ConcurrencyGate::new();
    assert_eq!(gate.max(), 4);
    assert_eq!(gate.max(), DEFAULT_MAX_CONCURRENCY);
    assert_eq!(gate.available(), 4);
}

/// **AC #5** —— 闸门**真的**限流：第 5 个许可拿不到。
#[tokio::test]
async fn the_concurrency_gate_limits_parallel_nodes() {
    let gate = ConcurrencyGate::new();

    let mut permits = Vec::new();
    for _ in 0..4 {
        permits.push(gate.acquire().await.expect("许可必须拿得到"));
    }

    assert_eq!(gate.available(), 0, "4 个槽都被占了");

    // 第 5 个**拿不到** —— 但它**等**得到。
    let waiter = {
        let gate = gate.clone();
        tokio::spawn(async move { gate.acquire().await.is_ok() })
    };

    // 让 waiter 跑一会儿，确认它**被挡住**了。
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!waiter.is_finished(), "第 5 个节点必须被挡住");

    // 放掉一个 —— waiter 立刻拿到。
    drop(permits.pop());
    assert!(
        waiter.await.expect("任务不能 panic"),
        "释放一个槽后 waiter 必须拿到许可"
    );
}

/// **AC #5** —— 闸门**克隆**后**不**增加槽数。
#[tokio::test]
async fn a_cloned_gate_shares_the_same_slots() {
    let gate = ConcurrencyGate::new();
    let clone = gate.clone();

    // 从闸门拿一个许可 —— **克隆**的闸门看到的可用槽也要减少。
    // 如果两者各有各的信号量，这里会看到 4 而不是 3。
    let permit = gate.acquire().await.expect("许可必须拿得到");
    assert_eq!(clone.available(), 3, "克隆的闸门共享同一批槽");

    // 从克隆再拿一个 —— 原闸门看到的可用槽**也**要减少。
    let permit2 = clone.acquire().await.expect("许可必须拿得到");
    assert_eq!(gate.available(), 2, "克隆的闸门共享同一批槽");

    drop(permit);
    drop(permit2);
    assert_eq!(gate.available(), 4);
}

/// **AC #5** —— 0 个槽会被**拒绝**（否则是永久挂起）。
#[test]
fn a_gate_with_zero_slots_is_rejected() {
    assert!(ConcurrencyGate::try_with_limit(0).is_none());
    // `with_limit` 把 0 抬到 1 —— 而不是留一个「永远拿不到许可」的闸门。
    assert_eq!(ConcurrencyGate::with_limit(0).max(), 1);
}

// ────────────────────────────────────────────────────────────────────
// AC #6 / #7 / #8 —— 条件求值
// ────────────────────────────────────────────────────────────────────

/// **AC #6** —— 结果**不是布尔** → **错误**。
#[test]
fn a_non_boolean_condition_result_is_an_error() {
    let state = state_with("count", json!(5));

    let err = eval_condition("count", &state).expect_err("非布尔必须报错");

    match err {
        ConditionError::NonBoolean { expression, actual } => {
            assert_eq!(expression, "count");
            assert!(actual.contains("整数"), "实际值形态要能指出类型：{actual}");
        }
        other => panic!("必须是 NonBoolean，实际是 {other:?}"),
    }
}

/// **AC #7** —— 求值**失败** → **错误**，**不是** `false`。
#[test]
fn a_failed_condition_evaluation_is_an_error() {
    let state = state_with("count", json!(5));

    // 语法错误 —— 括号没闭合。
    let err = eval_condition("count > ", &state).expect_err("语法错误必须报错");

    match err {
        ConditionError::EvaluationFailed { expression, .. } => {
            assert_eq!(expression, "count > ");
        }
        other => panic!("必须是 EvaluationFailed，实际是 {other:?}"),
    }

    // ⚠️ 关键：**不是** `Ok(false)`。
    // 「表达式写错」静默成 `false` = 图走错分支且不报错。
    assert!(eval_condition("count > ", &state).is_err());
}

/// **AC #8** —— 变量**不存在** → **错误**，**不是** `false`。
#[test]
fn a_missing_condition_variable_is_an_error() {
    let state = state_with("count", json!(5));

    let err = eval_condition("missing > 3", &state).expect_err("变量不存在必须报错");

    match err {
        ConditionError::UnknownVariable { variable } => {
            assert_eq!(variable, "missing");
        }
        other => panic!("必须是 UnknownVariable，实际是 {other:?}"),
    }

    assert!(eval_condition("missing > 3", &state).is_err());
}

/// 条件求值**成功**时给出正确的布尔值。
#[test]
fn a_boolean_condition_evaluates_to_the_right_branch() {
    let mut state = GraphState::new();
    state.insert("count", json!(5)).unwrap();
    state.insert("mode", json!("fast")).unwrap();
    state.insert("enabled", json!(true)).unwrap();

    assert!(eval_condition("count > 3", &state).unwrap());
    assert!(!eval_condition("count > 9", &state).unwrap());
    assert!(eval_condition("enabled", &state).unwrap());
    assert!(eval_condition("mode == \"fast\"", &state).unwrap());
    assert!(eval_condition("count > 3 && enabled", &state).unwrap());
}

/// 引用**对象**值 → **明确**的「不支持」错误（而不是 `UnknownVariable`）。
#[test]
fn an_object_valued_variable_is_reported_as_unsupported() {
    let state = state_with("config", json!({"a": 1}));

    let err = eval_condition("config != 0", &state).expect_err("对象值不能参与求值");

    match err {
        ConditionError::UnsupportedValue { variable, kind } => {
            assert_eq!(variable, "config");
            assert_eq!(kind, "Object");
        }
        other => panic!("必须是 UnsupportedValue，实际是 {other:?}"),
    }
}

/// 状态里的对象**不**让**其它**变量的求值失败。
#[test]
fn an_object_in_the_state_does_not_break_other_conditions() {
    let mut state = GraphState::new();
    state.insert("config", json!({"a": 1})).unwrap();
    state.insert("count", json!(5)).unwrap();

    assert!(eval_condition("count > 3", &state).unwrap());
}

// ────────────────────────────────────────────────────────────────────
// AC #9 / #10 / #11 —— 模板渲染
// ────────────────────────────────────────────────────────────────────

/// **AC #9** —— 整串占位符**保留原始类型**。
#[test]
fn a_whole_string_template_keeps_the_original_type() {
    let state = state_with("count", json!(5));

    // 数字仍是**数字**，不是字符串 "5"。
    let rendered = render_args(&json!("{{count}}"), &state).expect("渲染必须成功");
    assert_eq!(rendered, json!(5));
    assert!(rendered.is_number(), "整串占位符必须保留数字类型");

    // 布尔同理。
    let state = state_with("flag", json!(true));
    let rendered = render_args(&json!("{{flag}}"), &state).expect("渲染必须成功");
    assert_eq!(rendered, json!(true));
    assert!(rendered.is_boolean(), "整串占位符必须保留布尔类型");

    // 数组 / 对象同理。
    let state = state_with("items", json!([1, 2, 3]));
    let rendered = render_args(&json!("{{items}}"), &state).expect("渲染必须成功");
    assert_eq!(rendered, json!([1, 2, 3]));
    assert!(rendered.is_array(), "整串占位符必须保留数组类型");
}

/// **AC #9** —— 文本插值走**字符串**路径。
#[test]
fn a_mixed_template_is_interpolated_as_text() {
    let state = state_with("count", json!(5));

    let rendered = render_args(&json!("共 {{count}} 个"), &state).expect("渲染必须成功");
    assert_eq!(rendered, json!("共 5 个"));
    assert!(rendered.is_string());
}

/// **AC #10** —— 插值结果**仍含** `{{` → **错误**。
#[test]
fn an_unresolved_template_is_rejected() {
    // ── 情形 1：状态里的值**自己**含 `{{` ──
    let state = state_with("raw", json!("{{still_raw}}"));
    let err = render_args(&json!("前缀 {{raw}}"), &state).expect_err("残留占位符必须报错");
    assert!(
        matches!(err, TemplateError::Unresolved { .. }),
        "必须是 Unresolved，实际是 {err:?}"
    );

    // ── 情形 2：占位符没有闭合 ──
    let state = state_with("count", json!(5));
    let err = render_args(&json!("{{count"), &state).expect_err("未闭合必须报错");
    assert!(
        matches!(err, TemplateError::Malformed { .. }),
        "必须是 Malformed，实际是 {err:?}"
    );

    // ⚠️ 关键：不是「原样放行」。
    assert!(render_args(&json!("前缀 {{raw}}"), &state).is_err());
}

/// **AC #11** —— 嵌套 map / slice **递归**渲染。
#[test]
fn a_nested_template_is_rendered_recursively() {
    let state = state_with("count", json!(5));

    let template = json!({
        "top": "{{count}}",
        "nested": {
            "deep": "值是 {{count}}",
            "list": ["{{count}}", "x{{count}}"],
        },
    });

    let rendered = render_args(&template, &state).expect("渲染必须成功");

    assert_eq!(rendered["top"], json!(5));
    assert_eq!(rendered["nested"]["deep"], json!("值是 5"));
    assert_eq!(rendered["nested"]["list"][0], json!(5));
    assert_eq!(rendered["nested"]["list"][1], json!("x5"));
}

/// 模板引用**不存在**的变量 → **错误**。
#[test]
fn a_template_referencing_a_missing_variable_is_rejected() {
    let state = GraphState::new();

    let err = render_args(&json!("{{missing}}"), &state).expect_err("变量不存在必须报错");
    assert!(
        matches!(err, TemplateError::UnknownVariable { .. }),
        "必须是 UnknownVariable，实际是 {err:?}"
    );
}

// ────────────────────────────────────────────────────────────────────
// 节点注册表 —— `Arc<dyn NodeInvoke>` 的唯一合法位置
// ────────────────────────────────────────────────────────────────────

/// 节点注册表能装载**异构**实现并按键取出。
#[tokio::test]
async fn the_node_registry_holds_heterogeneous_implementations() {
    let mut registry = NodeRegistry::new();
    registry.register("echo", Arc::new(Echo));
    registry.register("silent", Arc::new(Silent));

    assert_eq!(registry.len(), 2);
    assert!(registry.get("echo").is_some());
    assert!(registry.get("absent").is_none());

    let state = GraphState::new();
    let ctx = NodeContext::new("node/echo", &state);
    let input = NodeInput::new().with("text", json!("hi"));

    let node = registry.get("echo").expect("节点必须存在");
    let output = node.invoke(&ctx, &input).await.expect("必须成功");
    assert_eq!(
        output.and_then(|o| o.get("echo").cloned()),
        Some(json!("hi"))
    );
}
