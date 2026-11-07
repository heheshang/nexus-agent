//! 模板渲染 —— 工具参数里的 `{{...}}` 占位符。
//!
//! # 本模块负责什么
//!
//! [`render_args`] —— 把一份 JSON 里的 `{{变量}}` 占位符
//! 用图状态里的值替换掉。
//!
//! # 三条规则（AC #9 / #10 / #11）
//!
//! | 规则 | 语义 |
//! |---|---|
//! | 整串是 `{{...}}` 且**只出现一次** | **保留原始类型**（数字仍是数字） |
//! | 否则 | 文本插值 |
//! | 插值结果**仍含** `{{` | **错误**（未解析的工具参数模板） |
//! | 嵌套 map / slice | **递归**渲染 |
//!
//! # 为什么「整串占位符」要**保留原始类型**
//!
//! 工具的参数是**有类型的** —— `{"limit": "{{count}}"}` 里的 `limit`
//! 期望是**数字**，不是字符串 `"5"`。
//!
//! 照抄参考架构 §3.5.4（Go 版 `graph.go:405`）：
//! 整个字符串是 `{{...}}` 且只出现一次 → **保留原始类型**。
//!
//! 举例：
//!
//! | 模板 | 状态 `count = 5` | 结果 |
//! |---|---|---|
//! | `"{{count}}"` | 数字 `5` | 数字 `5`（**不是** `"5"`） |
//! | `"共 {{count}} 个"` | 数字 `5` | 字符串 `"共 5 个"` |
//! | `{"n": "{{count}}"}` | 数字 `5` | `{"n": 5}` |
//!
//! # ⚠️ 「仍含 `{{`」为什么要**报错**
//!
//! 未解析的占位符如果**放行**，工具会收到一个字面量 `"{{count}}"` ——
//! 它可能恰好是**合法**的参数，于是**静默**地用错值跑完了。
//!
//! 报错让「模板没渲染对」在**渲染期**暴露，而不是在**工具行为**里暴露。
//!
//! # 变量路径
//!
//! 占位符里写**状态键**（如 `{{count}}`）。
//! **不支持**点号路径 —— 状态是**扁平**的一层键值表，
//! 点号路径需要额外的寻址语义，那属于 DSL 文本前端（决策 D15 · OQ-2）。

use serde_json::Value;

use crate::state::GraphState;

/// **模板渲染错误**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// 占位符引用了**不存在**的状态键。
    UnknownVariable {
        /// 变量名。
        variable: String,
    },

    /// 渲染结果**仍含** `{{`（AC #10）。
    ///
    /// 携带残留片段 —— 报错要能指出**哪里**没解析干净。
    Unresolved {
        /// 残留的片段。
        fragment: String,
    },

    /// 占位符**语法**非法（没有闭合的 `}}`）。
    Malformed {
        /// 出问题的片段。
        fragment: String,
    },
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownVariable { variable } => {
                write!(f, "模板引用了不存在的变量：{variable}")
            }
            Self::Unresolved { fragment } => {
                write!(f, "模板渲染后仍含未解析的占位符：{fragment}")
            }
            Self::Malformed { fragment } => {
                write!(f, "模板占位符语法非法：{fragment}")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

impl From<TemplateError> for nexus_core::error::AppError {
    /// 把模板错误映射成**应用层**错误。
    ///
    /// # 映射规则
    ///
    /// 全部 → `InvalidParam`（10001）。
    ///
    /// # ⚠️ 为什么是**参数**段而不是 `WorkflowDsl`
    ///
    /// 模板是**工具调用的参数**，不是图定义本身。
    /// 「这个工具参数模板引用了不存在的变量」是**调用方**的问题 ——
    /// 归**参数段**（1xxxx）。
    fn from(err: TemplateError) -> Self {
        use nexus_core::error::{AppError, ErrorCode};
        AppError::new(ErrorCode::InvalidParam, err.to_string())
    }
}

/// 渲染一份 JSON 里的占位符。
///
/// # 参数
///
/// - `value` —— 待渲染的 JSON（任意形状）。
/// - `state` —— 变量来源。
///
/// # 返回
///
/// 渲染后的 JSON。
///
/// # 错误
///
/// - 占位符引用了不存在的变量 → [`TemplateError::UnknownVariable`]
/// - 占位符语法非法 → [`TemplateError::Malformed`]
/// - 渲染结果仍含 `{{` → [`TemplateError::Unresolved`]
///
/// # 示例
///
/// ```
/// use nexus_workflow::state::GraphState;
/// use nexus_workflow::template::render_args;
///
/// let mut state = GraphState::new();
/// state.insert("count", serde_json::json!(5)).unwrap();
///
/// // 整串占位符 → 保留原始类型
/// assert_eq!(
///     render_args(&serde_json::json!("{{count}}"), &state).unwrap(),
///     serde_json::json!(5)
/// );
///
/// // 文本插值
/// assert_eq!(
///     render_args(&serde_json::json!("共 {{count}} 个"), &state).unwrap(),
///     serde_json::json!("共 5 个")
/// );
/// ```
pub fn render_args(value: &Value, state: &GraphState) -> Result<Value, TemplateError> {
    match value {
        Value::String(text) => render_string(text, state),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, item) in map {
                // ⚠️ **键**不渲染 —— 键是工具参数的**名字**，
                // 不是用户数据。渲染键会让「参数名」变得不可预测。
                out.insert(key.clone(), render_args(item, state)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(render_args(item, state)?);
            }
            Ok(Value::Array(out))
        }
        // 数字 / 布尔 / null —— 没有可渲染的东西。
        other => Ok(other.clone()),
    }
}

