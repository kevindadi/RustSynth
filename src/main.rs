use anyhow::Result;
use clap::Parser;
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

#[derive(Parser)]
#[command(name = "sypetype")]
#[command(about = "SyPetype: 从 Rust API 文档自动生成 Fuzz Target", long_about = None)]
#[command(version)]
struct Cli {
    /// rustdoc JSON 文件路径   
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// 目标 crate 名称
    #[arg(short, long, value_name = "NAME")]
    target: String,

    /// 输出目录
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    output: PathBuf,

    /// 导出 IR Graph (DOT 和 JSON 格式)
    #[arg(long)]
    ir_graph: bool,

    /// 导出 Petri Net (DOT 和 JSON 格式)
    #[arg(long)]
    petri_net: bool,

    /// 生成 Fuzz Target (默认开启，使用 --no-fuzz 禁用)
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    fuzz: bool,

    /// Fuzz target 名称
    #[arg(long, value_name = "NAME", default_value = "fuzz_target_1")]
    fuzz_name: String,

    /// 静默模式（不打印统计信息）
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    let cfg = Config {
        input_json: cli.input,
        target_crate: cli.target,
        output: config::OutputConfig {
            output_dir: cli.output,
            fuzz_dir: PathBuf::from("fuzz"),
            fuzz_target_name: cli.fuzz_name,
        },
        export: config::ExportConfig {
            export_ir_graph_dot: cli.ir_graph,
            ir_graph_dot_name: "ir_graph.dot".to_string(),
            export_ir_graph_json: cli.ir_graph,
            ir_graph_json_name: "ir_graph.json".to_string(),
            export_petri_net_dot: cli.petri_net,
            petri_net_dot_name: "petri_net.dot".to_string(),
            export_petri_net_json: cli.petri_net,
            petri_net_json_name: "petri_net.json".to_string(),
            print_stats: !cli.quiet,
        },
    };

    if !cli.fuzz {
        log::info!("已禁用 Fuzz Target 生成");
    }

    let pipeline = Pipeline::new(cfg.clone());
    pipeline.run()?;

    log::info!("✓ 完成！");
    if cli.ir_graph {
        log::info!(
            "  IR Graph 已导出到: {}",
            cfg.output.output_dir.join("ir_graph.dot").display()
        );
    }
    if cli.petri_net {
        log::info!(
            "  Petri Net 已导出到: {}",
            cfg.output.output_dir.join("petri_net.dot").display()
        );
    }
    if cli.fuzz {
        log::info!(
            "  Fuzz Target 已生成到: {}",
            cfg.fuzz_targets_dir().display()
        );
        log::info!(
            "  运行: cd {} && cargo fuzz run {}",
            cfg.fuzz_dir_path().display(),
            cfg.output.fuzz_target_name
        );
    }

    Ok(())
}
