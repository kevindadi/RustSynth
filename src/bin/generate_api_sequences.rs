//! 生成 API 序列工具
//! 
//! 使用 PCPN 分析生成最大 API 覆盖序列

use std::fs;
use std::path::{Path, PathBuf};
use clap::Parser;
use anyhow::Result;

// 从 lib 导入
use rustdoc_petri_net_builder::parse::ParsedCrate;
use rustdoc_petri_net_builder::ir_graph::builder::IrGraphBuilder;
use rustdoc_petri_net_builder::pcpn::{
    PcpnBuilder, PcpnConfig, ReachabilityAnalyzer, SearchConfig, SearchStrategy,
    CodeGenerator, GenerationMode, Witness,
    marking::{Marking, Token, ValueIdGen},
    types::{RustType, PrimitiveType},
    firing::Config,
};

/// API 序列生成工具
#[derive(Parser, Debug)]
#[command(name = "generate-api-sequences")]
#[command(about = "从 rustdoc JSON 生成最大 API 覆盖序列")]
struct Args {
    /// rustdoc JSON 文件
    #[arg(value_name = "JSON")]
    input: PathBuf,

    /// 输出目录
    #[arg(short, long, default_value = "generated_examples")]
    output: PathBuf,

    /// 最大搜索步数
    #[arg(long, default_value = "1000")]
    max_steps: usize,

    /// 详细输出
    #[arg(short, long)]
    verbose: bool,

    /// Crate 名称（用于 LLM 提示词）
    #[arg(short, long)]
    crate_name: Option<String>,

    /// 生成可达性图
    #[arg(long)]
    reachability_graph: bool,

    /// 最大可达性图状态数
    #[arg(long, default_value = "50")]
    max_graph_states: usize,

    /// DeepSeek API Key（用于代码补全）
    #[arg(long, env = "DEEPSEEK_API_KEY")]
    deepseek_api_key: Option<String>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // 推断 crate 名称
    let crate_name = args.crate_name.unwrap_or_else(|| {
        args.input.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // 创建输出目录
    fs::create_dir_all(&args.output)?;

    // 解析 JSON
    log::info!("📦 解析 {} ...", args.input.display());
    let parsed = ParsedCrate::from_json_file(&args.input)?;
    log::info!("  ✓ 解析完成: {} 个 items", parsed.crate_data.index.len());

    // 构建 IR Graph
    log::info!("🔧 构建 IR Graph...");
    let ir = IrGraphBuilder::new(&parsed).build();
    log::info!("  ✓ 节点: {}, 边: {}", ir.type_graph.node_count(), ir.type_graph.edge_count());

    // 构建 PCPN
    log::info!("🕸️ 构建 PCPN...");
    let builder = PcpnBuilder::new();
    let mut pcpn = builder.build_from_ir_graph(&ir);
    let stats = pcpn.stats();
    log::info!("  ✓ Places: {}, Transitions: {}, Types: {}", 
        stats.place_count, stats.transition_count, stats.type_count);
    log::info!("  ✓ API Transitions: {}, Structural Transitions: {}",
        stats.api_transition_count, stats.structural_transition_count);

    // 创建初始 marking（添加基本类型和 [u8]）
    log::info!("🎯 配置初始 token...");
    let initial_marking = create_initial_marking(&mut pcpn);
    log::info!("  ✓ 初始 token 已添加");

    // 可达性分析 - 使用最大 API 覆盖策略
    log::info!("🔍 最大 API 覆盖搜索...");
    let search_config = SearchConfig {
        max_steps: args.max_steps,
        max_stack_depth: 10,
        max_tokens_per_place: 10,
        strategy: SearchStrategy::MaxApiCoverage,
    };

    let analyzer = ReachabilityAnalyzer::new(&pcpn, search_config);
    let initial = Config::with_marking(initial_marking.clone());

    // 生成可达性图
    if args.reachability_graph {
        log::info!("📊 生成可达性图...");
        let dot = analyzer.generate_reachability_graph(initial.clone(), args.max_graph_states);
        let graph_path = args.output.join("reachability_graph.dot");
        fs::write(&graph_path, &dot)?;
        log::info!("  ✓ 可达性图: {} ({} bytes)", graph_path.display(), dot.len());
        
        // 尝试生成 SVG（如果有 dot 命令）
        if let Ok(output) = std::process::Command::new("dot")
            .args(["-Tsvg", "-o"])
            .arg(args.output.join("reachability_graph.svg"))
            .arg(&graph_path)
            .output()
        {
            if output.status.success() {
                log::info!("  ✓ 可达性图 SVG: {}", args.output.join("reachability_graph.svg").display());
            }
        }
    }

    // 执行最大覆盖搜索
    let result = analyzer.search(initial, |_| false); // 不设置目标，让它贪婪搜索
    let witness = result.witness.unwrap_or_else(Witness::empty);
    
    log::info!("  ✓ 覆盖 {} 个 API 调用", witness.api_calls().len());
    log::info!("  ✓ 总步数: {}", witness.len());

    // 打印 API 调用序列
    log::info!("📋 API 调用序列:");
    for (i, step) in witness.api_calls().iter().enumerate() {
        log::info!("   {}. {}", i + 1, step.transition_name);
    }

    // 生成输出
    generate_outputs(&pcpn, &witness, &args.output, &crate_name)?;

    // 如果有 DeepSeek API Key，尝试补全代码
    if let Some(api_key) = args.deepseek_api_key {
        log::info!("🤖 调用 DeepSeek API 补全代码...");
        complete_with_deepseek(&pcpn, &witness, &args.output, &crate_name, &api_key)?;
    }

    log::info!("✨ 完成! 输出目录: {}", args.output.display());

    // 打印文件列表
    log::info!("📁 生成的文件:");
    let mut entries: Vec<_> = fs::read_dir(&args.output)?.collect();
    entries.sort_by_key(|e| e.as_ref().ok().map(|e| e.path()));
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let size = fs::metadata(&path)?.len();
            log::info!("  - {} ({} bytes)", path.file_name().unwrap().to_string_lossy(), size);
        }
    }

