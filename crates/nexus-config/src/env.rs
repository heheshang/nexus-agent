//! 声明式环境变量映射表。
//!
//! 架构文档 §2.7 —— 「环境变量用 `NX_` 前缀 + 层级路径（`NX_LLM__DEEPSEEK__BASE_URL`），
//! 由一个**声明式映射表**驱动，而不是 `set_var` 逐个覆盖」。
//!
//! # 为什么是映射表而不是自动匹配
//!
//! 参考实现（viper）的自动匹配让「哪些环境变量能覆盖什么」成为**运行时的副作用**：
//! 你无法枚举它、无法文档化它、无法测试它。映射表把这变成**静态事实** ——
//! 一份清单，可以打印、可以断言、可以写进运维手册。
//!
//! # 命名约定
//!
//! `NX_<段>__<段>__<键>` → 配置路径 `段.段.键`（全小写）。
//! 双下划线 `__` 是**层级分隔符**，单下划线是键名的一部分。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::stack::ConfigStack;

/// 环境变量名前缀。
pub const ENV_PREFIX: &str = "NX_";

/// 层级分隔符（双下划线）。
pub const PATH_SEPARATOR: &str = "__";

/// 环境变量的值类型。
///
/// 之所以要显式声明类型：环境变量**只有字符串**。不声明类型就只能靠猜
/// （"42" 是数字还是字符串？），而猜测会让 `"false"` 变成 `true` 这类事故。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvValueType {
    /// 原样字符串。
    String,
    /// 布尔值。接受 `true`/`false`（大小写不敏感）与 `1`/`0`。
    Bool,
    /// 整数。
    Int,
    /// 浮点数。
    Float,
    /// 逗号分隔的字符串列表。
    List,
}

impl EnvValueType {
    /// 该类型的可读名称（用于错误信息）。
    pub fn expected_name(self) -> &'static str {
        match self {
            EnvValueType::String => "string",
            EnvValueType::Bool => "bool (true/false/1/0)",
            EnvValueType::Int => "i64",
            EnvValueType::Float => "f64",
            EnvValueType::List => "逗号分隔的列表",
        }
    }

    /// 把环境变量的字符串值转成对应的 JSON 值。
    ///
    /// 转换失败返回 `Err` —— 不做「猜一个默认值」的兜底。
    ///
    /// `env_var` 仅用于错误信息（完整名称，含 `NX_` 前缀）。
    pub fn parse(self, raw: &str, env_var: &str) -> Result<Value, EnvError> {
        let expected = self.expected_name().to_string();

        match self {
            EnvValueType::String => Ok(Value::String(raw.to_string())),
            EnvValueType::Bool => match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(Value::Bool(true)),
                "false" | "0" => Ok(Value::Bool(false)),
                other => Err(EnvError::InvalidValue {
                    env_var: env_var.to_string(),
                    raw: raw.to_string(),
                    expected,
                    detail: format!("无法解析为布尔值：{other}"),
                }),
            },
            EnvValueType::Int => {
                raw.trim()
                    .parse::<i64>()
                    .map(Value::from)
                    .map_err(|e| EnvError::InvalidValue {
                        env_var: env_var.to_string(),
                        raw: raw.to_string(),
                        expected,
                        detail: e.to_string(),
                    })
            }
            EnvValueType::Float => raw
                .trim()
                .parse::<f64>()
                .map_err(|e| EnvError::InvalidValue {
                    env_var: env_var.to_string(),
                    raw: raw.to_string(),
                    expected: expected.clone(),
                    detail: e.to_string(),
                })
                .and_then(|f| {
                    serde_json::Number::from_f64(f)
                        .map(Value::Number)
                        .ok_or_else(|| EnvError::InvalidValue {
                            env_var: env_var.to_string(),
                            raw: raw.to_string(),
                            expected,
                            detail: "非有限浮点数无法表示为 JSON".to_string(),
                        })
                }),
            EnvValueType::List => Ok(Value::Array(
                raw.split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| Value::String(s.to_string()))
                    .collect(),
            )),
        }
    }
}

