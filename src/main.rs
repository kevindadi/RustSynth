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
mod label_pt_net;
mod support_types;

use crate::ir_graph::builder::IrGraphBuilder;
use crate::label_pt_net::{ExportFormat, convert_ir_to_lpn};
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

    log::info!("正在解析 rustdoc JSON: {}", cli.input.display());
    let parsed_crate = ParsedCrate::from_json_file(&cli.input)?;

    if cli.stats {
        log::info!("=== ParsedCrate 统计 ===");
        parsed_crate.print_stats();
    }

    log::info!("正在构建 IR Graph...");
    let builder = IrGraphBuilder::new(&parsed_crate);
    let ir_graph = builder.build();

    if cli.stats {
        println!();
        ir_graph.print_stats();
    }

    // 转换为 Labeled Petri Net
    log::info!("正在转换为 Labeled Petri Net...");
    let lpn = convert_ir_to_lpn(&ir_graph);
    let lpn_stats = lpn.stats();
    log::info!(
        "  Places: {}, Transitions: {}, Arcs: {} (input: {}, output: {}), Initial tokens: {}",
        lpn_stats.place_count,
        lpn_stats.transition_count,
        lpn_stats.input_arc_count + lpn_stats.output_arc_count,
        lpn_stats.input_arc_count,
        lpn_stats.output_arc_count,
        lpn_stats.total_initial_tokens
    );

    if !cli.no_export {
        // 创建输出目录
        std::fs::create_dir_all(&cli.output_dir)?;

        let dot_path = cli.output_dir.join("ir_graph.dot");
        let json_path = cli.output_dir.join("ir_graph.json");

        log::info!("正在导出到目录: {}", cli.output_dir.display());

        ir_graph.export_dot(&parsed_crate, &dot_path)?;
        log::info!("  ✓ IR Graph DOT: {}", dot_path.display());

        ir_graph.export_json(&json_path)?;
        log::info!("  ✓ IR Graph JSON: {}", json_path.display());

        // 导出 Petri Net
        let lpn_dot_path = cli.output_dir.join("petri_net.dot");
        let lpn_pnml_path = cli.output_dir.join("petri_net.pnml");
        let lpn_json_path = cli.output_dir.join("petri_net.json");

        lpn.save_to_file(&lpn_dot_path, ExportFormat::Dot)?;
        log::info!("  ✓ Petri Net DOT: {}", lpn_dot_path.display());

        lpn.save_to_file(&lpn_pnml_path, ExportFormat::Pnml)?;
        log::info!("  ✓ Petri Net PNML: {}", lpn_pnml_path.display());

        lpn.save_to_file(&lpn_json_path, ExportFormat::Json)?;
        log::info!("  ✓ Petri Net JSON: {}", lpn_json_path.display());

        log::info!("✓ 完成!");
    }

    Ok(())
}