/// 渲染一个**字符串**。
///
/// # 三条分支
///
/// 1. **整串是一个占位符** → 返回状态的**原始值**（保留类型）；
/// 2. **含占位符** → 文本插值；
/// 3. **不含占位符** → 原样返回。
fn render_string(text: &str, state: &GraphState) -> Result<Value, TemplateError> {
    if let Some(name) = whole_placeholder(text) {
        // ── 分支 1：整串占位符 ──
        return state
            .get(name)
            .cloned()
            .ok_or_else(|| TemplateError::UnknownVariable {
                variable: name.to_string(),
            });
    }

    if !text.contains("{{") {
        // ── 分支 3：没有占位符 ──
        return Ok(Value::String(text.to_string()));
    }

    // ── 分支 2：文本插值 ──
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];

        let Some(end) = after.find("}}") else {
            // 没有闭合的 `}}` —— 语法非法。
            return Err(TemplateError::Malformed {
                fragment: rest[start..].to_string(),
            });
        };

        let name = after[..end].trim();
        if name.is_empty() {
            return Err(TemplateError::Malformed {
                fragment: rest[start..start + 2 + end + 2].to_string(),
            });
        }

        let value = state
            .get(name)
            .ok_or_else(|| TemplateError::UnknownVariable {
                variable: name.to_string(),
            })?;

        out.push_str(&stringify(value));
        rest = &after[end + 2..];
    }

    out.push_str(rest);

    // ── 残留检查（AC #10）──
    // 插值的值里**可能**自己带 `{{`（比如状态里存了一段未渲染的模板）。
    // 那必须报错 —— 放行等于把未解析的占位符交给工具。
    if let Some(idx) = out.find("{{") {
        return Err(TemplateError::Unresolved {
            fragment: out[idx..].to_string(),
        });
    }

    Ok(Value::String(out))
}

/// 判定一个字符串**是否**是「整串一个占位符」。
///
/// # 判据
///
/// - 去掉首尾空白后以 `{{` 开头；
/// - 第一个 `}}` 必须在**末尾**（否则不止一个占位符）。
///
/// # ⚠️ 为什么**先找第一个** `}}` 而不是「掐头去尾」
///
/// `"{{a}}}"` 的正确切分是「占位符 `a` + 尾随的 `}`」——
/// 如果掐掉**最后**两个字符，会得到变量名 `"a}"`，
/// 于是「能引用的变量名」取决于怎么切 —— 那是**歧义**。
///
/// 取**第一个** `}}` 之后的剩余部分若非空 → 不是整串占位符。
///
/// # 返回
///
/// 占位符里的变量名；不是整串占位符则 `None`。
fn whole_placeholder(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("{{")?;
    let end = inner.find("}}")?;
    if end + 2 != inner.len() {
        return None;
    }
    let name = inner[..end].trim();
    if name.is_empty() {
        return None;
    }
    Some(name)
}

/// 把一个状态值渲染成**文本**（插值用）。
///
/// # ⚠️ 字符串**原样**插入，其余用 JSON 形态
///
/// - 字符串 → 原样（不加引号）；
/// - 数字 / 布尔 / null → JSON 字面量形态；
/// - 数组 / 对象 → 紧凑 JSON。
fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
