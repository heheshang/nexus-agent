//! 配置栈：三层来源，按优先级合并。
//!
//! 架构文档 §2.7 —— `ConfigStack { base, file, env }`，优先级从低到高。
//!
//! # 为什么是三层而不是一个扁平 map
//!
//! 三层**在类型上可区分**（`ConfigLayer` 枚举），而不是三个同类型 `Config` 塞进
//! `HashMap` 后合并。区别在于：合并之后仍然能回答
//! 「这个值**从哪来**」—— 排查配置问题时这是第一个要问的问题。

use serde_json::{Map, Value};

/// 配置来源层。**Ord 的顺序即优先级**：`Base < File < Env`。
///
/// 变体顺序不可随意调整 —— 合并顺序依赖它。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigLayer {
    /// 打包进二进制的默认值。最低优先级。
    Base,
    /// 来自 `configs/*.yaml`。中优先级。
    File,
    /// 来自 `NX_` 前缀环境变量。最高优先级。
    Env,
}

impl ConfigLayer {
    /// 全部层，按优先级从低到高。
    pub const ALL: [ConfigLayer; 3] = [ConfigLayer::Base, ConfigLayer::File, ConfigLayer::Env];

    /// 层的稳定名称（用于日志与文档）。
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigLayer::Base => "base",
            ConfigLayer::File => "file",
            ConfigLayer::Env => "env",
        }
    }
}

impl std::fmt::Display for ConfigLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 配置栈。三层来源，优先级 `base < file < env`。
///
/// # 合并语义
///
/// 逐层深合并（deep merge）：
///
/// - 两层都是对象 → 递归合并，键取并集
/// - 否则高优先级层**整体覆盖**低优先级层
///
/// 也就是说 `file` 里的 `{"a": {"b": 1}}` 与 `env` 里的 `{"a": {"c": 2}}`
/// 合并后是 `{"a": {"b": 1, "c": 2}}` —— 环境变量覆盖的是**叶子**，不是整个子树。
///
/// # 示例
///
/// ```
/// use nexus_config::{ConfigStack, ConfigLayer};
/// use serde_json::json;
///
/// let mut stack = ConfigStack::new();
/// stack.set_base(json!({"log": {"level": "info"}}));
/// stack.set_file(json!({"log": {"level": "debug"}}));
///
/// // 高优先级层覆盖低优先级层
/// assert_eq!(stack.get("log.level"), Some(&json!("debug")));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigStack {
    base: Value,
    file: Value,
    env: Value,
    /// 三层合并后的结果。**由 setter 维护**，避免每次查询都重新深合并。
    merged: Value,
}

impl ConfigStack {
    /// 空配置栈。
    pub fn new() -> Self {
        Self {
            base: Value::Object(Map::new()),
            file: Value::Object(Map::new()),
            env: Value::Object(Map::new()),
            merged: Value::Object(Map::new()),
        }
    }

    /// 设置基础层（打包默认值）。**整体替换**该层。
    pub fn set_base(&mut self, value: Value) {
        self.base = value;
        self.recompute();
    }

    /// 设置文件层。**整体替换**该层。
    pub fn set_file(&mut self, value: Value) {
        self.file = value;
        self.recompute();
    }

    /// 设置环境变量层。**整体替换**该层。
    pub fn set_env(&mut self, value: Value) {
        self.env = value;
        self.recompute();
    }

    /// 重新计算合并结果。
    fn recompute(&mut self) {
        let mut result = self.base.clone();
        merge_into(&mut result, &self.file);
        merge_into(&mut result, &self.env);
        self.merged = result;
    }

    /// 基础层（只读）。
    pub fn base(&self) -> &Value {
        &self.base
    }

    /// 文件层（只读）。
    pub fn file(&self) -> &Value {
        &self.file
    }

    /// 环境变量层（只读）。
    pub fn env(&self) -> &Value {
        &self.env
    }

    /// 取某一层的值（只读）。
    pub fn layer(&self, layer: ConfigLayer) -> &Value {
        match layer {
            ConfigLayer::Base => &self.base,
            ConfigLayer::File => &self.file,
            ConfigLayer::Env => &self.env,
        }
    }

    /// 合并后的最终值。
    pub fn merged(&self) -> &Value {
        &self.merged
    }

    /// 按点分路径取值，例如 `"llm.deepseek.base_url"`。
    ///
    /// 路径不存在返回 `None`。数组索引用数字段名：`"providers.0.name"`。
    pub fn get(&self, path: &str) -> Option<&Value> {
        get_path(&self.merged, path)
    }

