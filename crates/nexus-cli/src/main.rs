//! `nexus` —— 单一二进制多子命令。
//!
//! 对应 Go 参考实现的 7 个 `cmd/*`：
//!
//! | Go cmd      | 子命令              | 依赖                |
//! |-------------|---------------------|---------------------|
//! | `server`    | `nexus serve`       | PG/Redis/Milvus/S3  |
//! | `worker`    | `nexus worker`      | 按 `--queues` 决定  |
//! | `docparser` | `nexus docparser`   | 无（本地子进程）    |
//! | `cli`       | `nexus cli`         | API                 |
//! | `migrate`   | `nexus migrate`     | PG                  |
//! | `loadtest`  | `nexus loadtest`    | API                 |
//! | `agenteval` | `nexus agenteval`   | API + 模型          |
//!
//! 关键约束：**合成一个二进制不改变部署拓扑**。worker 仍可独立部署、独立扩容。
//! 参见架构文档 §1.1。

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use nexus_config::mapping;

/// 队列选择。Go 版用 `-queues execution,doc-etl,memory`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Queue {
    /// 持久执行（仅需 PostgreSQL）
    Execution,
    /// 文档 ETL（PG + Redis + Milvus + S3）
    DocEtl,
    /// 长期记忆抽取（PG + Milvus）
    Memory,
}

#[derive(Parser, Debug)]
#[command(name = "nexus", version, about = "NexusAgent —— Rust 版")]
struct Cli {
    /// 配置文件目录（默认 ./configs）
    #[arg(long, global = true, env = "NX_CONFIG_DIR")]
    config_dir: Option<String>,

    /// 日志级别：trace/debug/info/warn/error
    #[arg(long, global = true, env = "NX_LOG_LEVEL", default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 启动 API 服务（REST + SSE + A2A 入站）
    Serve,
    /// 启动后台 Worker
    Worker {
        /// 要消费的队列，可重复指定
        #[arg(long = "queues", value_enum)]
        queues: Vec<Queue>,
    },
    /// 启动 PDF 解析服务（必须独立容器部署）
    Docparser,
    /// 执行数据库迁移
    Migrate,
    /// 命令行客户端
    Cli,
    /// 压测
    Loadtest {
        /// peer / stage / prepare / bootstrap
        #[arg(long, default_value = "stage")]
        mode: String,
    },
    /// Agent 评估
    Agenteval,
    /// 配置工具（打印映射表 / 生效配置 / 来源追踪）
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// 打印声明式环境变量映射表（可枚举的事实）
    Env,
    /// 打印当前生效的配置（base + file + env 合并后）
    Show,
    /// 查询某个配置路径的来源层
    Origin {
        /// 配置路径，如 `log.level`
        path: String,
    },
}

/// 打印声明式环境变量映射表。
///
/// 架构文档 §2.7 —— 「哪些环境变量能覆盖什么」必须**可枚举**。
/// 这条命令把映射表变成运行时的**可观测输出**，而不只是源码里的一个清单。
fn print_env_mapping() {
    let table = mapping::table();

    println!("# 环境变量映射表（{} 条）", table.len());
    println!("# 格式：环境变量 → 配置路径（类型）");
    println!();

    for mapping in table.mappings() {
        let secret = if mapping.secret { " [敏感]" } else { "" };
        println!(
            "{:<44} → {}{} ({})",
            mapping.env_var(),
            mapping.config_path,
            secret,
            mapping.value_type.expected_name()
        );
    }
}

/// 加载配置栈：base（打包默认）→ file（configs/*.yaml）→ env（`NX_*`）。
fn load_stack(config_dir: &Option<String>) -> Result<nexus_config::ConfigStack> {
    let mut stack = nexus_config::ConfigStack::new();

    stack.set_base(nexus_config::parse_base_config());

    let dir = config_dir
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(nexus_config::loader::DEFAULT_CONFIG_DIR));

    if dir.exists() {
        let file_layer = nexus_config::load_config_dir(&dir)?;
        stack.set_file(file_layer);
    }

    let report = nexus_config::apply_env(&mut stack, &mapping::table(), &nexus_config::ProcessEnv)?;

    // 未被映射表覆盖的 NX_* 变量 = 运维以为配了但其实没生效。
    if !report.unmapped.is_empty() {
        eprintln!(
            "警告：{} 个环境变量未被映射表覆盖（不生效）：{}",
            report.unmapped.len(),
            report.unmapped.join(", ")
        );
    }

    Ok(stack)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // TODO(Phase 0): 初始化 tracing（日志格式由 config.log.format 决定）
    tracing::info!("nexus starting: command = {:?}", cli.command);

    match cli.command {
        Command::Serve => {
            // TODO(Phase 5): 按架构文档 §1.2 的十三步顺序装配依赖并启动 axum
            anyhow::bail!("`nexus serve` 尚未实现（Phase 5）")
        }
        Command::Worker { queues } => {
            // TODO(Phase 9): 按 --queues 初始化对应的 QueueWorker
            anyhow::bail!("`nexus worker` 尚未实现（Phase 9）：queues = {queues:?}")
        }
        Command::Docparser => {
            anyhow::bail!("`nexus docparser` 尚未实现（Phase 8）")
        }
        Command::Migrate => {
            anyhow::bail!("`nexus migrate` 尚未实现（Phase 0）")
        }
        Command::Cli => {
            anyhow::bail!("`nexus cli` 尚未实现（Phase 5）")
        }
        Command::Loadtest { mode } => {
            anyhow::bail!("`nexus loadtest` 尚未实现（Phase 10）：mode = {mode}")
        }
        Command::Agenteval => {
            anyhow::bail!("`nexus agenteval` 尚未实现（Phase 10）")
        }
        Command::Config { action } => match action {
            ConfigAction::Env => print_env_mapping(),
            ConfigAction::Show => {
                let stack = load_stack(&cli.config_dir)?;
                println!("{}", serde_json::to_string_pretty(stack.merged())?);
            }
            ConfigAction::Origin { path } => {
                let stack = load_stack(&cli.config_dir)?;
                match stack.origin(&path) {
                    Some(layer) => {
                        println!("{path} ← {layer}");
                        if let Some(value) = stack.get(&path) {
                            println!("  值：{value}");
                        }
                    }
                    None => println!("{path} ← 未配置"),
                }
            }
        },
    }

    Ok(())
}
