use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

// mod config;
// mod generate;
mod ir_graph;
mod parse;
mod petri_net_traits;
// mod pipeline;
pub mod label_pt_net;
pub mod support_types;

use crate::ir_graph::builder::IrGraphBuilder;
use crate::label_pt_net::{ExportFormat, convert_ir_to_lpn};
use crate::parse::ParsedCrate;

#[derive(Parser)]
#[command(name = "sypetype")]
#[command(about = "SyPetype: 解析 Rust API 文档并生成 IR Graph 和 Petri Net（支持 cargo-fuzz）", long_about = None)]
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

    /// 生成 API 调用序列（用于 fuzz 测试）
    #[arg(long)]
    generate_sequences: bool,

    /// 序列最大深度（默认 5）
    #[arg(long, default_value = "5")]
    max_depth: usize,

    /// 添加基本类型 shims
    #[arg(long)]
    add_shims: bool,

    /// Fuzz 输入（十六进制字符串，用于测试）
    #[arg(long)]
    fuzz_input: Option<String>,
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
    let mut lpn = convert_ir_to_lpn(&ir_graph);

    // 添加基本类型 shims（如果启用）
    if cli.add_shims {
        log::info!("正在添加基本类型 shims...");
        lpn.add_primitive_shims(&ir_graph);
    }

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

    // 生成 API 调用序列
    if cli.generate_sequences {
        log::info!("正在生成 API 调用序列...");

        // 解析 fuzz 输入（如果提供）
        let fuzz_bytes: Vec<u8> = if let Some(hex_str) = &cli.fuzz_input {
            // 解析十六进制字符串
            hex_str
                .as_bytes()
                .chunks(2)
                .filter_map(|chunk| {
                    let s = std::str::from_utf8(chunk).ok()?;
                    u8::from_str_radix(s, 16).ok()
                })
                .collect()
        } else {
            // 使用默认种子
            vec![0x42, 0x13, 0x37, 0xDE, 0xAD, 0xBE, 0xEF]
        };

        let sequences = lpn.generate_api_sequences(cli.max_depth, &fuzz_bytes, &ir_graph);

        println!("\n=== 生成的 API 调用序列 ===");
        for (i, seq) in sequences.iter().enumerate() {
            println!("序列 {}: {:?}", i + 1, seq);
        }
        println!("\n共生成 {} 个序列", sequences.len());

        // 也可以生成详细序列
        let detailed_sequences =
            lpn.generate_api_sequences_detailed(cli.max_depth, &fuzz_bytes, &ir_graph);

        println!("\n=== 详细 API 调用序列 ===");
        for (i, seq) in detailed_sequences.iter().enumerate() {
            println!("序列 {}:", i + 1);
            println!("  变迁索引: {:?}", seq.transition_indices);
            println!("  API 调用: {:?}", seq.api_calls);
            println!(
                "  最终标记: {:?}",
                seq.final_marking
                    .iter()
                    .enumerate()
                    .filter(|&(_, t)| *t > 0)
                    .collect::<Vec<_>>()
            );
        }

        // 显示分析结果
        println!("\n=== Petri Net 分析 ===");
        let analysis = lpn.analyze(1000, 10);
        println!("  活性: {}", if analysis.live { "是" } else { "否" });
        println!("  10-有界性: {}", if analysis.bounded { "是" } else { "否" });

        // 显示 failure places
        let failure_places = lpn.get_failure_places();
        if !failure_places.is_empty() {
            println!("\n=== Failure Places ===");
            for idx in failure_places {
                println!("  [{}] {}", idx, lpn.places[idx]);
            }
        }
    }

    Ok(())
}
