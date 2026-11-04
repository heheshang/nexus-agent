//! `nexus-config` —— 分层配置与环境变量映射。
//!
//! # 职责边界
//!
//! 本 crate 负责**配置来源的显式化**：
//!
//! - 三层配置栈（`base` → `file` → `env`），优先级从低到高
//! - **声明式**的环境变量映射表 —— 「哪些环境变量能覆盖什么」是可枚举的事实
//! - 配置文件（YAML）的加载与合并
//!
//! # 明确**不做**的事
//!
//! - ❌ 不做业务语义解释（不校验「端口必须在 1–65535」这类业务规则）
//! - ❌ 不持有全局状态（配置由调用方持有并显式传递）
//! - ❌ 不做「自动匹配所有环境变量」—— 那是参考实现 viper 的行为，
//!   本设计刻意改为**显式映射**（架构文档 §2.7）
//!
//! # 关键设计
//!
//! 参考实现（Go + viper）的环境变量覆盖靠「viper 自动匹配」——
//! 哪些环境变量能覆盖什么，是**运行时的副作用**，不可枚举、不可文档化。
//!
//! 本设计用**声明式映射表**取代它：映射表是一份静态清单，
//! 每一项显式声明「环境变量名 → 配置路径 → 值类型」。
//! 「哪些环境变量能覆盖什么」因此变成**一个可以打印、可以测试、可以写进文档的事实**。
//!
//! # 参见
//!
//! - 架构文档 §2.7（配置）
//! - `.cursor/rules/rust-code-standards.mdc` §8（配置）

pub mod env;
pub mod loader;
pub mod mapping;
pub mod stack;

pub use env::{
    EnvApplyReport, EnvError, EnvMapping, EnvMappingTable, EnvSource, EnvValueType, MapEnv,
    ProcessEnv, apply_env,
};
pub use loader::{ConfigError, DEFAULT_CONFIG, load_config_dir, load_yaml_file};
pub use stack::{ConfigLayer, ConfigStack};

/// 解析打包的默认配置（base 层）。
///
/// 解析失败只可能是**编译期就存在的常量 YAML 有语法错误** —— 那是构建错误，
/// 不该留到运行时。因此这里用 `expect` 并 panic：它永远不该在生产触发。
pub fn parse_base_config() -> serde_json::Value {
    loader::parse_yaml(DEFAULT_CONFIG, std::path::Path::new("<default.yaml>"))
        .expect("打包的默认配置必须是合法 YAML —— 构建期即应发现")
}
