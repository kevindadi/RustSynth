/// SyPetype 完整工作流管道
use crate::config::Config;
use crate::generate::FuzzTargetGenerator;
use crate::ir_graph::builder::build_ir_graph;
use crate::ir_graph::structure::IrGraph;
use crate::parse::ParsedCrate;
use crate::pt_net::builder::PetriNetBuilder;
use crate::pt_net::structure::PetriNet;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// SyPetype 工作流执行器
pub struct Pipeline {
    config: Config,
}

impl Pipeline {
    /// 创建新的工作流执行器
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// 执行完整的工作流
    pub fn run(&self) -> Result<()> {
        log::info!("=== SyPetype 工作流开始 ===");

        // 步骤 1: 解析 rustdoc JSON
        log::info!("步骤 1: 解析 rustdoc JSON");
        let parsed_crate = self.parse_json()?;

        // 步骤 2: 构建 IR Graph
        log::info!("步骤 2: 构建 IR Graph");
        let ir_graph = self.build_ir_graph(parsed_crate)?;

        // 步骤 3: 导出 IR Graph（如果配置启用）
        if self.config.export.export_ir_graph_dot || self.config.export.export_ir_graph_json {
            log::info!("步骤 3: 导出 IR Graph");
            self.export_ir_graph(&ir_graph)?;
        }

        // 步骤 4: 转换为 Petri Net
        log::info!("步骤 4: 转换为 Petri Net");
        let petri_net = self.build_petri_net(&ir_graph)?;

        // 步骤 5: 导出 Petri Net（如果配置启用）
        if self.config.export.export_petri_net_dot || self.config.export.export_petri_net_json {
            log::info!("步骤 5: 导出 Petri Net");
            self.export_petri_net(&petri_net)?;
        }

        // 步骤 6: 生成 Fuzz Target
        log::info!("步骤 6: 生成 Fuzz Target");
        self.generate_fuzz_target(&petri_net)?;

        // 步骤 7: 生成 Fuzz 项目结构
        log::info!("步骤 7: 生成 Fuzz 项目结构");
        self.setup_fuzz_project()?;

        log::info!("=== SyPetype 工作流完成 ===");
        Ok(())
    }

    /// 解析 rustdoc JSON 文件
    fn parse_json(&self) -> Result<ParsedCrate> {
        log::info!("  从文件加载: {}", self.config.input_json.display());

        let parsed_crate = ParsedCrate::from_json_file(&self.config.input_json)
            .map_err(|e| anyhow::anyhow!("无法解析 rustdoc JSON 文件: {}", e))?;

        if self.config.export.print_stats {
            parsed_crate.print_stats();
        }

        Ok(parsed_crate)
    }

    /// 构建 IR Graph
    fn build_ir_graph(&self, parsed_crate: ParsedCrate) -> Result<IrGraph> {
        let ir_graph = build_ir_graph(parsed_crate);

        if self.config.export.print_stats {
            log::info!("  IR Graph 统计:");
            log::info!("    类型节点数: {}", ir_graph.type_nodes.len());
            log::info!("    操作节点数: {}", ir_graph.operations.len());
        }

        Ok(ir_graph)
    }

    /// 导出 IR Graph
    fn export_ir_graph(&self, ir_graph: &IrGraph) -> Result<()> {
        // 确保输出目录存在
        fs::create_dir_all(&self.config.output.output_dir)?;

        // 导出 DOT 格式
        if self.config.export.export_ir_graph_dot {
            let dot_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.ir_graph_dot_name);
            let dot_content = ir_graph.export_to_dot();
            fs::write(&dot_path, dot_content)?;
            log::info!("  ✓ IR Graph DOT 已导出: {}", dot_path.display());
        }