/// 一条环境变量映射。
///
/// 描述「哪个环境变量覆盖哪个配置路径，值是什么类型」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvMapping {
    /// 环境变量名（**不含** `NX_` 前缀），如 `LOG__LEVEL`。
    pub env_key: String,
    /// 目标配置路径（点分），如 `log.level`。
    pub config_path: String,
    /// 值类型。
    pub value_type: EnvValueType,
    /// 是否为敏感值（写入日志时脱敏）。
    pub secret: bool,
}

impl EnvMapping {
    /// 构造一条映射。
    pub fn new(
        env_key: impl Into<String>,
        config_path: impl Into<String>,
        value_type: EnvValueType,
    ) -> Self {
        Self {
            env_key: env_key.into(),
            config_path: config_path.into(),
            value_type,
            secret: false,
        }
    }

    /// 标记为敏感值。
    #[must_use]
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// 完整的环境变量名（含 `NX_` 前缀）。
    pub fn env_var(&self) -> String {
        format!("{ENV_PREFIX}{}", self.env_key.replace(PATH_SEPARATOR, "__"))
    }
}

/// 环境变量映射表。
///
/// 这是一份**静态清单** —— 「哪些环境变量能覆盖什么」因此可枚举、可文档化、可测试。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvMappingTable {
    mappings: Vec<EnvMapping>,
}

impl EnvMappingTable {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条映射（链式）。
    #[must_use]
    pub fn with(mut self, mapping: EnvMapping) -> Self {
        self.mappings.push(mapping);
        self
    }

    /// 追加一条映射（可变）。
    pub fn push(&mut self, mapping: EnvMapping) {
        self.mappings.push(mapping);
    }

    /// 全部映射。
    pub fn mappings(&self) -> &[EnvMapping] {
        &self.mappings
    }

    /// 映射条目数。
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    /// 按环境变量键查找映射。
    pub fn find(&self, env_key: &str) -> Option<&EnvMapping> {
        self.mappings.iter().find(|m| m.env_key == env_key)
    }

    /// 全部映射声明的环境变量名（完整形式，含前缀）。
    ///
    /// 这是「可枚举」的**直接产物** —— 可用于生成文档、打印启动日志、或写测试断言。
    pub fn declared_env_vars(&self) -> Vec<String> {
        let mut names: Vec<String> = self.mappings.iter().map(|m| m.env_var()).collect();
        names.sort();
        names
    }

    /// 从环境变量集合生成配置树。
    ///
    /// # 行为
    ///
    /// - **只处理**映射表里声明过的环境变量 —— 未声明的 `NX_*` 变量被**忽略**。
    /// - 同一目标路径被多条映射命中时，**后声明的覆盖先声明的**（与 `Vec` 顺序一致）。
    /// - 值类型解析失败 → 返回 `Err`（不静默跳过）。
    ///
    /// # 参数
    ///
    /// - `env`: 环境变量来源。生产用 [`ProcessEnv`]，测试用 [`MapEnv`]。
    pub fn build_layer(&self, env: &dyn EnvSource) -> Result<Value, EnvError> {
        let mut layer = serde_json::Map::new();

        for mapping in &self.mappings {
            let Some(raw) = env.get(&mapping.env_key) else {
                continue;
            };

            let parsed = mapping.value_type.parse(&raw, &mapping.env_var())?;

            set_path(&mut layer, &mapping.config_path, parsed);
        }

        Ok(Value::Object(layer))
    }
}

/// 环境变量来源。
///
/// 抽成 trait 是为了让测试**不依赖真实进程环境** —— 进程环境是全局状态，
/// 测试里改它会互相干扰（Rust 2024 起 `set_var` 还是 `unsafe`）。
pub trait EnvSource {
    /// 取环境变量的值。
    ///
    /// `key` **不含** `NX_` 前缀（如 `LOG__LEVEL`）。
    fn get(&self, key: &str) -> Option<String>;

    /// 全部 `NX_` 前缀环境变量的键（不含前缀）。
    ///
    /// 用于报告「有哪些 `NX_*` 变量没有被映射表覆盖」。
    fn nx_keys(&self) -> Vec<String>;
}

