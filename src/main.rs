//! SyPetype - Rust API 签名分析与 PCPN 构建工具
//!
//! 从 rustdoc JSON 提取 API 签名，构建二分 API Graph，
//! 转换为下推着色 Petri 网 (PCPN)，并生成可编译的 Rust 代码。
//!
//! ## 工具流水线
//! 1. 输入: `cargo doc` 生成的 rustdoc JSON
//! 2. API-Graph 构建（单态化后的二分图）
//! 3. API-Graph → PCPN 转换
//! 4. PCPN 有界 simulator（搜索 firing 序列）
//! 5. 从 firing 序列生成可编译 Rust 代码

mod apigraph;
mod emitter;
mod extract;
mod pcpn;
mod rustdoc_loader;
mod simulator;
mod type_model;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "sypetype",
    about = "Rust API 签名分析与 PCPN 构建工具",
    long_about = "从 rustdoc JSON 提取 API 签名，构建二分 API Graph,\n\
                  转换为下推着色 Petri 网 (PCPN)，并生成可编译的 Rust 代码。\n\n\
                  工具流水线:\n\
                  1. rustdoc JSON → API Graph（二分图）\n\
                  2. API Graph → PCPN（含 Own/Frz/Blk 库所）\n\
                  3. PCPN Simulator（有界搜索）\n\
                  4. Firing 序列 → 可编译 Rust 代码"
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

    /// 运行 PCPN 仿真器，搜索 witness 轨迹
    Simulate {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 每个 place 的最大 token 数
        #[arg(long, default_value = "3")]
        max_tokens: usize,

        /// 最大栈深度
        #[arg(long, default_value = "5")]
        max_stack: usize,

        /// 最大步数
        #[arg(long, default_value = "50")]
        max_steps: usize,

        /// 最小步数（目标条件）
        #[arg(long, default_value = "3")]
        min_steps: usize,

        /// 搜索策略 (bfs/dfs)
        #[arg(long, default_value = "bfs")]
        strategy: String,

        /// 仅探索指定模块 (可多次指定)
        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 运行完整流水线：生成 PCPN → 仿真 → 输出 Rust 代码
    Generate {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出目录
        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        /// 每个 place 的最大 token 数
        #[arg(long, default_value = "3")]
        max_tokens: usize,

        /// 最大栈深度
        #[arg(long, default_value = "5")]
        max_stack: usize,

        /// 最大步数
        #[arg(long, default_value = "50")]
        max_steps: usize,

        /// 最小步数（目标条件）
        #[arg(long, default_value = "5")]
        min_steps: usize,

        /// 搜索策略 (bfs/dfs)
        #[arg(long, default_value = "bfs")]
        strategy: String,

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
        Commands::Apigraph {
            input,
            out,
            modules,
        } => {
            run_apigraph(&input, &out, &modules)?;
        }
        Commands::Pcpn {
            input,
            out,
            modules,
        } => {
            run_pcpn(&input, &out, &modules)?;
        }
        Commands::All {
            input,
            out,
            modules,
        } => {
            run_apigraph(&input, &out, &modules)?;
            run_pcpn(&input, &out, &modules)?;
        }
        Commands::Simulate {
            input,
            max_tokens,
            max_stack,
            max_steps,
            min_steps,
            strategy,
            modules,
        } => {
            run_simulate(
                &input, max_tokens, max_stack, max_steps, min_steps, &strategy, &modules,
            )?;
        }
        Commands::Generate {
            input,
            out,
            max_tokens,
            max_stack,
            max_steps,
            min_steps,
            strategy,
            modules,
        } => {
            run_generate(
                &input, &out, max_tokens, max_stack, max_steps, min_steps, &strategy, &modules,
            )?;
        }
    }

    Ok(())
}

