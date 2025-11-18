use std::env;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use log::LevelFilter;
use rustdoc_types::Id;
use trustfall_rustdoc_adapter::Crate;
use trustfall_rustdoc_adapter::petri::{
    PetriNetBuilder, PlaceId, SynthesisConfig, SynthesisOutcome, Synthesizer, Transition,
};

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

    let petri_net = PetriNetBuilder::from_crate(&crate_data);

    println!(
        "Petri 网构建完成:{} 个 Place,{} 个 Transition.",
        petri_net.place_count(),
        petri_net.transition_count()
    );

    if let Some(output_path) = args.emit_net {
        let mut file = File::create(&output_path)
            .with_context(|| format!("无法创建输出文件:{}", output_path.display()))?;

        // 只支持 DOT 格式输出
        file.write_all(petri_net.to_dot(&crate_data).as_bytes())
            .context("写出 Petri 网 DOT 失败")?;
        println!("Petri 网 DOT 文件已写入:{}", output_path.display());
    }

    if args.goal_types.is_empty() {
        println!("未指定目标类型 (--goal),跳过程序合成.");
        return Ok(());
    }

    let initial_tokens = resolve_item_ids(&petri_net, &crate_data, &args.input_types)?;
    let goal_tokens = resolve_item_ids(&petri_net, &crate_data, &args.goal_types)?;

    let config = SynthesisConfig {
        max_depth: args.max_depth.unwrap_or(6),
        max_states: args.max_states.unwrap_or(10_000),
    };
    let synthesizer = Synthesizer::with_config(&petri_net, config);
    match synthesizer.synthesize(&initial_tokens, &goal_tokens) {
        SynthesisOutcome::Success(plan) => {
            println!("合成成功,调用序列长度:{}", plan.transitions.len());
            for (idx, transition_id) in plan.transitions.iter().enumerate() {
                if idx < plan.states.len() {
                    print_state(
                        &petri_net,
                        &crate_data,
                        idx,
                        &plan.states[idx],
                        &plan.place_indices,
                    )?;
                }

                if let Some(transition) = petri_net.transition(*transition_id) {
                    print_transition(&crate_data, idx, transition)?;
                }
            }
            if let Some(final_state) = plan.states.last() {
                print_state(
                    &petri_net,
                    &crate_data,
                    plan.transitions.len(),
                    final_state,
                    &plan.place_indices,
                )?;
            }
        }
        SynthesisOutcome::InvalidTypes { missing } => {
            eprintln!("输入或目标类型未在 Petri 网中找到:");
            for ty in missing {
                eprintln!("  - {ty}");
            }
            std::process::exit(2);
        }
        SynthesisOutcome::LimitExceeded => {
            eprintln!("搜索空间限制达到(max_depth / max_states),未能找到满足条件的程序.");
            std::process::exit(3);
        }
        SynthesisOutcome::GoalUnreachable => {
            eprintln!("未找到满足目标类型的调用序列.");
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_state(
    net: &trustfall_rustdoc_adapter::petri::PetriNet,
    crate_: &Crate,
    step: usize,
    state: &trustfall_rustdoc_adapter::petri::StepState,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "步骤 {step} 后的库所状态:")?;

    // 找到所有有令牌的库所
    let mut places_with_tokens = Vec::new();
    for (place_id, place) in net.places() {
        if let Some(&idx) = place_indices.get(&place_id) {
            if state.marking[idx] > 0 {
                places_with_tokens.push((place_id, place, state.marking[idx]));
            }
        }
    }

    if places_with_tokens.is_empty() {
        writeln!(stdout, "  (无令牌)")?;
    } else {
        for (_place_id, place, count) in places_with_tokens {
            // 获取类型名称
            let type_name = if let Some(item_id) = place.item_id() {
                crate_
                    .index
                    .get(&item_id)
                    .and_then(|item| item.name.as_deref())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Item ID: {}", item_id.0))
            } else {
                "Unknown".to_string()
            };
            writeln!(stdout, "  {}: {} 个令牌", type_name, count)?;
        }
    }
    writeln!(stdout)?;
    Ok(())
}

fn print_transition(crate_: &Crate, index: usize, transition: &Transition) -> io::Result<()> {
    let mut stdout = io::stdout().lock();

    // 获取函数名称
    let function_name = crate_
        .index
        .get(&transition.item_id)
        .and_then(|item| item.name.as_deref())
        .unwrap_or("Unknown");

    // 构建简化的函数签名：类型名 + 函数名
    let sig = format!("{}::{}", function_name, transition.name.as_ref());
    writeln!(stdout, "步骤 {index}: {}", sig)?;

    // 获取完整路径
    if let Some(path_summary) = crate_.paths.get(&transition.item_id) {
        let path = path_summary.path.join("::");
        writeln!(stdout, "  定位:{}", path)?;
    }

    writeln!(
        stdout,
        "  上下文:{}",
        describe_context(&transition.context, crate_)
    )?;

    if let Some(input_types) = &transition.input_types {
        let input_names: Vec<String> = input_types
            .iter()
            .filter_map(|id| {
                crate_
                    .index
                    .get(id)
                    .and_then(|item| item.name.as_deref())
                    .map(|s| s.to_string())
            })
            .collect();
    }

    if let Some(output_id) = transition.output_type {
        if let Some(output_item) = crate_.index.get(&output_id) {
            if let Some(output_name) = output_item.name.as_deref() {
                writeln!(stdout, "  返回类型:{}", output_name)?;
            }
        }
    }

    Ok(())
}

fn describe_context(
    context: &trustfall_rustdoc_adapter::petri::FunctionContext,
    crate_: &Crate,
) -> String {
    match context {
        trustfall_rustdoc_adapter::petri::FunctionContext::FreeFunction => "无约束函数".to_string(),
        trustfall_rustdoc_adapter::petri::FunctionContext::InherentMethod { receiver_id } => {
            let receiver_name = crate_
                .index
                .get(receiver_id)
                .and_then(|item| item.name.as_deref())
                .unwrap_or("Unknown");
            format!("固有方法,接收者:{}", receiver_name)
        }
        trustfall_rustdoc_adapter::petri::FunctionContext::TraitImplementation {
            receiver_id,
            trait_path,
        } => {
            let receiver_name = crate_
                .index
                .get(receiver_id)
                .and_then(|item| item.name.as_deref())
                .unwrap_or("Unknown");
            format!(
                "Trait 实现方法,接收者:{},Trait:{}",
                receiver_name, trait_path
            )
        }
    }
}

fn resolve_item_ids(
    net: &trustfall_rustdoc_adapter::petri::PetriNet,
    crate_: &Crate,
    names: &[String],
) -> Result<Vec<Id>> {
    let mut item_ids = Vec::new();
    let mut missing = Vec::new();
    for name in names {
        match find_item_id(net, crate_, name) {
            Some(id) => item_ids.push(id),
            None => missing.push(name.clone()),
        }
    }
    if missing.is_empty() {
        Ok(item_ids)
    } else {
        Err(anyhow!("以下类型未匹配:{}", missing.join(", ")))
    }
}

fn find_item_id(
    net: &trustfall_rustdoc_adapter::petri::PetriNet,
    crate_: &Crate,
    name: &str,
) -> Option<Id> {
    // 移除借用前缀（&, &mut, *const, *mut）
    let base_name = name.trim();
    let base_name = if base_name.starts_with("*const ") {
        &base_name[7..].trim()
    } else if base_name.starts_with("*mut ") {
        &base_name[5..].trim()
    } else if base_name.starts_with('&') {
        let mut rest = &base_name[1..];
        // 跳过生命周期
        while rest.starts_with('\'') {
            let mut end = 1;
            while end < rest.len() && (rest.as_bytes()[end] as char).is_alphanumeric()
                || rest.as_bytes()[end] == b'_'
            {
                end += 1;
            }
            rest = &rest[end..].trim_start();
        }
        // 检查 mut
        if rest.starts_with("mut ") {
            &rest[4..].trim()
        } else if rest.starts_with("mut")
            && (rest.len() == 3 || !(rest.as_bytes()[3] as char).is_alphabetic())
        {
            &rest[3..].trim()
        } else {
            rest.trim()
        }
    } else {
        base_name
    };

    // 在 crate 的 index 中查找匹配的 Item
    for (item_id, item) in crate_.index.iter() {
        // 检查完整路径或简单名称
        if let Some(item_name) = item.name.as_deref() {
            // 检查简单名称匹配
            if item_name == base_name {
                // 验证该类型在 Petri 网中有对应的 Place
                if net.place_id(*item_id).is_some() {
                    return Some(*item_id);
                }
            }

            // 检查完整路径匹配
            if let Some(path_summary) = crate_.paths.get(item_id) {
                let full_path = path_summary.path.join("::");
                if full_path == base_name || full_path.ends_with(&format!("::{}", base_name)) {
                    if net.place_id(*item_id).is_some() {
                        return Some(*item_id);
                    }
                }
            }
        }
    }

    None
}

struct CliArgs {
    json_path: PathBuf,
    input_types: Vec<String>,
    goal_types: Vec<String>,
    emit_net: Option<PathBuf>,
    max_depth: Option<usize>,
    max_states: Option<usize>,
    log_file: Option<PathBuf>,
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
        verbose,
    })
}

fn print_usage() {
    eprintln!(
        "用法:
  petri_synth --rustdoc <rustdoc.json> [--input <类型>]... [--goal <类型>]...
               [--emit-net <输出.dot>] [--max-depth N] [--max-states N]

示例:
  petri_synth --rustdoc target/doc/my_crate.json --input \"&str\" --goal \"String\"

说明:
  --rustdoc <path>     rustdoc JSON 输入文件路径 (required)
  --input <类型>             设定初始可用的类型,可多次指定.
  --goal <类型>              指定目标类型,可多次指定.
  --emit-net <输出.dot>      将构建好的 Petri 网拓扑写入 DOT 文件.
  --max-depth <N>           搜索调用序列的最大深度(默认 6).
  --max-states <N>          搜索过程中允许的最大状态数量(默认 10000)."
    );
}