    /// 取值并转为字符串。仅支持字符串与数字。
    pub fn get_str(&self, path: &str) -> Option<String> {
        match self.get(path)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// 取值并转为 `u64`。
    pub fn get_u64(&self, path: &str) -> Option<u64> {
        match self.get(path)? {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// 取值并转为 `bool`。
    ///
    /// 接受 `true` / `false`（大小写不敏感）。其余值视为 `None` ——
    /// **不做**「非空即真」这类宽松解释。
    pub fn get_bool(&self, path: &str) -> Option<bool> {
        match self.get(path)? {
            Value::Bool(b) => Some(*b),
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            _ => None,
        }
    }

    /// 某个路径的值**来自哪一层**（优先级最高的、提供了该路径的层）。
    ///
    /// 排查配置问题时最常问的问题：「这个值为什么是这样」。
    pub fn origin(&self, path: &str) -> Option<ConfigLayer> {
        // 从高优先级往低找，第一个含该路径的层就是来源。
        [ConfigLayer::Env, ConfigLayer::File, ConfigLayer::Base]
            .into_iter()
            .find(|layer| get_path(self.layer(*layer), path).is_some())
    }
}

/// 深合并：`overlay` 覆盖 `base`。
///
/// 两层都是对象时递归合并；否则 `overlay` 整体覆盖。
fn merge_into(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(base_value) => merge_into(base_value, overlay_value),
                    None => {
                        base_map.insert(key.clone(), overlay_value.clone());
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value.clone();
        }
    }
}

/// 按点分路径在 `Value` 中取值。
fn get_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => {
                let index: usize = segment.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn layer_priority_order_is_correct() {
        assert!(ConfigLayer::Base < ConfigLayer::File);
        assert!(ConfigLayer::File < ConfigLayer::Env);
        assert_eq!(ConfigLayer::ALL.len(), 3);
    }

    #[test]
    fn higher_priority_layer_overrides_lower() {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({"log": {"level": "info", "format": "text"}}));
        stack.set_file(json!({"log": {"level": "debug"}}));

        assert_eq!(stack.get("log.level"), Some(&json!("debug")));
        // 未被覆盖的键保留低优先级的值
        assert_eq!(stack.get("log.format"), Some(&json!("text")));
    }

    #[test]
    fn deep_merge_overrides_leaves_only() {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({"llm": {"deepseek": {"base_url": "http://a", "model": "m"}}}));
        stack.set_env(json!({"llm": {"deepseek": {"base_url": "http://b"}}}));

        assert_eq!(stack.get("llm.deepseek.base_url"), Some(&json!("http://b")));
        assert_eq!(stack.get("llm.deepseek.model"), Some(&json!("m")));
    }

    #[test]
    fn non_object_layer_is_replaced_wholesale() {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({"timeout": 30}));
        stack.set_file(json!({"timeout": 60}));

        assert_eq!(stack.get("timeout"), Some(&json!(60)));
    }

    #[test]
    fn origin_报告值的来源层() {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({"log": {"level": "info"}}));
        stack.set_file(json!({"log": {"level": "debug"}}));

        assert_eq!(stack.origin("log.level"), Some(ConfigLayer::File));
        assert_eq!(stack.origin("log.missing"), None);
    }

    #[test]
    fn env_层优先级高于_file() {
        let mut stack = ConfigStack::new();
        stack.set_file(json!({"a": 1}));
        stack.set_env(json!({"a": 2}));

        assert_eq!(stack.get("a"), Some(&json!(2)));
        assert_eq!(stack.origin("a"), Some(ConfigLayer::Env));
    }

    #[test]
    fn typed_value_lookup() {
        let mut stack = ConfigStack::new();
        stack.set_base(json!({
            "s": "text",
            "n": 42,
            "b": true,
            "bs": "false"
        }));

        assert_eq!(stack.get_str("s").as_deref(), Some("text"));
        assert_eq!(stack.get_u64("n"), Some(42));
        assert_eq!(stack.get_bool("b"), Some(true));
        assert_eq!(stack.get_bool("bs"), Some(false));
        // 不可转换时返回 None，不做宽松解释
        assert_eq!(stack.get_u64("s"), None);
        assert_eq!(stack.get_bool("s"), None);
    }

    #[test]
    fn empty_stack_yields_none() {
        let stack = ConfigStack::new();
        assert_eq!(stack.get("anything"), None);
        assert_eq!(stack.merged(), &json!({}));
    }
}
