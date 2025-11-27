use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod analysis;
mod config;
mod generate;
mod ir_graph;
mod parse;
mod pipeline;
mod pt_net;
mod support_types;

use crate::config::Config;
use crate::pipeline::Pipeline;

/// SyPetype: 从 Rust API 文档自动生成 Fuzz Target
#[derive(Parser)]
#[command(name = "sypetype")]
#[command(about = "从 Rust API 文档自动生成 Fuzz Target", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 运行完整的工作流（从 JSON 到 Fuzz Target）
    Run {
        /// 配置文件路径（TOML 或 JSON 格式）
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// rustdoc JSON 文件路径（覆盖配置文件）
        #[arg(short, long, value_name = "FILE")]
        input: Option<PathBuf>,

        /// 目标 crate 名称（覆盖配置文件）
        #[arg(short, long, value_name = "NAME")]
        target: Option<String>,

        /// 输出目录（覆盖配置文件）
        #[arg(short, long, value_name = "DIR")]
        output: Option<PathBuf>,

        /// 导出 IR Graph DOT 文件
        #[arg(long)]
        export_ir_dot: bool,

        /// 导出 IR Graph JSON 文件
        #[arg(long)]
        export_ir_json: bool,

        /// 导出 Petri Net DOT 文件
        #[arg(long)]
        export_petri_dot: bool,

        /// 导出 Petri Net JSON 文件
        #[arg(long)]
        export_petri_json: bool,

        /// 静默模式（不打印统计信息）
        #[arg(short, long)]
        quiet: bool,
    },

    /// 生成示例配置文件
    GenConfig {
        /// 输出配置文件路径
        #[arg(short, long, value_name = "FILE", default_value = "sypetype.toml")]
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    // 初始化日志
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            config,
            input,
            target,
            output,
            export_ir_dot,
            export_ir_json,
            export_petri_dot,
            export_petri_json,
            quiet,
        } => {
            // 加载配置
            let mut cfg = if let Some(config_path) = config {
                load_config(&config_path)?
            } else {
                log::info!("未指定配置文件，使用默认配置");
                Config::default()
            };

            // 命令行参数覆盖配置文件
            if let Some(input_path) = input {
                cfg.input_json = input_path;
            }
            if let Some(target_name) = target {
                cfg.target_crate = target_name;
            }
            if let Some(output_dir) = output {
                cfg.output.output_dir = output_dir;
            }
            if export_ir_dot {
                cfg.export.export_ir_graph_dot = true;
            }
            if export_ir_json {
                cfg.export.export_ir_graph_json = true;
            }
            if export_petri_dot {
                cfg.export.export_petri_net_dot = true;
            }
            if export_petri_json {
                cfg.export.export_petri_net_json = true;
            }
            if quiet {
                cfg.export.print_stats = false;
            }

            // 运行工作流
            let pipeline = Pipeline::new(cfg);
            pipeline.run()?;

            log::info!("✓ 工作流执行成功！");
        }

        Commands::GenConfig { output } => {
            Config::create_example_config(&output)?;
            log::info!("✓ 示例配置文件已生成: {}", output.display());
            log::info!(
                "  请编辑配置文件，然后运行: sypetype run --config {}",
                output.display()
            );
        }
    }

    Ok(())
}

/// 加载配置文件（自动识别 TOML 或 JSON 格式）
fn load_config(path: &PathBuf) -> Result<Config> {
    log::info!("从配置文件加载: {}", path.display());

    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    match extension {
        "toml" => Config::from_toml_file(path),
        "json" => Config::from_json_file(path),
        _ => {
            // 尝试自动检测格式
            if let Ok(config) = Config::from_toml_file(path) {
                Ok(config)
            } else {
                Config::from_json_file(path)
            }
        }
    }
}
