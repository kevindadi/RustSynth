use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

// 临时注释掉依赖模块，只输出解析内容
// mod analysis;
// mod config;
// mod generate;
mod ir_graph;
mod parse;
mod petri_net_traits;
// mod pipeline;
// mod pt_net;
mod support_types;

use crate::ir_graph::builder::IrGraphBuilder;
use crate::parse::ParsedCrate;

#[derive(Parser)]
#[command(name = "sypetype")]
#[command(about = "SyPetype: 解析 Rust API 文档并生成 IR Graph", long_about = None)]
#[command(version)]
struct Cli {
    /// rustdoc JSON 文件路径   
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// 输出目录（默认为 ./graph）
    #[arg(short, long, value_name = "DIR", default_value = "graph")]
    output_dir: PathBuf,

    /// 打印统计信息
    #[arg(long)]
    stats: bool,

    /// 不导出文件，仅打印统计
    #[arg(long)]
    no_export: bool,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    // 从 JSON 文件解析
    log::info!("正在解析 rustdoc JSON: {}", cli.input.display());
    let parsed_crate = ParsedCrate::from_json_file(&cli.input)?;

    if cli.stats {
        log::info!("=== ParsedCrate 统计 ===");
        parsed_crate.print_stats();
    }

    // 构建 IR Graph
    log::info!("正在构建 IR Graph...");
    let builder = IrGraphBuilder::new(&parsed_crate);
    let ir_graph = builder.build();

    if cli.stats {
        println!();
        ir_graph.print_stats();
    }

    // 导出
    if !cli.no_export {
        // 创建输出目录
        std::fs::create_dir_all(&cli.output_dir)?;

        let dot_path = cli.output_dir.join("ir_graph.dot");
        let json_path = cli.output_dir.join("ir_graph.json");
        let debug_path = cli.output_dir.join("ir_graph_debug.txt");

        log::info!("正在导出到目录: {}", cli.output_dir.display());

        ir_graph.export_dot(&parsed_crate, &dot_path)?;
        log::info!("  ✓ DOT: {}", dot_path.display());

        ir_graph.export_json(&json_path)?;
        log::info!("  ✓ JSON: {}", json_path.display());

        log::info!("✓ 完成!");
    }

    Ok(())
}
