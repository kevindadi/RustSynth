use std::env;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use log::LevelFilter;
use trustfall_rustdoc_adapter::Crate;
use trustfall_rustdoc_adapter::petri::PetriNetBuilder;

fn main() -> Result<()> {
    let args = parse_args()?;
    let log_level = if args.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    if let Some(log_file) = &args.log_file {
        let log_file_path = log_file.clone();
        pretty_env_logger::formatted_timed_builder()
            .filter_level(log_level)
            .target(pretty_env_logger::env_logger::Target::Pipe(Box::new(
                File::create(&log_file_path)
                    .with_context(|| format!("无法创建日志文件: {}", log_file_path.display()))?,
            )))
            .init();
        eprintln!("📝 日志将保存到: {}", log_file_path.display());
    } else {
        pretty_env_logger::formatted_timed_builder()
            .filter_level(log_level)
            .init();
    }

    let reader = BufReader::new(
        File::open(&args.json_path)
            .with_context(|| format!("无法打开 rustdoc JSON 文件:{}", args.json_path.display()))?,
    );

    let crate_data: Crate = serde_json::from_reader(reader).context("解析 rustdoc JSON 失败")?;

    let petri_net = PetriNetBuilder::from_crate_with_logs(
        &crate_data,
        args.type_log_file.as_ref(),
        args.generic_order_log_file.as_ref(),
    );

    println!(
        "Petri 网构建完成:{} 个 Place,{} 个 Transition.",
        petri_net.place_count(),
        petri_net.transition_count()
    );

    if let Some(output_path) = args.emit_net {
        let mut file = File::create(&output_path)
            .with_context(|| format!("无法创建输出文件:{}", output_path.display()))?;

        // 只支持 DOT 格式输出
        let dot_string = trustfall_rustdoc_adapter::petri::export::to_dot(&petri_net, &crate_data);
        file.write_all(dot_string.as_bytes())
            .context("写出 Petri 网 DOT 失败")?;
        println!("Petri 网 DOT 文件已写入:{}", output_path.display());
    }

    if args.goal_types.is_empty() {
        println!("未指定目标类型 (--goal),跳过程序合成.");
        return Ok(());
    }

    Ok(())
}

struct CliArgs {
    json_path: PathBuf,
    #[allow(unused)]
    input_types: Vec<String>,
    goal_types: Vec<String>,
    emit_net: Option<PathBuf>,
    #[allow(unused)]
    max_depth: Option<usize>,
    #[allow(unused)]
    max_states: Option<usize>,
    log_file: Option<PathBuf>,
    type_log_file: Option<PathBuf>,
    generic_order_log_file: Option<PathBuf>,
    verbose: bool,
}

fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1);
    let mut json_path = None;
    let mut input_types = Vec::new();
    let mut goal_types = Vec::new();
    let mut emit_net = None;
    let mut max_depth = None;
    let mut max_states = None;
    let mut log_file = None;
    let mut type_log_file = None;
    let mut generic_order_log_file = None;
    let mut verbose = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--rustdoc" => {
                let value = args.next().context("`--rustdoc` 需要一个输入文件路径")?;
                json_path = Some(PathBuf::from(value));
            }
            "--input" => {
                let value = args.next().context("`--input` 需要一个类型字符串")?;
                input_types.push(value);
            }
            "--goal" => {
                let value = args.next().context("`--goal` 需要一个类型字符串")?;
                goal_types.push(value);
            }
            "--emit-net" => {
                let value = args.next().context("`--emit-net` 需要一个输出文件路径")?;
                emit_net = Some(PathBuf::from(value));
            }
            "--max-depth" => {
                let value = args.next().context("`--max-depth` 需要最大探索深度")?;
                max_depth = Some(value.parse().context("无法解析 --max-depth 为数字")?);
            }
            "--max-states" => {
                let value = args.next().context("`--max-states` 需要最大状态数量")?;
                max_states = Some(value.parse().context("无法解析 --max-states 为数字")?);
            }
            "--log" => {
                let value = args.next().context("`--log` 需要日志文件路径")?;
                log_file = Some(PathBuf::from(value));
            }
            "--type-log" => {
                let value = args.next().context("`--type-log` 需要类型日志文件路径")?;
                type_log_file = Some(PathBuf::from(value));
            }
            "--generic-order-log" => {
                let value = args
                    .next()
                    .context("`--generic-order-log` 需要泛型偏序关系分析日志文件路径")?;
                generic_order_log_file = Some(PathBuf::from(value));
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                bail!("未知参数:{other}");
            }
        }
    }

    let json_path = json_path.ok_or_else(|| anyhow!("缺少 rustdoc JSON 文件路径."))?;

    Ok(CliArgs {
        json_path,
        input_types,
        goal_types,
        emit_net,
        max_depth,
        max_states,
        log_file,
        type_log_file,
        generic_order_log_file,
        verbose,
    })
}

fn print_usage() {
    eprintln!(
        "用法:
  petri_synth --rustdoc <rustdoc.json> [--input <类型>]... [--goal <类型>]...
               [--emit-net <输出.dot>] [--max-depth N] [--max-states N]
               [--log <日志文件>] [--type-log <类型日志文件>] [--generic-order-log <泛型偏序关系分析文件>]

示例:
  petri_synth --rustdoc target/doc/my_crate.json --input \"&str\" --goal \"String\"
  petri_synth --rustdoc target/doc/my_crate.json --type-log types.txt
  petri_synth --rustdoc target/doc/my_crate.json --generic-order-log generic_order.txt

说明:
  --rustdoc <path>     rustdoc JSON 输入文件路径 (required)
  --input <类型>             设定初始可用的类型,可多次指定.
  --goal <类型>              指定目标类型,可多次指定.
  --emit-net <输出.dot>      将构建好的 Petri 网拓扑写入 DOT 文件.
  --max-depth <N>           搜索调用序列的最大深度(默认 6).
  --max-states <N>          搜索过程中允许的最大状态数量(默认 10000).
  --log <文件>               将运行日志保存到文件.
  --type-log <文件>          将类型清单保存到文件(包含所有类型和泛型约束).
  --generic-order-log <文件> 将泛型偏序关系分析保存到文件(包含泛型可用性和约束层级关系)."
    );
}
