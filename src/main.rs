mod api_extract;
mod canonicalize;
mod emit;
mod model;
mod rustdoc_loader;
mod search;
mod transition;
mod type_norm;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "sypetype",
    about = "签名层协议可达性分析与见证代码生成器",
    long_about = "基于 Colored Petri Net 的 Rust API 可达性搜索工具\n\
                  从 rustdoc JSON 中提取 API 签名，构建资源状态机，\n\
                  搜索可行调用轨迹并生成可编译的 Rust 代码片段"
)]
struct Args {
    /// Rustdoc JSON 文件路径 (由 cargo +nightly rustdoc -- -Z unstable-options --output-format json 生成)
    #[arg(short, long)]
    input: PathBuf,

    /// 输出代码片段路径 (可选, 默认输出到 stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// 最大搜索步数
    #[arg(long, default_value = "20")]
    max_steps: usize,

    /// 每种类型最大 token 数量
    #[arg(long, default_value = "5")]
    max_tokens_per_type: usize,

    /// 最大借用嵌套深度
    #[arg(long, default_value = "3")]
    max_borrow_depth: usize,

    /// 是否启用 LIFO 借用栈 (pushdown 模式)
    #[arg(long)]
    enable_loan_stack: bool,

    /// 仅探索指定模块 (可多次指定)
    #[arg(long = "module")]
    modules: Vec<String>,

    /// 目标类型 (尝试合成此类型的 owned token)
    #[arg(long)]
    target_type: Option<String>,

    /// 在临时 crate 中验证生成的代码
    #[arg(long)]
    verify: bool,

    /// 输出内部 trace
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();

    tracing::info!("加载 rustdoc JSON: {:?}", args.input);
    let krate = rustdoc_loader::load_rustdoc_json(&args.input)
        .context("加载 rustdoc JSON 失败")?;

    // 获取 crate 信息
    let crate_name = if let Some(root_item) = krate.index.get(&krate.root) {
        root_item.name.clone().unwrap_or_else(|| "unknown".to_string())
    } else {
        "unknown".to_string()
    };
    tracing::info!("解析 crate: {} (version: {})", crate_name, krate.crate_version.as_deref().unwrap_or("unknown"));

    // 构建类型归一化上下文
    tracing::info!("构建类型归一化映射...");
    let type_context = type_norm::TypeContext::from_crate(&krate)?;

    // 提取 API 签名
    tracing::info!("提取 API 签名...");
    let apis = api_extract::extract_apis(&krate, &type_context, &args.modules)?;
    tracing::info!("提取到 {} 个 API", apis.len());

    if apis.is_empty() {
        anyhow::bail!("未找到任何公开 API，请检查输入文件或模块过滤条件");
    }

    // 构建搜索配置
    let config = search::SearchConfig {
        max_steps: args.max_steps,
        max_tokens_per_type: args.max_tokens_per_type,
        max_borrow_depth: args.max_borrow_depth,
        enable_loan_stack: args.enable_loan_stack,
        target_type: args.target_type.clone(),
    };

    // 执行可达性搜索
    tracing::info!("开始可达性搜索 (max_steps={})...", args.max_steps);
    let result = search::search(&apis, &type_context, &config)?;

    match result {
        Some((_final_state, trace)) => {
            tracing::info!("✓ 找到可行轨迹 (共 {} 步)", trace.len());

            // 生成 Rust 代码
            let snippet = emit::emit_code(&trace, &type_context, args.verbose)?;

            // 输出代码
            if let Some(output_path) = &args.output {
                std::fs::write(output_path, &snippet)
                    .context(format!("写入输出文件失败: {:?}", output_path))?;
                tracing::info!("✓ 代码已写入: {:?}", output_path);
            } else {
                println!("\n{}", "=".repeat(60));
                println!("生成的代码片段:");
                println!("{}", "=".repeat(60));
                println!("{}", snippet);
                println!("{}", "=".repeat(60));
            }

            // 可选：验证生成的代码
            if args.verify {
                tracing::info!("验证生成的代码...");
                emit::verify_code(&snippet, &type_context.crate_name)?;
            }

            // 输出 trace
            if args.verbose {
                println!("\n{}", "=".repeat(60));
                println!("执行轨迹:");
                println!("{}", "=".repeat(60));
                for (i, step) in trace.iter().enumerate() {
                    println!("Step {}: {}", i, step.description());
                }
                println!("{}", "=".repeat(60));
            }
        }
        None => {
            tracing::warn!("✗ 未找到可行轨迹");
            tracing::info!(
                "提示: 尝试增加 --max-steps 或 --max-tokens-per-type，或检查 API 签名"
            );
            std::process::exit(1);
        }
    }

    Ok(())
}

