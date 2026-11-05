//! 本仓库的**声明式环境变量映射表**。
//!
//! 架构文档 §2.7 —— 「哪些环境变量能覆盖什么」必须**可枚举、可文档化**。
//!
//! 这份表就是那个可枚举的事实。改动它时同步更新 `docs/operations/configuration.md`。
//!
//! # 命名规则
//!
//! `NX_<段>__<段>__<键>` → 配置路径 `段.段.键`（全小写）。
//! 双下划线 `__` 是层级分隔符；单下划线是键名的一部分。

use crate::env::{EnvMapping, EnvMappingTable, EnvValueType};

/// 本仓库的映射表。
///
/// 这是一份**静态清单** —— 启动时可以打印它（`nexus config env`），
/// 运维手册可以直接引用它，测试可以断言它。
pub fn table() -> EnvMappingTable {
    EnvMappingTable::new()
        // ---- 日志 ----
        .with(EnvMapping::new(
            "LOG__LEVEL",
            "log.level",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "LOG__FORMAT",
            "log.format",
            EnvValueType::String,
        ))
        // ---- 服务 ----
        .with(EnvMapping::new(
            "SERVER__HOST",
            "server.host",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "SERVER__PORT",
            "server.port",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "SERVER__HEARTBEAT_SECS",
            "server.heartbeat_secs",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "SERVER__MAX_BODY_BYTES",
            "server.max_body_bytes",
            EnvValueType::Int,
        ))
        // ---- 数据库 ----
        .with(EnvMapping::new("DATABASE__URL", "database.url", EnvValueType::String).secret())
        .with(EnvMapping::new(
            "DATABASE__MAX_CONNECTIONS",
            "database.max_connections",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "DATABASE__CONNECT_TIMEOUT_SECS",
            "database.connect_timeout_secs",
            EnvValueType::Int,
        ))
        // ---- 派生存储（Redis / Milvus / S3）----
        .with(EnvMapping::new("REDIS__URL", "redis.url", EnvValueType::String).secret())
        .with(EnvMapping::new(
            "MILVUS__URL",
            "milvus.url",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "S3__BUCKET",
            "s3.bucket",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "S3__ENDPOINT",
            "s3.endpoint",
            EnvValueType::String,
        ))
        .with(EnvMapping::new("S3__ACCESS_KEY", "s3.access_key", EnvValueType::String).secret())
        .with(EnvMapping::new("S3__SECRET_KEY", "s3.secret_key", EnvValueType::String).secret())
        // ---- Worker ----
        .with(EnvMapping::new(
            "WORKER__LEASE_TTL_SECS",
            "worker.lease_ttl_secs",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "WORKER__POLL_INTERVAL_MS",
            "worker.poll_interval_ms",
            EnvValueType::Int,
        ))
        // ---- LLM 网关 ----
        .with(EnvMapping::new(
            "LLM__DEFAULT_PROVIDER",
            "llm.default_provider",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "LLM__REQUEST_TIMEOUT_SECS",
            "llm.request_timeout_secs",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "LLM__CONTEXT_BUDGET",
            "llm.context_budget",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "LLM__DEEPSEEK__BASE_URL",
            "llm.deepseek.base_url",
            EnvValueType::String,
        ))
        .with(
            EnvMapping::new(
                "LLM__DEEPSEEK__API_KEY",
                "llm.deepseek.api_key",
                EnvValueType::String,
            )
            .secret(),
        )
        .with(EnvMapping::new(
            "LLM__DEEPSEEK__MODEL",
            "llm.deepseek.model",
            EnvValueType::String,
        ))
        .with(EnvMapping::new(
            "LLM__MAX_CONNS_PER_PROVIDER",
            "llm.max_conns_per_provider",
            EnvValueType::Int,
        ))
        // ---- 沙箱 ----
        .with(EnvMapping::new(
            "SANDBOX__TIMEOUT_SECS",
            "sandbox.timeout_secs",
            EnvValueType::Int,
        ))
        .with(EnvMapping::new(
            "SANDBOX__MAX_OUTPUT_BYTES",
            "sandbox.max_output_bytes",
            EnvValueType::Int,
        ))
        // ---- 观测 ----
        .with(EnvMapping::new(
            "OBSERVABILITY__OTLP_ENABLED",
            "observability.otlp_enabled",
            EnvValueType::Bool,
        ))
        .with(EnvMapping::new(
            "OBSERVABILITY__OTLP_ENDPOINT",
            "observability.otlp_endpoint",
            EnvValueType::String,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_table_is_not_empty() {
        assert!(table().len() > 20);
    }

    #[test]
    fn all_env_var_names_follow_convention() {
        for mapping in table().mappings() {
            assert!(
                mapping.env_var().starts_with("NX_"),
                "环境变量必须带 NX_ 前缀：{}",
                mapping.env_var()
            );
            // `__` 是层级分隔符；段内允许单下划线（如 `CONNECT_TIMEOUT_SECS`），
            // 但不允许段首/段尾下划线或连续三个下划线 —— 那会让切分产生歧义。
            let segments: Vec<&str> = mapping.env_key.split("__").collect();
            assert!(
                segments
                    .iter()
                    .all(|s| !s.starts_with('_') && !s.ends_with('_')),
                "键名段的首尾不应出现下划线（切分会歧义）：{}",
                mapping.env_key
            );
            assert!(
                !mapping.env_key.contains("___"),
                "不应出现连续三个下划线：{}",
                mapping.env_key
            );
            assert!(
                !mapping.config_path.contains("__"),
                "配置路径用单点分隔：{}",
                mapping.config_path
            );
        }
    }

    #[test]
    fn config_paths_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for mapping in table().mappings() {
            assert!(
                seen.insert(mapping.config_path.clone()),
                "配置路径重复：{}",
                mapping.config_path
            );
        }
    }

    #[test]
    fn secret_vars_are_marked_sensitive() {
        for mapping in table().mappings() {
            if mapping.config_path.ends_with("api_key")
                || mapping.config_path.ends_with("secret_key")
                || mapping.config_path.ends_with("url") && mapping.config_path.contains("database")
            {
                assert!(mapping.secret, "敏感变量未标记：{}", mapping.config_path);
            }
        }
    }

    #[test]
    fn all_env_var_names_are_enumerable() {
        let names = table().declared_env_vars();

        assert!(names.contains(&"NX_LOG__LEVEL".to_string()));
        assert!(names.contains(&"NX_LLM__DEEPSEEK__BASE_URL".to_string()));
        assert!(names.windows(2).all(|w| w[0] <= w[1]), "应已排序");
    }

    #[test]
    fn mapping_path_resolves_in_config_tree() {
        use crate::env::MapEnv;

        let env = MapEnv::new().with("LLM__DEEPSEEK__BASE_URL", "http://localhost:11434");
        let layer = table().build_layer(&env).unwrap();

        assert_eq!(
            layer["llm"]["deepseek"]["base_url"],
            serde_json::json!("http://localhost:11434")
        );
    }
}
