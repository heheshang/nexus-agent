//! 条件求值 —— **布尔**分支条件。
//!
//! # 本模块负责什么
//!
//! [`eval_condition`] —— 用 `evalexpr` 求值一个布尔表达式，
//! 变量从 [`GraphState`] 取。
//!
//! # ⚠️ 三条**错误优先**的规则（AC #6 / #7 / #8）
//!
//! | 规则 | 理由 |
//! |---|---|
//! | 结果**必须**是布尔，非布尔 → **错误** | 条件语义是「yes / no 哪个目标」 |
//! | 求值**失败** → **错误** | 静默当 `false` = 走错分支且**不报错** |
//! | 变量**不存在** → **错误** | 同上 |
//!
//! # 为什么「失败 → 错误」是**硬要求**（不是保守）
//!
//! 条件分支的错误**不报错**是最难排查的一类故障：
//! 图**安静地**走了 `no` 分支，产出看起来「正常」，
//! 但结果**从一开始就是错的** —— 而且**没有任何证据**指向原因。
//!
//! 所以本模块**不提供**「失败当 `false`」的入口。
//! 调用方想容错，必须**显式**写出来。
//!
//! # 变量映射
//!
//! 图状态的 JSON 值 → `evalexpr` 的值：
//!
//! | JSON | evalexpr | 备注 |
//! |---|---|---|
//! | `Bool` | `Boolean` | |
//! | 整数 | `Int`（`i64`） | |
//! | 浮点 | `Float`（`f64`） | |
//! | 字符串 | `String` | |
//! | 数组 | `Tuple` | 元素**递归**映射 |
//! | `null` | `Empty` | |
//! | 对象 | **不支持** | 引用它 → [`ConditionError::UnsupportedValue`] |
//!
//! # ⚠️ 为什么对象**不支持**
//!
//! `evalexpr` 没有「字典」类型 —— 它只有标量与元组。
//! 把 JSON 对象硬塞进 `Tuple` 会让 `a.b` 与 `a[0]` **语义混淆**。
//!
//! 与其给出一个**含义不明**的映射，不如**明确报错**。
//!
//! # ⚠️ 「不支持的对象」**不**让整个求值失败
//!
//! 状态里有对象 ≠ 表达式引用了它。
//!
//! 所以装上下文时**跳过**不支持的值；只有当表达式**真的引用**了它，
//! 才报 [`ConditionError::UnsupportedValue`] ——
//! 报的是「这个变量用不了」，而不是「整个状态用不了」。

use evalexpr::{
    ContextWithMutableVariables, DefaultNumericTypes, EvalexprError, HashMapContext, Value,
};

use crate::state::GraphState;

/// **条件求值错误**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionError {
    /// **表达式语法错误**（或求值过程出错）。
    ///
    /// 携带原始消息 —— 表达式写错时，报错要能指出**哪里**错。
    EvaluationFailed {
        /// 表达式原文。
        expression: String,
        /// 原因。
        reason: String,
    },

    /// 结果**不是布尔**（AC #6）。
    NonBoolean {
        /// 表达式原文。
        expression: String,
        /// 实际结果的可读形态。
        actual: String,
    },

    /// 变量**不存在**（AC #8）。
    UnknownVariable {
        /// 变量名。
        variable: String,
    },

    /// 变量**存在**，但它的值**不能**参与条件求值。
    UnsupportedValue {
        /// 变量名。
        variable: String,
        /// 值类型名。
        kind: String,
    },
}

impl std::fmt::Display for ConditionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EvaluationFailed { expression, reason } => {
                write!(f, "条件表达式求值失败（{expression}）：{reason}")
            }
            Self::NonBoolean { expression, actual } => {
                write!(f, "条件表达式结果不是布尔（{expression}）：实际是 {actual}")
            }
            Self::UnknownVariable { variable } => {
                write!(f, "条件表达式引用了不存在的变量：{variable}")
            }
            Self::UnsupportedValue { variable, kind } => {
                write!(f, "状态变量 {variable} 的值类型 {kind} 不支持条件求值")
            }
        }
    }
}

