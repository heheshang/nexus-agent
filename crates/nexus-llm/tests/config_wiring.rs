//! 网关的**配置接线**测试。
//!
//! # 检验的设计条款
//!
//! | 测试 | 条款 |
//! |---|---|
//! | `the_timeout_comes_from_the_config_not_from_a_hardcoded_value` | `NX_LLM__REQUEST_TIMEOUT_SECS` 映射必须**真的生效** |
//! | `a_missing_timeout_falls_back_to_the_default` | 配置缺失不该让服务起不来 |
//! | `a_zero_timeout_falls_back_to_the_default` | 0 超时会让所有请求立即失败 —— 那是配置错误 |
//! | `the_concurrency_limit_comes_from_the_config` | `llm.max_conns_per_provider` 映射 |

use std::time::Duration;

use nexus_config::ConfigStack;
use nexus_llm::{GatewayConfig, ProviderConfig};

/// 构造一个只含 `llm` 段的配置栈。
fn stack_with(llm: serde_json::Value) -> ConfigStack {
    let mut stack = ConfigStack::new();
    stack.set_base(serde_json::json!({ "llm": llm }));
    stack
}

/// 构造一个空的配置栈（没有 `llm` 段）。
fn empty_stack() -> ConfigStack {
    let mut stack = ConfigStack::new();
    stack.set_base(serde_json::json!({}));
    stack
}

#[test]
fn the_timeout_comes_from_the_config_not_from_a_hardcoded_value() {
    let stack = stack_with(serde_json::json!({ "request_timeout_secs": 300 }));

    let config = ProviderConfig::from_config(&stack);

    assert_eq!(
        config.timeout,
        Duration::from_secs(300),
        "超时必须走配置路径 —— 运维设了 NX_LLM__REQUEST_TIMEOUT_SECS 就得生效"
    );
}

#[test]
fn the_timeout_default_matches_the_base_config() {
    // base 配置里是 `llm.request_timeout_secs: 120` ——
    // 缺省值必须与它一致，否则「不配」与「配错」的表现不一样。
    let stack = stack_with(serde_json::json!({ "request_timeout_secs": 120 }));

    let config = ProviderConfig::from_config(&stack);

    assert_eq!(config.timeout, Duration::from_secs(120));
    assert_eq!(
        ProviderConfig::default().timeout,
        Duration::from_secs(120),
        "默认超时应与 base 配置一致"
    );
}

#[test]
fn a_missing_timeout_falls_back_to_the_default() {
    // 配置栈不完整**不该**让服务起不来 ——
    // 那会让「忘了配一项」变成「服务不可用」。
    let stack = empty_stack();

    let config = ProviderConfig::from_config(&stack);

    assert_eq!(config.timeout, Duration::from_secs(120));
}

#[test]
fn a_zero_timeout_falls_back_to_the_default() {
    // 0 超时会让**每个**请求立即失败 —— 那是配置错误，不是「立即超时」。
    let stack = stack_with(serde_json::json!({ "request_timeout_secs": 0 }));

    let config = ProviderConfig::from_config(&stack);

    assert_eq!(
        config.timeout,
        Duration::from_secs(120),
        "0 超时应回退到缺省值 —— 否则所有请求都会立即失败"
    );
}

#[test]
fn the_concurrency_limit_comes_from_the_config() {
    let stack = stack_with(serde_json::json!({ "max_conns_per_provider": 4 }));

    let config = GatewayConfig::from_config(&stack);

    assert_eq!(config.max_conns_per_provider, 4);
}

#[test]
fn a_zero_concurrency_limit_falls_back_to_the_default() {
    // 0 并发会让所有请求卡在 `acquire` 上 —— 那是配置错误。
    let stack = stack_with(serde_json::json!({ "max_conns_per_provider": 0 }));

    let config = GatewayConfig::from_config(&stack);

    assert_eq!(config.max_conns_per_provider, 16);
}