    Ok(())
}

/// 创建初始 marking，添加基本类型 token
fn create_initial_marking(pcpn: &mut rustdoc_petri_net_builder::pcpn::PcpnNet) -> Marking {
    let mut marking = pcpn.initial_marking.clone();
    let mut value_gen = ValueIdGen::new();

    // 添加基本类型（可重复使用）
    let primitives = [
        ("u8", PrimitiveType::U8),
        ("u16", PrimitiveType::U16),
        ("u32", PrimitiveType::U32),
        ("u64", PrimitiveType::U64),
        ("usize", PrimitiveType::Usize),
        ("i8", PrimitiveType::I8),
        ("i32", PrimitiveType::I32),
        ("i64", PrimitiveType::I64),
        ("bool", PrimitiveType::Bool),
        ("char", PrimitiveType::Char),
    ];

    for (name, prim) in primitives {
        // 注册类型
        let type_id = pcpn.types.register(RustType::Primitive(prim));
        
        // 找到或创建对应的 place
        if let Some(place_id) = pcpn.find_or_create_place_for_type(type_id) {
            // 添加多个 token（基本类型可重复使用）
            for _ in 0..3 {
                marking.add(place_id, Token {
                    type_id,
                    value_id: value_gen.next(),
                });
            }
            log::info!("    + {} (3 tokens)", name);
        }
    }

    // 添加 [u8] 切片类型
    let u8_type_id = pcpn.types.register(RustType::Primitive(PrimitiveType::U8));
    let slice_type_id = pcpn.types.register(RustType::Slice(Box::new(RustType::Primitive(PrimitiveType::U8))));
    if let Some(place_id) = pcpn.find_or_create_place_for_type(slice_type_id) {
        for _ in 0..3 {
            marking.add(place_id, Token {
                type_id: slice_type_id,
                value_id: value_gen.next(),
            });
        }
        log::info!("    + [u8] (3 tokens)");
    }

    // 添加 &[u8] 引用类型
    let ref_slice_type_id = pcpn.types.register(RustType::Reference {
        is_mut: false,
        inner: Box::new(RustType::Slice(Box::new(RustType::Primitive(PrimitiveType::U8)))),
    });
    if let Some(place_id) = pcpn.find_or_create_place_for_type(ref_slice_type_id) {
        for _ in 0..3 {
            marking.add(place_id, Token {
                type_id: ref_slice_type_id,
                value_id: value_gen.next(),
            });
        }
        log::info!("    + &[u8] (3 tokens)");
    }

    // 添加 String 类型
    let string_type_id = pcpn.types.register(RustType::Named {
        path: "String".to_string(),
        type_args: vec![],
    });
    if let Some(place_id) = pcpn.find_or_create_place_for_type(string_type_id) {
        for _ in 0..2 {
            marking.add(place_id, Token {
                type_id: string_type_id,
                value_id: value_gen.next(),
            });
        }
        log::info!("    + String (2 tokens)");
    }

    // 添加 &str 类型
    let str_type_id = pcpn.types.register(RustType::Reference {
        is_mut: false,
        inner: Box::new(RustType::Primitive(PrimitiveType::Str)),
    });
    if let Some(place_id) = pcpn.find_or_create_place_for_type(str_type_id) {
        for _ in 0..2 {
            marking.add(place_id, Token {
                type_id: str_type_id,
                value_id: value_gen.next(),
            });
        }
        log::info!("    + &str (2 tokens)");
    }

    // 标记这些类型为可无限使用（Copy 语义）
    let _ = u8_type_id; // 使用变量避免警告

    marking
}