impl std::error::Error for ConditionError {}

impl From<ConditionError> for nexus_core::error::AppError {
    /// 把条件错误映射成**应用层**错误。
    ///
    /// # 映射规则
    ///
    /// 全部 → `WorkflowDsl`（30005）。
    ///
    /// # ⚠️ 为什么**全部**映射到 `WorkflowDsl`
    ///
    /// 条件求值失败 = **图定义**层面的问题（表达式写错、引用了不存在的键）——
    /// 它不是「运行时资源不足」，也不是「上游挂了」。
    ///
    /// 映射到别的码会让运维以为「重试一下就好」——
    /// 而重试一个写错的表达式**永远不会**变对。
    fn from(err: ConditionError) -> Self {
        use nexus_core::error::{AppError, ErrorCode};
        AppError::new(ErrorCode::WorkflowDsl, err.to_string())
    }
}

/// 求值一个**布尔**条件表达式。
///
/// # 参数
///
/// - `expression` —— 条件表达式（如 `"count > 3 && mode == \"fast\""`）。
/// - `state` —— 变量来源。
///
/// # 返回
///
/// 表达式的布尔值。
///
/// # 错误（AC #6 / #7 / #8）
///
/// - 表达式语法错误 / 求值出错 → [`ConditionError::EvaluationFailed`]
/// - 结果**不是布尔** → [`ConditionError::NonBoolean`]
/// - 引用了**不存在**的变量 → [`ConditionError::UnknownVariable`]
/// - 引用了**类型不支持**的变量（对象） → [`ConditionError::UnsupportedValue`]
///
/// # 示例
///
/// ```
/// use nexus_workflow::condition::eval_condition;
/// use nexus_workflow::state::GraphState;
///
/// let mut state = GraphState::new();
/// state.insert("count", serde_json::json!(5)).unwrap();
///
/// assert!(eval_condition("count > 3", &state).unwrap());
/// assert!(eval_condition("count > 9", &state).is_ok());
/// ```
pub fn eval_condition(expression: &str, state: &GraphState) -> Result<bool, ConditionError> {
    let context = build_context(state);

    let value = evalexpr::eval_with_context(expression, &context)
        .map_err(|err| classify_error(expression, &err, state))?;

    match value {
        Value::Boolean(b) => Ok(b),
        other => Err(ConditionError::NonBoolean {
            expression: expression.to_string(),
            actual: describe(&other),
        }),
    }
}

/// 把图状态**装进**求值上下文。
///
/// # 为什么变量名要**过滤**
///
/// `evalexpr` 的标识符语法不允许某些字符（空格、点号等）。
/// 状态键里可能有这类字符 —— 它们**不可能**在表达式里被引用，
/// 所以**跳过**它们，而不是让整个上下文构建失败。
///
/// ⚠️ 被跳过的键如果被表达式引用 → 求值时报
/// [`ConditionError::UnknownVariable`] —— 仍然是**错误**，不是 `false`。
///
/// # 为什么**不支持的值**也跳过
///
/// 见模块文档「不支持的对象**不**让整个求值失败」。
/// 被跳过的值如果被引用 → [`ConditionError::UnsupportedValue`]。
fn build_context(state: &GraphState) -> HashMapContext<DefaultNumericTypes> {
    let mut context = HashMapContext::<DefaultNumericTypes>::new();

    for (key, value) in state.iter() {
        if !is_valid_identifier(key) {
            continue;
        }
        let Some(mapped) = map_value(value) else {
            continue;
        };
        // `set_value` 只在「同名变量已存在且类型不同」时失败 ——
        // 这里每个键只装一次，不会触发那个分支。
        let _ = context.set_value(key.clone(), mapped);
    }

    context
}