/// 真实进程环境。
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(format!("{ENV_PREFIX}{key}")).ok()
    }

    fn nx_keys(&self) -> Vec<String> {
        std::env::vars()
            .filter_map(|(name, _)| name.strip_prefix(ENV_PREFIX).map(|k| k.to_string()))
            .collect()
    }
}

/// 内存里的环境变量集合（测试用）。
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    vars: BTreeMap<String, String>,
}

impl MapEnv {
    /// 空集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置一个变量。`key` **不含** `NX_` 前缀。
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// 设置一个变量（可变）。
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }
}

impl EnvSource for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn nx_keys(&self) -> Vec<String> {
        self.vars.keys().cloned().collect()
    }
}

/// 应用环境变量层的报告。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvApplyReport {
    /// 被映射表覆盖、且**实际设置了值**的环境变量。
    pub applied: Vec<String>,
    /// 未被映射表覆盖的 `NX_*` 变量。
    pub unmapped: Vec<String>,
}

/// 环境变量相关的错误。
#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    /// 环境变量的值无法按声明的类型解析。
    #[error("环境变量 {env_var} 的值 {raw:?} 不是合法的 {expected}：{detail}")]
    InvalidValue {
        /// 环境变量名（含前缀）。
        env_var: String,
        /// 原始值。
        raw: String,
        /// 期望的类型。
        expected: String,
        /// 解析器给出的细节。
        detail: String,
    },
}

/// 把环境变量应用到配置栈的 `env` 层，返回报告。
///
/// 未被映射表覆盖的 `NX_*` 变量**被忽略**（不报错）——
/// 但它们会出现在报告里，可以被日志或测试消费。
pub fn apply_env(
    stack: &mut ConfigStack,
    table: &EnvMappingTable,
    env: &dyn EnvSource,
) -> Result<EnvApplyReport, EnvError> {
    let layer = table.build_layer(env)?;
    stack.set_env(layer);

    let declared: Vec<&str> = table
        .mappings()
        .iter()
        .map(|m| m.env_key.as_str())
        .collect();
    let mut unmapped: Vec<String> = env
        .nx_keys()
        .into_iter()
        .filter(|k| !declared.contains(&k.as_str()))
        .map(|k| format!("{ENV_PREFIX}{k}"))
        .collect();
    unmapped.sort();

    let mut applied: Vec<String> = table
        .mappings()
        .iter()
        .filter(|m| env.get(&m.env_key).is_some())
        .map(|m| m.env_var())
        .collect();
    applied.sort();

    Ok(EnvApplyReport { applied, unmapped })
}

