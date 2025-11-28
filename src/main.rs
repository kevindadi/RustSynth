use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod analysis;
mod config;
mod cp_net;
mod generate;
mod ir_graph;
mod parse;
mod petri_net_traits;
mod pipeline;
mod pt_net;
mod support_types;

use crate::config::Config;
use crate::pipeline::Pipeline;

#[derive(Parser)]
#[command(name = "sypetype")]
#[command(about = "SyPetype: 从 Rust API 文档自动生成 Petri 网", long_about = None)]
#[command(version)]
struct Cli {
    /// rustdoc JSON 文件路径   
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// 目标 crate 名称, 用于 USE: crate::target_crate::...
    #[arg(short, long, value_name = "NAME")]
    target: String,

    /// 被测库的路径 (相对于输出目录的 fuzz 文件夹)
    #[arg(long, value_name = "PATH")]
    lib_path: Option<String>,

    /// 输出目录
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    output: PathBuf,

    /// 导出 IR Graph (DOT + JSON)
    #[arg(long)]
    ir_graph: bool,

    /// 导出 PT-Net (Place/Transition Net, DOT + JSON)
    #[arg(long)]
    petri_net: bool,

    /// 导出 CP-Net (Colored Petri Net with Trait Hub, DOT + JSON)
    #[arg(long)]
    cp_net: bool,

    /// 生成 Fuzz Target (默认开启，使用 --no-fuzz 禁用)
    #[arg(long, default_value = "true", action = clap::ArgAction::Set)]
    fuzz: bool,

    /// Fuzz target 名称
    #[arg(long, value_name = "NAME", default_value = "fuzz_target_1")]
    fuzz_name: String,

    #[arg(short, long)]
    quiet: bool,

    #[arg(long)]
    print_summary: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    let cfg = Config {
        input_json: cli.input,
        target_crate: cli.target,
        lib_path: cli.lib_path,
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
            export_cp_net_dot: cli.cp_net,
            cp_net_dot_name: "cp_net.dot".to_string(),
            export_cp_net_json: cli.cp_net,
            cp_net_json_name: "cp_net.json".to_string(),
            print_stats: !cli.quiet,
            print_type_summary: cli.print_summary,
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
            "  PT-Net 已导出到: {}",
            cfg.output.output_dir.join("petri_net.dot").display()
        );
    }
    if cli.cp_net {
        log::info!(
            "  CP-Net 已导出到: {}",
            cfg.output.output_dir.join("cp_net.dot").display()
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