        // 导出 JSON 格式
        if self.config.export.export_ir_graph_json {
            let json_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.ir_graph_json_name);
            let json_content = ir_graph.export_to_json();
            fs::write(&json_path, serde_json::to_string_pretty(&json_content)?)?;
            log::info!("  ✓ IR Graph JSON 已导出: {}", json_path.display());
        }

        Ok(())
    }

    /// 构建 Petri Net
    fn build_petri_net(&self, ir_graph: &IrGraph) -> Result<PetriNet> {
        let petri_net = PetriNetBuilder::from_ir(ir_graph);

        if self.config.export.print_stats {
            log::info!("  {}", petri_net.export_stats());
        }

        Ok(petri_net)
    }

    /// 导出 Petri Net
    fn export_petri_net(&self, petri_net: &PetriNet) -> Result<()> {
        // 确保输出目录存在
        fs::create_dir_all(&self.config.output.output_dir)?;

        // 导出 DOT 格式
        if self.config.export.export_petri_net_dot {
            let dot_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.petri_net_dot_name);
            let dot_content = petri_net.export_to_dot();
            fs::write(&dot_path, dot_content)?;
            log::info!("  ✓ Petri Net DOT 已导出: {}", dot_path.display());
        }

        // 导出 JSON 格式
        if self.config.export.export_petri_net_json {
            let json_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.petri_net_json_name);
            let json_content = petri_net.export_to_json();
            fs::write(&json_path, serde_json::to_string_pretty(&json_content)?)?;
            log::info!("  ✓ Petri Net JSON 已导出: {}", json_path.display());
        }

        Ok(())
    }

    /// 生成 Fuzz Target
    fn generate_fuzz_target(&self, petri_net: &PetriNet) -> Result<()> {
        let generator = FuzzTargetGenerator::new(self.config.target_crate.clone());
        let fuzz_code = generator.generate(petri_net);

        // 确保 fuzz_targets 目录存在
        let fuzz_targets_dir = self.config.fuzz_targets_dir();
        fs::create_dir_all(&fuzz_targets_dir)?;

        // 写入 fuzz target 文件
        let target_file =
            fuzz_targets_dir.join(format!("{}.rs", self.config.output.fuzz_target_name));
        fs::write(&target_file, fuzz_code)?;

        log::info!("  ✓ Fuzz target 已生成: {}", target_file.display());

        Ok(())
    }

    /// 设置 Fuzz 项目结构
    fn setup_fuzz_project(&self) -> Result<()> {
        let fuzz_dir = self.config.fuzz_dir_path();

        // 确保 fuzz 目录存在
        fs::create_dir_all(&fuzz_dir)?;

        // 生成 Cargo.toml
        self.generate_fuzz_cargo_toml(&fuzz_dir)?;

        // 生成 .gitignore
        self.generate_fuzz_gitignore(&fuzz_dir)?;

        log::info!("  ✓ Fuzz 项目结构已创建: {}", fuzz_dir.display());

        Ok(())
    }

    /// 生成 Fuzz 项目的 Cargo.toml
    fn generate_fuzz_cargo_toml(&self, fuzz_dir: &Path) -> Result<()> {
        let cargo_toml_content = format!(
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

[dependencies.{}]
path = ".."

# Prevent this from interfering with workspaces
[workspace]
members = ["."]

[[bin]]
name = "{}"
path = "fuzz_targets/{}.rs"
test = false
doc = false
"#,
            self.config.target_crate,
            self.config.target_crate,
            self.config.output.fuzz_target_name,
            self.config.output.fuzz_target_name
        );

        let cargo_toml_path = fuzz_dir.join("Cargo.toml");
        fs::write(cargo_toml_path, cargo_toml_content)?;

        Ok(())
    }

    /// 生成 .gitignore
    fn generate_fuzz_gitignore(&self, fuzz_dir: &Path) -> Result<()> {
        let gitignore_content = r#"target
corpus
artifacts
"#;

        let gitignore_path = fuzz_dir.join(".gitignore");
        fs::write(gitignore_path, gitignore_content)?;

        Ok(())
    }
}
