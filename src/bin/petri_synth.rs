use std::env;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use trustfall_rustdoc_adapter::Crate;
use trustfall_rustdoc_adapter::petri::{
    BorrowKind, FunctionSummary, PetriNetBuilder, PlaceId, SynthesisConfig, SynthesisOutcome,
    Synthesizer, TypeDescriptor,
};

fn main() -> Result<()> {
    let args = parse_args()?;
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
        let extension = output_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());

        match extension.as_deref() {
            Some("dot") | Some("gv") => {
                file.write_all(petri_net.to_dot().as_bytes())
                    .context("写出 Petri 网 DOT 失败")?;
                println!("Petri 网 DOT 拓扑已写入:{}", output_path.display());
            }
            _ => {
                serde_json::to_writer_pretty(&mut file, &petri_net)
                    .context("写出 Petri 网 JSON 失败")?;
                println!("Petri 网 JSON 拓扑已写入:{}", output_path.display());
            }
        }
    }

    if args.goal_types.is_empty() {
        println!("未指定目标类型 (--goal),跳过程序合成.");
        return Ok(());
    }

    let initial_tokens = resolve_descriptors(&petri_net, &args.input_types)?;
    let goal_tokens = resolve_descriptors(&petri_net, &args.goal_types)?;

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
                    print_state(&petri_net, idx, &plan.states[idx], &plan.place_indices)?;
                }

                if let Some(transition) = petri_net.transition(*transition_id) {
                    print_transition(idx, &transition.summary)?;
                }
            }
            if let Some(final_state) = plan.states.last() {
                print_state(
                    &petri_net,
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
        for (place_id, place, count) in places_with_tokens {
            let borrow_kinds = state.available_borrows.get(&place_id);
            let borrow_str = if let Some(borrows) = borrow_kinds {
                let mut kinds: Vec<String> = borrows
                    .iter()
                    .map(|b| match *b {
                        BorrowKind::Owned => "Owned".to_string(),
                        BorrowKind::SharedRef => "&".to_string(),
                        BorrowKind::MutRef => "&mut".to_string(),
                        BorrowKind::RawConstPtr => "*const".to_string(),
                        BorrowKind::RawMutPtr => "*mut".to_string(),
                    })
                    .collect();
                kinds.sort();
                format!("[{}]", kinds.join(", "))
            } else {
                "[Unknown]".to_string()
            };
            writeln!(
                stdout,
                "  {}: {} 个令牌, 借用类型: {}",
                place.descriptor.display(),
                count,
                borrow_str
            )?;
        }
    }
    writeln!(stdout)?;
    Ok(())
}

fn print_transition(index: usize, summary: &FunctionSummary) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "步骤 {index}: {}", summary.signature)?;
    if let Some(path) = &summary.qualified_path {
        writeln!(stdout, "  定位:{path}")?;
    }
    writeln!(stdout, "  上下文:{}", describe_context(&summary.context))?;
    if !summary.generics.is_empty() {
        writeln!(
            stdout,
            "  泛型:{}",
            summary
                .generics
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    if !summary.where_clauses.is_empty() {
        writeln!(
            stdout,
            "  where 子句:{}",
            summary
                .where_clauses
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    if !summary.trait_bounds.is_empty() {
        writeln!(
            stdout,
            "  Trait 约束:{}",
            summary
                .trait_bounds
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    Ok(())
}

fn describe_context(context: &trustfall_rustdoc_adapter::petri::FunctionContext) -> String {
    match context {
        trustfall_rustdoc_adapter::petri::FunctionContext::FreeFunction => "无约束函数".to_string(),
        trustfall_rustdoc_adapter::petri::FunctionContext::InherentMethod { receiver } => {
            format!("固有方法,接收者:{}", receiver.display())
        }
        trustfall_rustdoc_adapter::petri::FunctionContext::TraitImplementation {
            receiver,
            trait_path,
        } => format!(
            "Trait 实现方法,接收者:{},Trait:{}",
            receiver.display(),
            trait_path
        ),
    }
}

fn resolve_descriptors(
    net: &trustfall_rustdoc_adapter::petri::PetriNet,
    names: &[String],
) -> Result<Vec<TypeDescriptor>> {
    let mut descriptors = Vec::new();
    let mut missing = Vec::new();
    for name in names {
        match find_descriptor(net, name) {
            Some(descriptor) => descriptors.push(descriptor),
            None => missing.push(name.clone()),
        }
    }
    if missing.is_empty() {
        Ok(descriptors)
    } else {
        Err(anyhow!("以下类型未匹配:{}", missing.join(", ")))
    }
}

fn find_descriptor(
    net: &trustfall_rustdoc_adapter::petri::PetriNet,
    name: &str,
) -> Option<TypeDescriptor> {
    // 解析借用类型前缀
    let (base_name, borrow_kind) = parse_borrow_prefix(name);

    // 查找基础类型（规范化版本）
    let base_descriptor = net.places().find_map(|place| {
        let descriptor = &place.1.descriptor;
        let normalized = descriptor.normalized();
        if normalized.display() == base_name || normalized.canonical() == base_name {
            Some(normalized)
        } else {
            None
        }
    })?;

    Some(base_descriptor.with_borrow_kind(borrow_kind))
}

/// 解析类型名中的借用前缀，返回基础类型名和借用类型
fn parse_borrow_prefix(name: &str) -> (&str, BorrowKind) {
    let name = name.trim();

    // 检查原始指针
    if name.starts_with("*const ") {
        return (&name[7..].trim(), BorrowKind::RawConstPtr);
    }
    if name.starts_with("*mut ") {
        return (&name[5..].trim(), BorrowKind::RawMutPtr);
    }

    // 检查引用
    if name.starts_with('&') {
        let mut rest = &name[1..];
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
            return (&rest[4..].trim(), BorrowKind::MutRef);
        } else if rest.starts_with("mut")
            && (rest.len() == 3 || !(rest.as_bytes()[3] as char).is_alphabetic())
        {
            return (&rest[3..].trim(), BorrowKind::MutRef);
        } else {
            return (rest.trim(), BorrowKind::SharedRef);
        }
    }

    // 没有借用前缀，是 Owned
    (name, BorrowKind::Owned)
}

struct CliArgs {
    json_path: PathBuf,
    input_types: Vec<String>,
    goal_types: Vec<String>,
    emit_net: Option<PathBuf>,
    max_depth: Option<usize>,
    max_states: Option<usize>,
}

fn parse_args() -> Result<CliArgs> {
    let mut args = env::args().skip(1);
    let mut json_path = None;
    let mut input_types = Vec::new();
    let mut goal_types = Vec::new();
    let mut emit_net = None;
    let mut max_depth = None;
    let mut max_states = None;

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
    })
}

fn print_usage() {
    eprintln!(
        "用法:
  petri_synth <rustdoc.json> [--input <类型>]... [--goal <类型>]...
               [--emit-net <输出.json>] [--max-depth N] [--max-states N]

示例:
  petri_synth --rustdoc target/doc/my_crate.json --input \"&str\" --goal \"String\"

说明:
  --rustdoc <path>     rustdoc JSON 输入文件路径 (required)
  --input <类型>             设定初始可用的类型,可多次指定.
  --goal <类型>              指定目标类型,可多次指定.
  --emit-net <输出.json>     将构建好的 Petri 网拓扑写入 JSON 文件.
  --max-depth <N>           搜索调用序列的最大深度(默认 6).
  --max-states <N>          搜索过程中允许的最大状态数量(默认 10000)."
    );
}
