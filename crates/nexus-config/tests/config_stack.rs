//! 配置栈的集成测试：验证三层优先级与「声明式映射」的实际行为。
//!
//! 架构文档 §2.7。

use nexus_config::mapping;
use nexus_config::{ConfigLayer, ConfigStack, EnvSource, MapEnv, ProcessEnv, apply_env};

/// 构造一个三层齐全的配置栈。
fn full_stack(env: &MapEnv) -> ConfigStack {
    let mut stack = ConfigStack::new();

    stack.set_base(nexus_config::parse_base_config());
    stack.set_file(serde_json::json!({
        "log": {"level": "info", "format": "text"},
        "server": {"port": 8080},
        "llm": {"deepseek": {"base_url": "http://file", "model": "from-file"}}
    }));

    apply_env(&mut stack, &mapping::table(), env).expect("环境变量应用应成功");

    stack
}

#[test]
fn three_layer_priority_env_over_file_over_base() {
    let env = MapEnv::new()
        .with("LOG__LEVEL", "debug")
        .with("SERVER__PORT", "9090");

    let stack = full_stack(&env);

    // env 层胜出
    assert_eq!(stack.get("log.level"), Some(&serde_json::json!("debug")));
    assert_eq!(stack.origin("log.level"), Some(ConfigLayer::Env));

    // file 层胜出（env 未设置）
    assert_eq!(stack.get("log.format"), Some(&serde_json::json!("text")));
    assert_eq!(stack.origin("log.format"), Some(ConfigLayer::File));

    // base 层胜出（file 与 env 均未涉及）
    assert_eq!(
        stack.get("worker.lease_ttl_secs"),
        Some(&serde_json::json!(30))
    );
    assert_eq!(
        stack.origin("worker.lease_ttl_secs"),
        Some(ConfigLayer::Base)
    );
}

#[test]
fn env_overrides_leaves_not_whole_subtree() {
    let env = MapEnv::new().with("LLM__DEEPSEEK__BASE_URL", "http://env");

    let stack = full_stack(&env);

    assert_eq!(
        stack.get("llm.deepseek.base_url"),
        Some(&serde_json::json!("http://env"))
    );
    // 同一子树下的其他键保留 file 层的值
    assert_eq!(
        stack.get("llm.deepseek.model"),
        Some(&serde_json::json!("from-file"))
    );
}

#[test]
fn unset_env_vars_change_no_values() {
    let env = MapEnv::new();
    let with_env = full_stack(&env);

    let mut without_env = ConfigStack::new();
    without_env.set_base(nexus_config::parse_base_config());
    without_env.set_file(serde_json::json!({
        "log": {"level": "info", "format": "text"},
        "server": {"port": 8080},
        "llm": {"deepseek": {"base_url": "http://file", "model": "from-file"}}
    }));

    assert_eq!(with_env.merged(), without_env.merged());
}

#[test]
fn undeclared_env_vars_ignored_but_reported() {
    let env = MapEnv::new()
        .with("LOG__LEVEL", "debug")
        .with("TYPO__LOG__LEVEL", "warn");

    let mut stack = ConfigStack::new();
    let report = apply_env(&mut stack, &mapping::table(), &env).unwrap();

    assert_eq!(report.applied, vec!["NX_LOG__LEVEL"]);
    assert_eq!(report.unmapped, vec!["NX_TYPO__LOG__LEVEL"]);

    // 未声明的变量没有生效
    assert_eq!(stack.get("log.level"), Some(&serde_json::json!("debug")));
    assert_eq!(stack.get("typo.log.level"), None);
}

#[test]
fn wrong_value_type_reports_error() {
    let env = MapEnv::new().with("SERVER__PORT", "not-a-number");

    let mut stack = ConfigStack::new();
    let result = apply_env(&mut stack, &mapping::table(), &env);

    assert!(result.is_err(), "类型错误必须报错，不能静默跳过");
}

#[test]
fn mapping_table_enumerates_all_declared_env_vars() {
    let names = mapping::table().declared_env_vars();

    assert!(names.contains(&"NX_LOG__LEVEL".to_string()));
    assert!(names.contains(&"NX_DATABASE__URL".to_string()));
    assert!(names.contains(&"NX_LLM__DEEPSEEK__BASE_URL".to_string()));
}

#[test]
fn process_env_source_enumerates_prefixed_vars() {
    // 真实进程环境里可能没有 NX_ 变量，测试不应依赖具体环境。
    let keys = ProcessEnv.nx_keys();

    for key in &keys {
        assert!(!key.starts_with("NX_"), "nx_keys 应剥掉前缀：{key}");
    }
}
