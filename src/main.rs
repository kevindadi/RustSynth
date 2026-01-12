//! SyPetype - Rust API 签名分析与 PCPN 构建工具
//!
//! 从 rustdoc JSON 提取 API 签名，构建二分 API Graph，
//! 并转换为下推着色 Petri 网 (PCPN)。

mod apigraph;
mod extract;
mod pcpn;
mod rustdoc_loader;
mod type_model;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "sypetype",
    about = "Rust API 签名分析与 PCPN 构建工具",
    long_about = "从 rustdoc JSON 提取 API 签名，构建二分 API Graph，\n\
                  并转换为下推着色 Petri 网 (PCPN)。"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 构建 API Graph（二分图：函数节点 + 类型节点）
    Apigraph {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出目录
        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        /// 仅探索指定模块 (可多次指定)
        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 构建 PCPN（下推着色 Petri 网）
    Pcpn {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出目录
        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        /// 仅探索指定模块 (可多次指定)
        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 同时生成 API Graph 和 PCPN
    All {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出目录
        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        /// 仅探索指定模块 (可多次指定)
        #[arg(long = "module")]
        modules: Vec<String>,
    },
}

fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Commands::Apigraph { input, out, modules } => {
            run_apigraph(&input, &out, &modules)?;
        }
        Commands::Pcpn { input, out, modules } => {
            run_pcpn(&input, &out, &modules)?;
        }
        Commands::All { input, out, modules } => {
            run_apigraph(&input, &out, &modules)?;
            run_pcpn(&input, &out, &modules)?;
        }
    }

    Ok(())
}

fn run_apigraph(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("加载 rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("加载 rustdoc JSON 失败")?;

    let crate_name = if let Some(root_item) = krate.index.get(&krate.root) {
        root_item.name.clone().unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    };
    tracing::info!(
        "解析 crate: {} (version: {})",
        crate_name,
        krate.crate_version.as_deref().unwrap_or("unknown")
    );

    tracing::info!("构建 API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;

    let stats = graph.stats();
    tracing::info!("{}", stats);

    // 输出 DOT
    let dot_path = out.join("apigraph.dot");
    std::fs::write(&dot_path, graph.to_dot()).context("写入 apigraph.dot 失败")?;
    tracing::info!("✓ API Graph DOT 已生成: {:?}", dot_path);

    // 输出 JSON
    let json_path = out.join("apigraph.json");
    let json = serde_json::to_string_pretty(&graph).context("序列化 API Graph 失败")?;
    std::fs::write(&json_path, json).context("写入 apigraph.json 失败")?;
    tracing::info!("✓ API Graph JSON 已生成: {:?}", json_path);

    tracing::info!("  使用 'dot -Tpng {} -o apigraph.png' 生成图片", dot_path.display());

    Ok(())
}

fn run_pcpn(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("加载 rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("加载 rustdoc JSON 失败")?;

    let crate_name = if let Some(root_item) = krate.index.get(&krate.root) {
        root_item.name.clone().unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    };
    tracing::info!(
        "解析 crate: {} (version: {})",
        crate_name,
        krate.crate_version.as_deref().unwrap_or("unknown")
    );

    tracing::info!("构建 API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    tracing::info!("转换为 PCPN...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);

    let stats = pcpn.stats();
    tracing::info!("{}", stats);

    // 输出 DOT
    let dot_path = out.join("pcpn.dot");
    std::fs::write(&dot_path, pcpn.to_dot()).context("写入 pcpn.dot 失败")?;
    tracing::info!("✓ PCPN DOT 已生成: {:?}", dot_path);

    // 输出 JSON
    let json_path = out.join("pcpn.json");
    let json = serde_json::to_string_pretty(&pcpn).context("序列化 PCPN 失败")?;
    std::fs::write(&json_path, json).context("写入 pcpn.json 失败")?;
    tracing::info!("✓ PCPN JSON 已生成: {:?}", json_path);

    tracing::info!("  使用 'dot -Tpng {} -o pcpn.png' 生成图片", dot_path.display());

    Ok(())
}
