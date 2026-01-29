//! SyPetype - Pushdown CPN Safe Rust Synthesizer
//!
//! 从 rustdoc JSON 提取 API 签名，构建 Pushdown Colored Petri Net，
//! 通过有界可达性搜索生成可编译的 Safe Rust 代码片段。

mod apigraph;
mod config;
mod emitter;
mod extract;
mod lifetime_analyzer;
mod pcpn;
mod rustdoc_loader;
mod simulator;
mod type_model;
mod types;
mod unify;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "sypetype",
    about = "Pushdown CPN Safe Rust Synthesizer",
    long_about = "从 rustdoc JSON 提取 API 签名，构建 Pushdown Colored Petri Net，\n\
                  通过有界可达性搜索生成可编译的 Safe Rust 代码片段。\n\n\
                  Extracts API signatures from rustdoc JSON, builds a Pushdown Colored Petri Net,\n\
                  and synthesizes compilable Safe Rust code snippets via bounded reachability."
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 运行合成器 / Run the synthesizer
    Synth {
        /// Rustdoc JSON 文件路径
        #[arg(long)]
        doc_json: PathBuf,

        /// 任务配置文件 (TOML)
        #[arg(long)]
        task: PathBuf,

        /// 输出文件路径
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// 构建 API Graph
    Apigraph {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 构建 PCPN
    Pcpn {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 同时生成 API Graph 和 PCPN
    All {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 运行仿真器
    Simulate {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(long, default_value = "10")]
        max_tokens: usize,

        #[arg(long, default_value = "100")]
        max_stack: usize,

        #[arg(long, default_value = "50")]
        max_steps: usize,

        #[arg(long, default_value = "3")]
        min_steps: usize,

        #[arg(long, default_value = "bfs")]
        strategy: String,

        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 生成可达图
    Reachability {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        #[arg(long, default_value = "100")]
        max_states: usize,

        #[arg(long, default_value = "2")]
        max_tokens: usize,

        #[arg(long, default_value = "3")]
        max_stack: usize,

        #[arg(long = "module")]
        modules: Vec<String>,
    },

    /// 完整流水线：PCPN → 仿真 → 代码生成
    Generate {
        #[arg(short, long)]
        input: PathBuf,

        #[arg(short, long, default_value = ".")]
        out: PathBuf,

        #[arg(long, default_value = "3")]
        max_tokens: usize,

        #[arg(long, default_value = "5")]
        max_stack: usize,

        #[arg(long, default_value = "50")]
        max_steps: usize,

        #[arg(long, default_value = "5")]
        min_steps: usize,

        #[arg(long, default_value = "bfs")]
        strategy: String,

        #[arg(long = "module")]
        modules: Vec<String>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Commands::Synth {
            doc_json,
            task,
            out,
        } => {
            run_synth(&doc_json, &task, out.as_ref())?;
        }
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
            max_tokens: _,
            max_stack,
            max_steps,
            min_steps: _,
            strategy: _,
            modules,
        } => {
            run_simulate(&input, max_steps, max_stack, &modules)?;
        }
        Commands::Reachability {
            input,
            out,
            max_states,
            max_tokens: _,
            max_stack,
            modules,
        } => {
            run_reachability(&input, &out, max_states, max_stack, &modules)?;
        }
        Commands::Generate {
            input,
            out,
            max_tokens: _,
            max_stack,
            max_steps,
            min_steps: _,
            strategy: _,
            modules,
        } => {
            run_generate(&input, &out, max_steps, max_stack, &modules)?;
        }
    }

    Ok(())
}

fn run_synth(doc_json: &PathBuf, task_path: &PathBuf, out: Option<&PathBuf>) -> Result<()> {
    tracing::info!("Loading task configuration: {:?}", task_path);
    let task = config::TaskConfig::load(task_path).context("Failed to load task config")?;

    tracing::info!("Loading rustdoc JSON: {:?}", doc_json);
    let krate = rustdoc_loader::load_rustdoc_json(doc_json).context("Failed to load rustdoc JSON")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!("Crate: {} (version: {})", crate_name, krate.crate_version.as_deref().unwrap_or("unknown"));

    tracing::info!("Building API Graph...");
    let graph = extract::build_api_graph(&krate, &[])?;
    tracing::info!("{}", graph.stats());

    tracing::info!("Converting to PCPN (9-place model)...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);
    tracing::info!("{}", pcpn.stats());

    let sim_config = simulator::SimConfig::from_task_config(&task, &pcpn);
    tracing::info!(
        "Running simulator (max_steps={}, stack_depth={})...",
        sim_config.max_steps,
        sim_config.stack_depth
    );

    let sim = simulator::Simulator::new(&pcpn, sim_config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ Found witness ({} states explored)", result.states_explored);
        simulator::print_trace(&result.trace);

        let code = emitter::emit_rust_code(&result.trace, &pcpn);
        println!("\n=== Generated Rust Code ===\n");
        println!("{}", code);

        if let Some(out_path) = out {
            std::fs::write(out_path, &code).context("Failed to write output file")?;
            tracing::info!("✓ Code written to: {:?}", out_path);
        }
    } else {
        tracing::warn!("✗ No witness found ({} states explored)", result.states_explored);
        tracing::info!("Try increasing max_steps or adjusting bounds in task config");
    }

    Ok(())
}

fn run_apigraph(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("Loading rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("Failed to load rustdoc JSON")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!("Crate: {} (version: {})", crate_name, krate.crate_version.as_deref().unwrap_or("unknown"));

    tracing::info!("Building API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    std::fs::create_dir_all(out).context("Failed to create output directory")?;

    let dot_path = out.join("apigraph.dot");
    std::fs::write(&dot_path, graph.to_dot()).context("Failed to write apigraph.dot")?;
    tracing::info!("✓ API Graph DOT: {:?}", dot_path);

    let json_path = out.join("apigraph.json");
    let json = serde_json::to_string_pretty(&graph).context("Failed to serialize API Graph")?;
    std::fs::write(&json_path, json).context("Failed to write apigraph.json")?;
    tracing::info!("✓ API Graph JSON: {:?}", json_path);

    Ok(())
}

fn run_pcpn(input: &PathBuf, out: &PathBuf, modules: &[String]) -> Result<()> {
    tracing::info!("Loading rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("Failed to load rustdoc JSON")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!("Crate: {} (version: {})", crate_name, krate.crate_version.as_deref().unwrap_or("unknown"));

    tracing::info!("Building API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    tracing::info!("Converting to PCPN (9-place model)...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);
    tracing::info!("{}", pcpn.stats());

    std::fs::create_dir_all(out).context("Failed to create output directory")?;

    let dot_path = out.join("pcpn.dot");
    std::fs::write(&dot_path, pcpn.to_dot()).context("Failed to write pcpn.dot")?;
    tracing::info!("✓ PCPN DOT: {:?}", dot_path);

    let json_path = out.join("pcpn.json");
    let json = serde_json::to_string_pretty(&pcpn).context("Failed to serialize PCPN")?;
    std::fs::write(&json_path, json).context("Failed to write pcpn.json")?;
    tracing::info!("✓ PCPN JSON: {:?}", json_path);

    Ok(())
}

fn run_simulate(input: &PathBuf, max_steps: usize, max_stack: usize, modules: &[String]) -> Result<()> {
    let (pcpn, _) = build_pcpn(input, modules)?;

    let config = simulator::SimConfig {
        max_steps,
        stack_depth: max_stack,
        ..Default::default()
    };

    tracing::info!("Running simulator (max_steps={}, stack_depth={})...", max_steps, max_stack);

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ Found witness ({} states explored)", result.states_explored);
        simulator::print_trace(&result.trace);
    } else {
        tracing::warn!("✗ No witness found ({} states explored)", result.states_explored);
    }

    Ok(())
}

fn run_reachability(input: &PathBuf, out: &PathBuf, max_states: usize, max_stack: usize, modules: &[String]) -> Result<()> {
    let (pcpn, _) = build_pcpn(input, modules)?;

    std::fs::create_dir_all(out).context("Failed to create output directory")?;

    let config = simulator::SimConfig {
        max_steps: max_states * 2,
        stack_depth: max_stack,
        ..Default::default()
    };

    let sim = simulator::Simulator::new(&pcpn, config);
    tracing::info!("Generating reachability graph (max_states={})...", max_states);

    let reachability = sim.generate_reachability_graph(max_states);
    tracing::info!("{}", reachability.stats());

    let dot_path = out.join("reachability.dot");
    std::fs::write(&dot_path, reachability.to_dot(&pcpn)).context("Failed to write reachability.dot")?;
    tracing::info!("✓ Reachability DOT: {:?}", dot_path);

    println!("\n=== Reachability Stats ===");
    println!("States: {}", reachability.states.len());
    println!("Edges: {}", reachability.edges.len());

    Ok(())
}

fn run_generate(input: &PathBuf, out: &PathBuf, max_steps: usize, max_stack: usize, modules: &[String]) -> Result<()> {
    let (pcpn, _) = build_pcpn(input, modules)?;

    std::fs::create_dir_all(out).context("Failed to create output directory")?;

    let pcpn_dot_path = out.join("pcpn.dot");
    std::fs::write(&pcpn_dot_path, pcpn.to_dot()).context("Failed to write pcpn.dot")?;
    tracing::info!("✓ PCPN DOT: {:?}", pcpn_dot_path);

    let config = simulator::SimConfig {
        max_steps,
        stack_depth: max_stack,
        ..Default::default()
    };

    tracing::info!("Running simulator (max_steps={}, stack_depth={})...", max_steps, max_stack);

    let sim = simulator::Simulator::new(&pcpn, config);
    let result = sim.run();

    if result.found {
        tracing::info!("✓ Found witness ({} states explored)", result.states_explored);
        simulator::print_trace(&result.trace);

        let code = emitter::emit_rust_code(&result.trace, &pcpn);

        let code_path = out.join("generated.rs");
        std::fs::write(&code_path, &code).context("Failed to write generated.rs")?;
        tracing::info!("✓ Generated code: {:?}", code_path);

        println!("\n=== Generated Rust Code ===\n");
        println!("{}", code);
    } else {
        tracing::warn!("✗ No witness found ({} states explored)", result.states_explored);
        tracing::info!("Try increasing --max-steps");
    }

    Ok(())
}

fn build_pcpn(input: &PathBuf, modules: &[String]) -> Result<(pcpn::Pcpn, apigraph::ApiGraph)> {
    tracing::info!("Loading rustdoc JSON: {:?}", input);
    let krate = rustdoc_loader::load_rustdoc_json(input).context("Failed to load rustdoc JSON")?;

    let crate_name = get_crate_name(&krate);
    tracing::info!("Crate: {} (version: {})", crate_name, krate.crate_version.as_deref().unwrap_or("unknown"));

    tracing::info!("Building API Graph...");
    let graph = extract::build_api_graph(&krate, modules)?;
    tracing::info!("{}", graph.stats());

    tracing::info!("Converting to PCPN (9-place model)...");
    let pcpn = pcpn::Pcpn::from_api_graph(&graph);
    tracing::info!("{}", pcpn.stats());

    Ok((pcpn, graph))
}

fn get_crate_name(krate: &rustdoc_types::Crate) -> String {
    if let Some(root_item) = krate.index.get(&krate.root) {
        root_item.name.clone().unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    }
}