/// 生成各种模式的输出
fn generate_outputs(
    pcpn: &rustdoc_petri_net_builder::pcpn::PcpnNet,
    witness: &Witness,
    output_dir: &Path,
    crate_name: &str,
) -> Result<()> {
    log::info!("📝 生成输出...");

    // 1. 带占位符的代码（让 LLM 补全）
    log::info!("  1️⃣ 带占位符的代码:");
    let config = PcpnConfig {
        generation_mode: GenerationMode::WithPlaceholders,
        generate_compilable_code: false,
        include_type_annotations: false,
        generate_test_harness: true,
        ..Default::default()
    };
    let mut generator = CodeGenerator::new(pcpn, &config);
    let code = generator.generate(witness);

    let path = output_dir.join("1_api_sequence_with_placeholders.rs");
    fs::write(&path, &code.code)?;
    log::info!("     → {} (API: {})", path.display(), code.api_trace.len());

    // 2. API 序列列表
    log::info!("  2️⃣ API 序列列表:");
    let mut trace = String::new();
    trace.push_str(&format!("// {} API Sequence\n", crate_name));
    trace.push_str(&format!("// Total: {} API calls\n\n", witness.api_calls().len()));
    for (i, step) in witness.api_calls().iter().enumerate() {
        trace.push_str(&format!("{}. {}\n", i + 1, step.transition_name));
    }
    let path = output_dir.join("2_api_trace.txt");
    fs::write(&path, &trace)?;
    log::info!("     → {}", path.display());

    // 3. LLM 提示词
    log::info!("  3️⃣ LLM 补全提示词:");
    let prompt = generate_llm_prompt(pcpn, witness, crate_name, &code.code);
    let path = output_dir.join("3_llm_prompt.md");
    fs::write(&path, &prompt)?;
    log::info!("     → {}", path.display());

    // 4. 统计信息 JSON
    log::info!("  4️⃣ 统计信息:");
    let stats = serde_json::json!({
        "crate_name": crate_name,
        "total_api_calls": witness.api_calls().len(),
        "total_steps": witness.len(),
        "unique_apis": witness.api_calls().iter()
            .map(|s| &s.transition_name)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "api_sequence": witness.api_calls().iter()
            .map(|s| &s.transition_name)
            .collect::<Vec<_>>(),
    });
    let path = output_dir.join("4_stats.json");
    fs::write(&path, serde_json::to_string_pretty(&stats)?)?;
    log::info!("     → {}", path.display());

    Ok(())
}

