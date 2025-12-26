//! SyPetype 工作流

use crate::config::{Config, Format, Stage};
use crate::ir_graph::builder::IrGraphBuilder;
use crate::ir_graph::structure::IrGraph;
use crate::parse::ParsedCrate;

// PCPN 模块
use crate::pcpn::{
    PcpnBuilder, PcpnNet, PcpnConfig, ReachabilityAnalyzer, SearchConfig,
    CodeGenerator,
};

use anyhow::Result;
use std::fs;

pub struct Pipeline {
    config: Config,
    pcpn_config: PcpnConfig,
}

impl Pipeline {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            pcpn_config: PcpnConfig::default(),
        }
    }

    /// 设置 PCPN 配置
    pub fn with_pcpn_config(mut self, pcpn_config: PcpnConfig) -> Self {
        self.pcpn_config = pcpn_config;
        self
    }

    pub fn run(&self) -> Result<()> {
        // Step 1: 解析 JSON
        self.config.log("Step 1: Parse rustdoc JSON");
        let parsed = self.parse()?;

        if !self.config.should_run(Stage::Ir) {
            self.config.log("✓ 完成");
            return Ok(());
        }

        // Step 2: 构建 IR Graph
        self.config.log("Step 2: Build IR Graph");
        let ir = self.build_ir(&parsed)?;

        if self.config.export_ir() {
            self.export_ir(&ir, &parsed)?;
        }

        if !self.config.should_run(Stage::Pn) {
            self.config.log("✓ 完成");
            return Ok(());
        }

        // Step 3: 构建 PCPN
        self.config.log("Step 3: Build PCPN");
        let pcpn = self.build_pcpn(&ir)?;

        // Step 4: 导出 PCPN
        self.config.log("Step 4: Export PCPN");
        self.export_pcpn(&pcpn)?;

        // Step 5: 可达性分析和 API 序列生成
        self.config.log("Step 5: Reachability Analysis & API Sequence Generation");
        self.analyze_and_generate(&pcpn)?;

        // Step 6: 生成 Fuzz 项目 (可选)
        if self.config.fuzz {
            self.config.log("Step 6: Generate Fuzz Project");
            self.gen_fuzz(&pcpn)?;
        }

        self.config.log("✓ 完成");
        Ok(())
    }

    fn parse(&self) -> Result<ParsedCrate> {
        self.config
            .log_verbose(&format!("  加载: {}", self.config.input.display()));

        let parsed = ParsedCrate::from_json_file(&self.config.input)
            .map_err(|e| anyhow::anyhow!("解析失败: {}", e))?;

        if self.config.verbose {
            parsed.print_stats();
        }
        Ok(parsed)
    }

    fn build_ir(&self, parsed: &ParsedCrate) -> Result<IrGraph> {
        let ir = IrGraphBuilder::new(parsed).build();

        self.config.log_verbose(&format!(
            "  IR Graph: {} 节点, {} 边",
            ir.type_graph.node_count(),
            ir.type_graph.edge_count()
        ));
        Ok(ir)
    }

    fn export_ir(&self, ir: &IrGraph, parsed: &ParsedCrate) -> Result<()> {
        fs::create_dir_all(&self.config.output)?;

        let dot = self.config.output.join("ir_graph.dot");
        ir.export_dot(parsed, &dot)?;
        self.config.log(&format!("  ✓ IR DOT: {}", dot.display()));

        let json = self.config.output.join("ir_graph.json");
        ir.export_json(&json)?;
        self.config.log(&format!("  ✓ IR JSON: {}", json.display()));

        Ok(())
    }

    /// 导出 PCPN
    fn export_pcpn(&self, pcpn: &PcpnNet) -> Result<()> {
        fs::create_dir_all(&self.config.output)?;

        for fmt in self.config.pn_formats() {
            let ext = match fmt {
                Format::Dot => "dot",
                Format::Json => "json",
                Format::Pnml => "pnml",
                Format::All => continue,
            };

            let path = self.config.output.join(format!("pcpn.{}", ext));

            match fmt {
                Format::Json => {
                    // 导出 JSON 格式
                    let stats = pcpn.stats();
                    let json = serde_json::json!({
                        "stats": {
                            "place_count": stats.place_count,
                            "transition_count": stats.transition_count,
                            "arc_count": stats.arc_count,
                            "type_count": stats.type_count,
                            "api_transition_count": stats.api_transition_count,
                            "structural_transition_count": stats.structural_transition_count,
                        }
                    });
                    fs::write(&path, serde_json::to_string_pretty(&json)?)?;
                }
                _ => {
                    // TODO: 实现其他导出格式
                    continue;
                }
            }

            self.config.log(&format!(
                "  ✓ PCPN {}: {}",
                ext.to_uppercase(),
                path.display()
            ));
        }
        Ok(())
    }

    /// 构建 PCPN
    fn build_pcpn(&self, ir: &IrGraph) -> Result<PcpnNet> {
        let builder = PcpnBuilder::new();
        let pcpn = builder.build_from_ir_graph(ir);

        let stats = pcpn.stats();
        self.config.log_verbose(&format!("  PCPN: {}", stats));

        Ok(pcpn)
    }

    /// 可达性分析和 API 序列生成
    fn analyze_and_generate(&self, pcpn: &PcpnNet) -> Result<()> {
        fs::create_dir_all(&self.config.output)?;

        // 配置搜索参数
        let search_config = SearchConfig {
            max_steps: self.pcpn_config.max_steps,
            max_stack_depth: self.pcpn_config.max_stack_depth,
            max_tokens_per_place: self.pcpn_config.max_tokens_per_place,
            ..Default::default()
        };

        let analyzer = ReachabilityAnalyzer::new(pcpn, search_config);

        // 从初始标记开始探索
        let initial = crate::pcpn::firing::Config::with_marking(pcpn.initial_marking.clone());

        // 探索可达配置
        let reachable = analyzer.explore_all(initial.clone());
        self.config.log_verbose(&format!("  探索到 {} 个可达配置", reachable.len()));

        // 尝试寻找到达目标的路径
        // 这里简化为：寻找能调用最多 API 的路径
        let result = analyzer.search(initial, |config| {
            // 目标：栈为空时停止
            config.stack.is_empty() && !config.marking.non_empty_places().is_empty()
        });

        if result.found {
            if let Some(witness) = &result.witness {
                self.config.log(&format!(
                    "  找到 API 序列: {} 步 ({} API 调用)",
                    witness.len(),
                    witness.api_calls().len()
                ));

                // 生成代码
                let mut code_gen = CodeGenerator::new(pcpn, &self.pcpn_config);
                let generated = code_gen.generate(witness);

                // 保存生成的代码
                let code_path = self.config.output.join("generated_sequence.rs");
                fs::write(&code_path, &generated.code)?;
                self.config.log(&format!("  ✓ 生成代码: {}", code_path.display()));

                // 保存 API trace
                let trace_path = self.config.output.join("api_trace.txt");
                fs::write(&trace_path, witness.to_api_sequence_string())?;
                self.config.log(&format!("  ✓ API Trace: {}", trace_path.display()));

                // 如果使用 LLM，生成提示词
                if self.pcpn_config.use_llm_completion {
                    let prompt = code_gen.generate_llm_prompt(witness, &self.config.crate_name());
                    let prompt_path = self.config.output.join("llm_prompt.md");
                    fs::write(&prompt_path, &prompt)?;
                    self.config.log(&format!("  ✓ LLM Prompt: {}", prompt_path.display()));
                }
            }
        } else {
            self.config.log("  未找到满足目标的 API 序列");
        }

        // 保存统计信息
        let stats_path = self.config.output.join("analysis_stats.json");
        let stats_json = serde_json::json!({
            "states_explored": result.states_explored,
            "configs_generated": result.configs_generated,
            "max_depth_reached": result.max_depth_reached,
            "found": result.found,
        });
        fs::write(&stats_path, serde_json::to_string_pretty(&stats_json)?)?;
        self.config.log(&format!("  ✓ 分析统计: {}", stats_path.display()));

        Ok(())
    }

    fn gen_fuzz(&self, pcpn: &PcpnNet) -> Result<()> {
        let fuzz_dir = self.config.fuzz_dir();
        let targets_dir = fuzz_dir.join("fuzz_targets");
        fs::create_dir_all(&targets_dir)?;

        let crate_name = self.config.crate_name();

        // Cargo.toml
        let lib_dep = self
            .config
            .lib_path
            .as_ref()
            .map(|p| format!("[dependencies.{}]\npath = \"{}\"", crate_name, p))
            .unwrap_or_else(|| format!("[dependencies.{}]\npath = \"..\"", crate_name));

        let cargo = format!(
            r#"[package]
name = "{}-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
arbitrary = {{ version = "1.3", features = ["derive"] }}

{}

[workspace]
members = ["."]

[[bin]]
name = "fuzz_target_1"
path = "fuzz_targets/fuzz_target_1.rs"
test = false
doc = false
"#,
            crate_name, lib_dep
        );
        fs::write(fuzz_dir.join("Cargo.toml"), cargo)?;

        // .gitignore
        fs::write(fuzz_dir.join(".gitignore"), "target\ncorpus\nartifacts\n")?;

        // 生成基于 PCPN 分析的 fuzz target
        let fuzz_target = self.generate_fuzz_target(pcpn)?;
        fs::write(targets_dir.join("fuzz_target_1.rs"), fuzz_target)?;

        self.config
            .log(&format!("  ✓ Fuzz 项目: {}", fuzz_dir.display()));
        Ok(())
    }

    /// 生成 fuzz target
    fn generate_fuzz_target(&self, pcpn: &PcpnNet) -> Result<String> {
        let crate_name = self.config.crate_name();

        // 从 PCPN 获取 API 信息
        let api_count = pcpn.api_transitions().count();

        let target = format!(
            r#"#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::{{Arbitrary, Unstructured}};

// 基于 PCPN 分析生成的 fuzz target
// API 数量: {}

/// API 选择器
#[derive(Debug, Arbitrary)]
enum ApiChoice {{
    // TODO: 根据 PCPN 分析结果生成具体的 API 选择
    Api0,
    Api1,
    // ...
}}

fuzz_target!(|data: &[u8]| {{
    let mut u = Unstructured::new(data);
    
    // 使用 arbitrary 选择 API 序列
    if let Ok(choices) = Vec::<ApiChoice>::arbitrary(&mut u) {{
        for choice in choices.iter().take(10) {{
            match choice {{
                ApiChoice::Api0 => {{
                    // TODO: 调用 API 0
                }}
                ApiChoice::Api1 => {{
                    // TODO: 调用 API 1
                }}
            }}
        }}
    }}
}});
"#,
            api_count
        );

        Ok(target)
    }
}