fn run_apigraph(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("加载 rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("加载 rustdoc JSON 失败")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!(
        "解析 crate: {} (version: {})",
        crate_name,
        krate.crate_version.as_deref().unwrap_or("unknown")
    );

    tracing::info!("构建 API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;

    let stats = graph.stats();
    tracing::info!("{}", stats);

    // 确保输出目录存在
    std::fs::create_dir_all(out).context("创建输出目录失败")?;

    // 输出 DOT
    let dot_path = out.join("apigraph.dot");
    std::fs::write(&dot_path, graph.to_dot()).context("写入 apigraph.dot 失败")?;
    tracing::info!("✓ API Graph DOT 已生成: {:?}", dot_path);

    // 输出 JSON
    let json_path = out.join("apigraph.json");
    let json = serde_json::to_string_pretty(&graph).context("序列化 API Graph 失败")?;
    std::fs::write(&json_path, json).context("写入 apigraph.json 失败")?;
    tracing::info!("✓ API Graph JSON 已生成: {:?}", json_path);

    tracing::info!(
        "  使用 'dot -Tpng {} -o apigraph.png' 生成图片",
        dot_path.display()
    );

    Ok(())
}

fn run_pcpn(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("加载 rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("加载 rustdoc JSON 失败")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!(
        "解析 crate: {} (version: {})",
        crate_name,
        krate.crate_version.as_deref().unwrap_or("unknown")
    );

    tracing::info!("构建 API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    tracing::info!("转换为 PCPN (Own/Frz/Blk model)...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);

    let stats = pcpn.stats();
    tracing::info!("{}", stats);

    // 确保输出目录存在
    std::fs::create_dir_all(out).context("创建输出目录失败")?;

    // 输出 DOT
    let dot_path = out.join("pcpn.dot");
    std::fs::write(&dot_path, pcpn.to_dot()).context("写入 pcpn.dot 失败")?;
    tracing::info!("✓ PCPN DOT 已生成: {:?}", dot_path);

    // 输出 JSON
    let json_path = out.join("pcpn.json");
    let json = serde_json::to_string_pretty(&pcpn).context("序列化 PCPN 失败")?;
    std::fs::write(&json_path, json).context("写入 pcpn.json 失败")?;
    tracing::info!("✓ PCPN JSON 已生成: {:?}", json_path);

    tracing::info!(
        "  使用 'dot -Tpng {} -o pcpn.png' 生成图片",
        dot_path.display()
    );

    Ok(())
}

fn run_simulate(
    input: &PathBuf,
    max_tokens: usize,
    max_stack: usize,
    max_steps: usize,
    min_steps: usize,
    strategy: &str,
    modules: &[String],
) -> Result<()> {
    let (pcpn, _) = build_pcpn(input, modules)?;

    // 配置仿真器
    let config = simulator::SimConfig {
        max_tokens_per_place: max_tokens,
        max_stack_depth: max_stack,
        max_steps,
        min_steps,
        strategy: parse_strategy(strategy),
        target_place: None,
    };

    tracing::info!(
        "运行仿真器 (策略={:?}, max_tokens={}, max_stack={}, max_steps={})",
        config.strategy,
        config.max_tokens_per_place,
        config.max_stack_depth,
        config.max_steps
    );

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ 找到 witness (探索 {} 个状态)", result.states_explored);
        simulator::print_trace(&result.trace);
        simulator::print_final_state(&result.final_state, &pcpn);
    } else {
        tracing::warn!("✗ 未找到 witness (探索 {} 个状态)", result.states_explored);
    }

    Ok(())
}

fn run_generate(
    input: &PathBuf,
    out: &PathBuf,
    max_tokens: usize,
    max_stack: usize,
    max_steps: usize,
    min_steps: usize,
    strategy: &str,
    modules: &[String],
) -> Result<()> {
    let (pcpn, graph) = build_pcpn(input, modules)?;

    // 确保输出目录存在
    std::fs::create_dir_all(out).context("创建输出目录失败")?;

    // 输出 PCPN DOT
    let pcpn_dot_path = out.join("pcpn.dot");
    std::fs::write(&pcpn_dot_path, pcpn.to_dot()).context("写入 pcpn.dot 失败")?;
    tracing::info!("✓ PCPN DOT 已生成: {:?}", pcpn_dot_path);

    // 配置仿真器
    let config = simulator::SimConfig {
        max_tokens_per_place: max_tokens,
        max_stack_depth: max_stack,
        max_steps,
        min_steps,
        strategy: parse_strategy(strategy),
        target_place: None,
    };

    tracing::info!(
        "运行仿真器 (策略={:?}, max_tokens={}, max_stack={}, max_steps={})",
        config.strategy,
        config.max_tokens_per_place,
        config.max_stack_depth,
        config.max_steps
    );

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ 找到 witness (探索 {} 个状态)", result.states_explored);
        simulator::print_trace(&result.trace);

        // 生成 Rust 代码
        let rust_code = emitter::emit_rust_code(&result.trace);
        let code_path = out.join("generated.rs");
        std::fs::write(&code_path, &rust_code).context("写入 generated.rs 失败")?;
        tracing::info!("✓ Rust 代码已生成: {:?}", code_path);

        // 也打印到控制台
        println!("\n=== Generated Rust Code ===");
        println!("{}", rust_code);
    } else {
        tracing::warn!("✗ 未找到 witness (探索 {} 个状态)", result.states_explored);
        tracing::info!("尝试增加 --max-steps 或 --max-tokens 参数");
    }

    // 输出 API Graph（用于参考）
    let graph_dot_path = out.join("apigraph.dot");
    std::fs::write(&graph_dot_path, graph.to_dot()).context("写入 apigraph.dot 失败")?;
    tracing::info!("✓ API Graph DOT 已生成: {:?}", graph_dot_path);

    Ok(())
}

/// 构建 PCPN
fn build_pcpn(input: &PathBuf, modules: &[String]) -> Result<(pcpn::Pcpn, apigraph::ApiGraph)> {
    tracing::info!("加载 rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("加载 rustdoc JSON 失败")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!(
        "解析 crate: {} (version: {})",
        crate_name,
        krate.crate_version.as_deref().unwrap_or("unknown")
    );

    tracing::info!("构建 API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    tracing::info!("转换为 PCPN (Own/Frz/Blk model)...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);
    tracing::info!("{}", pcpn.stats());

    Ok((pcpn, graph))
}

/// 获取 crate 名称
fn get_crate_name(krate: &rustdoc_types::Crate) -> String {
    if let Some(root_item) = krate.index.get(&krate.root) {
        root_item
            .name
            .clone()
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    }
}

/// 解析搜索策略
fn parse_strategy(s: &str) -> simulator::SearchStrategy {
    match s.to_lowercase().as_str() {
        "dfs" => simulator::SearchStrategy::Dfs,
        _ => simulator::SearchStrategy::Bfs,
    }
}