/// 在配置树里按点分路径设置值（创建缺失的中间对象）。
fn set_path(root: &mut serde_json::Map<String, Value>, path: &str, value: Value) {
    let segments: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (index, segment) in segments.iter().enumerate() {
        if index + 1 == segments.len() {
            current.insert((*segment).to_string(), value);
            return;
        }

        let entry = current
            .entry((*segment).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));

        if !entry.is_object() {
            *entry = Value::Object(serde_json::Map::new());
        }
        // 上面刚确保是对象，取 object_mut 必然成功。
        current = entry
            .as_object_mut()
            .expect("刚确保为对象，取 object_mut 必然成功");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table() -> EnvMappingTable {
        EnvMappingTable::new()
            .with(EnvMapping::new(
                "LOG__LEVEL",
                "log.level",
                EnvValueType::String,
            ))
            .with(EnvMapping::new(
                "SERVER__PORT",
                "server.port",
                EnvValueType::Int,
            ))
            .with(EnvMapping::new(
                "SERVER__DEBUG",
                "server.debug",
                EnvValueType::Bool,
            ))
            .with(EnvMapping::new("LLM__TAGS", "llm.tags", EnvValueType::List))
    }

    #[test]
    fn unset_env_vars_produce_no_entries() {
        let env = MapEnv::new();
        let layer = table().build_layer(&env).unwrap();

        assert_eq!(layer, json!({}));
    }

    #[test]
    fn set_env_var_overrides_matching_path() {
        let env = MapEnv::new()
            .with("LOG__LEVEL", "debug")
            .with("SERVER__PORT", "8080");

        let layer = table().build_layer(&env).unwrap();

        assert_eq!(layer["log"]["level"], json!("debug"));
        assert_eq!(layer["server"]["port"], json!(8080));
    }

    #[test]
    fn undeclared_env_vars_are_ignored() {
        let env = MapEnv::new()
            .with("LOG__LEVEL", "debug")
            .with("UNKNOWN__THING", "x");

        let layer = table().build_layer(&env).unwrap();

        assert_eq!(layer, json!({"log": {"level": "debug"}}));
    }

    #[test]
    fn unmapped_vars_appear_in_report() {
        let env = MapEnv::new()
            .with("LOG__LEVEL", "debug")
            .with("UNKNOWN__THING", "x");

        let mut stack = ConfigStack::new();
        let report = apply_env(&mut stack, &table(), &env).unwrap();

        assert_eq!(report.applied, vec!["NX_LOG__LEVEL"]);
        assert_eq!(report.unmapped, vec!["NX_UNKNOWN__THING"]);
    }

    #[test]
    fn wrong_value_type_errors_instead_of_skipping() {
        let env = MapEnv::new().with("SERVER__PORT", "not-a-number");

        let err = table().build_layer(&env).unwrap_err();

        match err {
            EnvError::InvalidValue {
                env_var, expected, ..
            } => {
                assert_eq!(env_var, "NX_SERVER__PORT");
                assert_eq!(expected, "i64");
            }
        }
    }

    #[test]
    fn bool_accepts_multiple_spellings() {
        for (raw, expected) in [
            ("true", true),
            ("TRUE", true),
            ("1", true),
            ("false", false),
            ("0", false),
        ] {
            let env = MapEnv::new().with("SERVER__DEBUG", raw);
            let layer = table().build_layer(&env).unwrap();
            assert_eq!(layer["server"]["debug"], json!(expected), "输入 {raw}");
        }
    }

    #[test]
    fn list_is_split_by_comma_and_trimmed() {
        let env = MapEnv::new().with("LLM__TAGS", "a, b ,c");
        let layer = table().build_layer(&env).unwrap();

        assert_eq!(layer["llm"]["tags"], json!(["a", "b", "c"]));
    }

    #[test]
    fn mapping_table_is_enumerable() {
        let names = table().declared_env_vars();

        assert_eq!(
            names,
            vec![
                "NX_LLM__TAGS",
                "NX_LOG__LEVEL",
                "NX_SERVER__DEBUG",
                "NX_SERVER__PORT",
            ]
        );
    }

    #[test]
    fn env_var_name_has_prefix_and_separator() {
        let mapping = EnvMapping::new(
            "LLM__DEEPSEEK__BASE_URL",
            "llm.deepseek.base_url",
            EnvValueType::String,
        );
        assert_eq!(mapping.env_var(), "NX_LLM__DEEPSEEK__BASE_URL");
    }

    #[test]
    fn later_mapping_for_same_path_wins() {
        let table = EnvMappingTable::new()
            .with(EnvMapping::new("A", "x.y", EnvValueType::String))
            .with(EnvMapping::new("B", "x.y", EnvValueType::String));

        let env = MapEnv::new().with("A", "first").with("B", "second");
        let layer = table.build_layer(&env).unwrap();

        assert_eq!(layer["x"]["y"], json!("second"));
    }

    #[test]
    fn applied_to_stack_overrides_file_layer() {
        let mut stack = ConfigStack::new();
        stack.set_file(json!({"log": {"level": "info"}}));

        let env = MapEnv::new().with("LOG__LEVEL", "debug");
        apply_env(&mut stack, &table(), &env).unwrap();

        assert_eq!(stack.get("log.level"), Some(&json!("debug")));
        assert_eq!(
            stack.origin("log.level"),
            Some(crate::stack::ConfigLayer::Env)
        );
    }
}
