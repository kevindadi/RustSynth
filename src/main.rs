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
// mod emitter; // 简化版不生成 Rust 代码
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
        #[arg(long, default_value = "10")]
        max_tokens: usize,

        /// 最大栈深度
        #[arg(long, default_value = "100")]
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

    /// 生成可达图（状态空间图）
    Reachability {
        /// Rustdoc JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出目录
        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        /// 最大状态数
        #[arg(long, default_value = "100")]
        max_states: usize,

        /// 每个 place 的最大 token 数
        #[arg(long, default_value = "2")]
        max_tokens: usize,

        /// 最大栈深度
        #[arg(long, default_value = "3")]
        max_stack: usize,

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
        Commands::Reachability {
            input,
            out,
            max_states,
            max_tokens,
            max_stack,
            modules,
        } => {
            run_reachability(&input, &out, max_states, max_tokens, max_stack, &modules)?;
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
    _max_tokens: usize, // 简化版使用 budget
    _max_stack: usize,  // 简化版不使用栈深度
    max_steps: usize,
    min_steps: usize,
    strategy: &str,
    modules: &[String],
) -> Result<()> {
    let (pcpn, _) = build_pcpn(input, modules)?;

    // 配置仿真器（简化版）
    let config = simulator::SimConfig {
        dup_limit: 2,
        max_steps,
        min_steps,
        strategy: parse_strategy(strategy),
    };

    tracing::info!(
        "运行仿真器 (策略={:?}, dup_limit={}, max_steps={})",
        config.strategy,
        config.dup_limit,
        config.max_steps
    );

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ 找到 witness (探索 {} 个状态)", result.states_explored);
        simulator::print_trace(&result.trace);
    } else {
        tracing::warn!("✗ 未找到 witness (探索 {} 个状态)", result.states_explored);
    }

    Ok(())
}

fn run_generate(
    input: &PathBuf,
    out: &PathBuf,
    _max_tokens: usize, // 简化版使用 budget
    _max_stack: usize,  // 简化版不使用栈深度
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

    // 配置仿真器（简化版）
    let config = simulator::SimConfig {
        dup_limit: 2,
        max_steps,
        min_steps,
        strategy: parse_strategy(strategy),
    };

    tracing::info!(
        "运行仿真器 (策略={:?}, dup_limit={}, max_steps={})",
        config.strategy,
        config.dup_limit,
        config.max_steps
    );

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ 找到 witness (探索 {} 个状态)", result.states_explored);
        simulator::print_trace(&result.trace);

        // 保存抽象 trace 到文件
        let trace_path = out.join("trace.txt");
        let trace_content = result
            .trace
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{}. {}", i + 1, f))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&trace_path, &trace_content).context("写入 trace.txt 失败")?;
        tracing::info!("✓ 抽象 Trace 已生成: {:?}", trace_path);
    } else {
        tracing::warn!("✗ 未找到 witness (探索 {} 个状态)", result.states_explored);
        tracing::info!("尝试增加 --max-steps 参数");
    }

    // 输出 API Graph（用于参考）
    let graph_dot_path = out.join("apigraph.dot");
    std::fs::write(&graph_dot_path, graph.to_dot()).context("写入 apigraph.dot 失败")?;
    tracing::info!("✓ API Graph DOT 已生成: {:?}", graph_dot_path);

    Ok(())
}

/// 生成可达图
fn run_reachability(
    input: &PathBuf,
    out: &PathBuf,
    max_states: usize,
    _max_tokens: usize, // 简化版使用 budget
    _max_stack: usize,  // 简化版不使用栈深度
    modules: &[String],
) -> Result<()> {
    let (pcpn, _graph) = build_pcpn(input, modules)?;

    // 确保输出目录存在
    std::fs::create_dir_all(out).context("创建输出目录失败")?;

    // 配置仿真器（简化版）
    let config = simulator::SimConfig {
        dup_limit: 2,
        max_steps: max_states * 2,
        min_steps: 0,
        strategy: simulator::SearchStrategy::Bfs,
    };

    let sim = simulator::Simulator::new(&pcpn, config);
    tracing::info!(
        "生成可达图 (max_states={}, dup_limit={})",
        max_states,
        config.dup_limit
    );

    let reachability = sim.generate_reachability_graph(max_states);

    // 输出统计信息
    tracing::info!("{}", reachability.stats());

    // 输出 DOT 文件
    let dot_path = out.join("reachability.dot");
    std::fs::write(&dot_path, reachability.to_dot(&pcpn)).context("写入 reachability.dot 失败")?;
    tracing::info!("✓ 可达图 DOT 已生成: {:?}", dot_path);

    // 打印部分信息
    println!("\n=== 可达图统计 ===");
    println!("状态数: {}", reachability.states.len());
    println!("边数: {}", reachability.edges.len());

    // 打印前几条边
    println!("\n=== 部分转移 (前 20 条) ===");
    for (i, (from, to, label)) in reachability.edges.iter().take(20).enumerate() {
        println!("  {}. s{} --[{}]--> s{}", i + 1, from, label, to);
    }
    if reachability.edges.len() > 20 {
        println!("  ... (还有 {} 条)", reachability.edges.len() - 20);
    }

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
