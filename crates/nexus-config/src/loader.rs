//! 配置文件加载。
//!
//! 架构文档 §2.7 —— 文件层来自 `configs/*.yaml`。
//!
//! 目录约定（与参考实现一致）：
//!
//! ```text
//! configs/
//! ├── config.yaml        # 主配置，入库
//! ├── local.yaml         # 本地覆盖，不入库（.gitignore）
//! └── secrets.yaml       # 密钥，不入库（.gitignore）
//! ```

use std::path::{Path, PathBuf};

use serde_json::Value;

/// 配置文件目录的默认名。
pub const DEFAULT_CONFIG_DIR: &str = "configs";

/// 主配置文件名。
pub const DEFAULT_CONFIG_FILE: &str = "config.yaml";

/// 打包进二进制的默认配置（base 层）。
///
/// 用 `include_str!` 编译进二进制 —— 这样 base 层**不需要**外部文件就能工作，
/// 符合架构文档「打包进二进制的默认值」的描述。
pub const DEFAULT_CONFIG: &str = include_str!("../resources/default.yaml");

/// 配置加载错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 配置文件不存在。
    #[error("配置文件不存在：{0}")]
    NotFound(PathBuf),
    /// 配置文件读取失败。
    #[error("读取配置文件 {path} 失败：{source}")]
    Io {
        /// 文件路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: std::io::Error,
    },
    /// 配置文件解析失败。
    #[error("解析配置文件 {path} 失败：{source}")]
    Parse {
        /// 文件路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: serde_yaml_ng::Error,
    },
}

/// 解析一份 YAML 为 JSON 值树。
pub fn parse_yaml(content: &str, path: &Path) -> Result<Value, ConfigError> {
    serde_yaml_ng::from_str(content).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

/// 读取单个 YAML 配置文件。
pub fn load_yaml_file(path: &Path) -> Result<Value, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ConfigError::NotFound(path.to_path_buf())
        } else {
            ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;

    parse_yaml(&content, path)
}

/// 加载配置目录下的所有 `*.yaml` 文件并深合并。
///
/// 文件按**名字排序**处理 —— 保证加载顺序确定，结果可复现。
/// `local.yaml` 与 `secrets.yaml` 排在后面（`local` > `config` > `secrets` 的字典序
/// 并非如此，故此处**显式**排序规则见下）。
///
/// 实际上按字典序：`config.yaml` < `local.yaml` < `secrets.yaml` —— 恰好满足
/// 「local 覆盖 config、secrets 最后覆盖」的期望。
pub fn load_config_dir(dir: &Path) -> Result<Value, ConfigError> {
    if !dir.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| ConfigError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|e| e.to_str()) == Some("yaml")
                || path.extension().and_then(|e| e.to_str()) == Some("yml")
        })
        .collect();

    // 按路径排序，保证加载顺序确定。
    entries.sort();

    let mut merged = Value::Object(serde_json::Map::new());
    for path in entries {
        let value = load_yaml_file(&path)?;
        merge_into(&mut merged, &value);
    }

    Ok(merged)
}

/// 深合并：`overlay` 覆盖 `base`（对象递归合并，其余整体覆盖）。
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_yaml() {
        let value = parse_yaml("a: 1\nb:\n  c: x\n", Path::new("test.yaml")).unwrap();

        assert_eq!(value, json!({"a": 1, "b": {"c": "x"}}));
    }

    #[test]
    fn invalid_yaml_reports_error() {
        let err = parse_yaml("a: [unclosed\n", Path::new("test.yaml")).unwrap_err();

        assert!(matches!(err, ConfigError::Parse { .. }));
    }

    #[test]
    fn missing_dir_yields_empty_object() {
        let value = load_config_dir(Path::new("/nonexistent/configs")).unwrap();

        assert_eq!(value, json!({}));
    }

    #[test]
    fn multiple_files_deep_merge_in_name_order() {
        let dir = std::env::temp_dir().join(format!("nexus-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.yaml"),
            "log:\n  level: info\n  format: text\n",
        )
        .unwrap();
        std::fs::write(dir.join("local.yaml"), "log:\n  level: debug\n").unwrap();

        let value = load_config_dir(&dir).unwrap();

        // local.yaml 覆盖 config.yaml 的 level，format 保留
        assert_eq!(value, json!({"log": {"level": "debug", "format": "text"}}));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn default_config_parses_and_is_non_empty() {
        let value = parse_yaml(DEFAULT_CONFIG, Path::new("default.yaml")).unwrap();

        assert!(value.is_object());
        assert!(!value.as_object().unwrap().is_empty());
    }
}