/// 标识符**是否合法** —— 能否在 `evalexpr` 表达式里被引用。
///
/// # 判据
///
/// 首字符必须是 ASCII 字母或下划线；
/// 后续字符必须是 ASCII 字母、数字或下划线。
///
/// # ⚠️ 为什么用**保守**判据
///
/// `evalexpr` 的词法分析器比这更宽松（它允许更多字符），
/// 但**宽松**意味着「同一个键在两个地方能不能引用」不一致 ——
/// 保守判据保证「能引用的键**一定**能引用」。
fn is_valid_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 把一个 JSON 值映射成 `evalexpr` 的值。
///
/// # 返回
///
/// `None` = 这个值**不支持**参与条件求值（目前只有对象）。
fn map_value(value: &serde_json::Value) -> Option<Value> {
    use serde_json::Value as Json;

    match value {
        Json::Bool(b) => Some(Value::Boolean(*b)),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::Int(i))
            } else {
                n.as_f64().map(Value::Float)
            }
        }
        Json::String(s) => Some(Value::String(s.clone())),
        Json::Array(items) => {
            let mut tuple = Vec::with_capacity(items.len());
            for item in items {
                tuple.push(map_value(item)?);
            }
            Some(Value::Tuple(tuple))
        }
        Json::Null => Some(Value::Empty),
        Json::Object(_) => None,
    }
}

/// 把 `evalexpr` 的值渲染成**可读**形态 —— 用于报错。
fn describe(value: &Value) -> String {
    match value {
        Value::String(s) => format!("字符串 {s:?}"),
        Value::Float(f) => format!("浮点数 {f}"),
        Value::Int(i) => format!("整数 {i}"),
        Value::Boolean(b) => format!("布尔 {b}"),
        Value::Tuple(items) => format!("元组（{} 个元素）", items.len()),
        Value::Empty => "空值".to_string(),
    }
}

/// 把 `evalexpr` 的错误**细分** —— 让「变量不存在」有**专属**错误类型。
///
/// # 为什么需要这一步
///
/// `evalexpr` 对「变量不存在」报 [`EvalexprError::VariableIdentifierNotFound`]，
/// 并**带上**变量名。
///
/// 但「变量不存在」有**两种**成因，需要**区分**：
///
/// | 成因 | 错误 |
/// |---|---|
/// | 状态里**根本没有**这个键 | [`ConditionError::UnknownVariable`] |
/// | 状态里**有**这个键，但值**不支持**（对象） | [`ConditionError::UnsupportedValue`] |
///
/// 区分它们的理由：前者是「表达式引用了不存在的键」（定义写错了），
/// 后者是「表达式引用了一个不能比较的值」。
/// 两种的**修法**不同 —— 混在一起报，运维只能自己逐个试。
///
/// # 错误
///
/// - 变量不存在 → [`ConditionError::UnknownVariable`]
/// - 变量存在但类型不支持 → [`ConditionError::UnsupportedValue`]
/// - 其它求值错误 → [`ConditionError::EvaluationFailed`]
fn classify_error(expression: &str, err: &EvalexprError, state: &GraphState) -> ConditionError {
    match err {
        EvalexprError::VariableIdentifierNotFound(name) => match state.get(name) {
            Some(value) => ConditionError::UnsupportedValue {
                variable: name.clone(),
                kind: json_kind(value),
            },
            None => ConditionError::UnknownVariable {
                variable: name.clone(),
            },
        },
        other => ConditionError::EvaluationFailed {
            expression: expression.to_string(),
            reason: other.to_string(),
        },
    }
}

/// JSON 值的类型名 —— 用于报错。
fn json_kind(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "Null".to_string(),
        serde_json::Value::Bool(_) => "Bool".to_string(),
        serde_json::Value::Number(_) => "Number".to_string(),
        serde_json::Value::String(_) => "String".to_string(),
        serde_json::Value::Array(_) => "Array".to_string(),
        serde_json::Value::Object(_) => "Object".to_string(),
    }
}