/// 生成 LLM 补全提示词
fn generate_llm_prompt(
    _pcpn: &rustdoc_petri_net_builder::pcpn::PcpnNet,
    witness: &Witness,
    crate_name: &str,
    code_template: &str,
) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!(r#"# {} API 序列补全任务

## 任务描述

请根据以下 API 调用序列模板，生成完整的、可编译的 Rust 测试代码。

## API 调用序列

共 {} 个 API 调用：

"#, crate_name, witness.api_calls().len()));

    for (i, step) in witness.api_calls().iter().enumerate() {
        prompt.push_str(&format!("{}. `{}`\n", i + 1, step.transition_name));
    }

    prompt.push_str(&format!(r#"

## 代码模板

以下代码包含 `todo!()` 占位符，请补全这些占位符：

```rust
{}
```

## 补全要求

1. **基本类型值**：
   - `u8`, `u16`, `u32` 等整数类型使用 `0` 或随机值
   - `bool` 使用 `true` 或 `false`
   - `&str` 使用 `"test"` 或类似字符串
   - `&[u8]` 使用 `&[0u8; 32]` 或 `b"test data"`

2. **复杂类型**：
   - 优先使用 `Default::default()` 或 `::new()`
   - 如果需要特定配置，请参考 crate 文档

3. **所有权规则**：
   - 正确处理借用和所有权
   - 需要 `&mut` 的地方创建可变绑定

4. **错误处理**：
   - `Result` 类型使用 `.unwrap()` 或 `?`
   - `Option` 类型使用 `.unwrap()` 或合适的默认值

5. **输出格式**：
   - 生成完整的 Rust 测试函数
   - 包含必要的 `use` 语句
   - 代码应该能够编译通过

## 示例参考

对于 base64 crate，典型的用法：

```rust
use base64::{{Engine, engine::general_purpose}};

let input = b"hello world";
let encoded = general_purpose::STANDARD.encode(input);
let decoded = general_purpose::STANDARD.decode(&encoded).unwrap();
assert_eq!(input.as_slice(), decoded.as_slice());
```

请生成完整的补全代码：
"#, code_template));

    prompt
}

/// 使用 DeepSeek API 补全代码
fn complete_with_deepseek(
    pcpn: &rustdoc_petri_net_builder::pcpn::PcpnNet,
    witness: &Witness,
    output_dir: &Path,
    crate_name: &str,
    api_key: &str,
) -> Result<()> {
    // 生成提示词
    let config = PcpnConfig {
        generation_mode: GenerationMode::WithPlaceholders,
        generate_compilable_code: false,
        include_type_annotations: false,
        generate_test_harness: true,
        ..Default::default()
    };
    let mut generator = CodeGenerator::new(pcpn, &config);
    let code = generator.generate(witness);
    let prompt = generate_llm_prompt(pcpn, witness, crate_name, &code.code);

    // 构建请求
    let request = serde_json::json!({
        "model": "deepseek-chat",
        "messages": [
            {
                "role": "system",
                "content": "你是一个 Rust 代码生成专家。请根据给定的 API 调用序列模板，生成完整的、可编译的 Rust 测试代码。只输出代码，不要解释。"
            },
            {
                "role": "user", 
                "content": prompt
            }
        ],
        "temperature": 0.3,
        "max_tokens": 4000
    });

    // 发送请求
    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()?;

    if response.status().is_success() {
        let result: serde_json::Value = response.json()?;
        if let Some(content) = result["choices"][0]["message"]["content"].as_str() {
            // 提取代码块
            let code = extract_code_block(content);
            let path = output_dir.join("5_deepseek_completed.rs");
            fs::write(&path, &code)?;
            log::info!("  ✓ DeepSeek 补全代码: {}", path.display());
        }
    } else {
        log::warn!("  ⚠ DeepSeek API 请求失败: {}", response.status());
    }

    Ok(())
}

/// 从 markdown 中提取代码块
fn extract_code_block(content: &str) -> String {
    // 尝试提取 ```rust ... ``` 块
    if let Some(start) = content.find("```rust") {
        let rest = &content[start + 7..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 尝试提取 ``` ... ``` 块
    if let Some(start) = content.find("```") {
        let rest = &content[start + 3..];
        if let Some(end) = rest.find("```") {
            return rest[..end].trim().to_string();
        }
    }
    // 返回原始内容
    content.to_string()
}
