/// SyPetype 完整工作流管道
use crate::config::Config;
use crate::cp_net::structure::CpPetriNet;
use crate::generate::FuzzTargetGenerator;
use crate::ir_graph::builder::build_ir_graph;
use crate::ir_graph::structure::IrGraph;
use crate::parse::ParsedCrate;
use crate::petri_net_traits::{FromIrGraph, PetriNetExport};
use crate::pt_net::structure::PetriNet;
use anyhow::Result;
use std::fs;
use std::path::Path;

/// SyPetype 工作流执行器
pub struct Pipeline {
    config: Config,
}

impl Pipeline {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn run(&self) -> Result<()> {
        // Step 1: 解析 rustdoc JSON
        log::info!("Step 1: Parse rustdoc JSON");
        let parsed_crate = self.parse_json()?;

        // Step 2: Build IR Graph
        log::info!("Step 2: Build IR Graph");
        let ir_graph = self.build_ir_graph(parsed_crate)?;

        // Step 3: Export IR Graph (if enabled)
        if self.config.export.export_ir_graph_dot || self.config.export.export_ir_graph_json {
            log::info!("Step 3: Export IR Graph");
            self.export_ir_graph(&ir_graph)?;
        }

        // Step 4: Convert to PT-Net (Place/Transition Net)
        log::info!("Step 4: Convert to PT-Net");
        let petri_net = self.build_petri_net(&ir_graph)?;

        // Step 5: Export PT-Net (if enabled)
        if self.config.export.export_petri_net_dot || self.config.export.export_petri_net_json {
            log::info!("Step 5: Export PT-Net");
            self.export_petri_net(&petri_net)?;
        }

        // Step 6: Convert to CP-Net (Colored Petri Net with Trait Hub)
        if self.config.export.export_cp_net_dot || self.config.export.export_cp_net_json {
            log::info!("Step 6: Convert to CP-Net");
            let cp_net = self.build_cp_net(&ir_graph)?;

            log::info!("Step 6.1: Export CP-Net");
            self.export_cp_net(&cp_net)?;
        }

        // Step 7: Generate Fuzz Target
        log::info!("Step 7: Generate Fuzz Target");
        self.generate_fuzz_target(&petri_net)?;

        // Step 8: Generate Fuzz Project Structure
        log::info!("Step 8: Generate Fuzz Project Structure");
        self.setup_fuzz_project()?;

        log::info!("SyPetype workflow completed");
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

        if self.config.export.print_type_summary {
            parsed_crate.print_type_trait_summary();
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
        fs::create_dir_all(&self.config.output.output_dir)?;

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

    fn build_petri_net(&self, ir_graph: &IrGraph) -> Result<PetriNet> {
        let petri_net = PetriNet::from_ir_graph(ir_graph);

        if self.config.export.print_stats {
            log::info!("  {}", petri_net.get_stats_string());
        }

        Ok(petri_net)
    }

    fn export_petri_net(&self, petri_net: &PetriNet) -> Result<()> {
        fs::create_dir_all(&self.config.output.output_dir)?;

        if self.config.export.export_petri_net_dot {
            let dot_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.petri_net_dot_name);
            petri_net.export_dot(&dot_path)?;
            log::info!("  ✓ PT-Net DOT 已导出: {}", dot_path.display());
        }

        if self.config.export.export_petri_net_json {
            let json_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.petri_net_json_name);
            petri_net.export_json(&json_path)?;
            log::info!("  ✓ PT-Net JSON 已导出: {}", json_path.display());
        }

        Ok(())
    }

    /// 构建 CP-Net（Colored Petri Net with Trait Hub）
    fn build_cp_net(&self, ir_graph: &IrGraph) -> Result<CpPetriNet> {
        let cp_net = CpPetriNet::from_ir_graph(ir_graph);

        if self.config.export.print_stats {
            log::info!("  CP-Net 统计:");
            cp_net.print_stats();
        }

        Ok(cp_net)
    }

    /// 导出 CP-Net
    fn export_cp_net(&self, cp_net: &CpPetriNet) -> Result<()> {
        fs::create_dir_all(&self.config.output.output_dir)?;

        if self.config.export.export_cp_net_dot {
            let dot_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.cp_net_dot_name);
            cp_net.export_dot(&dot_path)?;
            log::info!("  ✓ CP-Net DOT 已导出: {}", dot_path.display());
        }

        if self.config.export.export_cp_net_json {
            let json_path = self
                .config
                .output
                .output_dir
                .join(&self.config.export.cp_net_json_name);
            cp_net.export_json(&json_path)?;
            log::info!("  ✓ CP-Net JSON 已导出: {}", json_path.display());
        }

        Ok(())
    }

    /// 生成 Fuzz Target
    fn generate_fuzz_target(&self, petri_net: &PetriNet) -> Result<()> {
        let generator = FuzzTargetGenerator::new(self.config.target_crate.clone());
        let fuzz_code = generator.generate(petri_net);

        let fuzz_targets_dir = self.config.fuzz_targets_dir();
        fs::create_dir_all(&fuzz_targets_dir)?;

        let target_file =
            fuzz_targets_dir.join(format!("{}.rs", self.config.output.fuzz_target_name));
        fs::write(&target_file, fuzz_code)?;

        log::info!("  ✓ Fuzz target 已生成: {}", target_file.display());

        Ok(())
    }

    /// 设置 Fuzz 项目结构
    fn setup_fuzz_project(&self) -> Result<()> {
        let fuzz_dir = self.config.fuzz_dir_path();

        fs::create_dir_all(&fuzz_dir)?;
        self.generate_fuzz_cargo_toml(&fuzz_dir)?;
        self.generate_fuzz_gitignore(&fuzz_dir)?;

        log::info!("  ✓ Fuzz 项目结构已创建: {}", fuzz_dir.display());

        Ok(())
    }

    /// 生成 Fuzz 项目的 Cargo.toml
    fn generate_fuzz_cargo_toml(&self, fuzz_dir: &Path) -> Result<()> {
        // 构建依赖部分
        let lib_dependency = if let Some(ref lib_path) = self.config.lib_path {
            // 使用用户指定的路径
            format!(
                "[dependencies.{}]\npath = \"{}\"\nfeatures = [\"std\"]",
                self.config.target_crate, lib_path
            )
        } else {
            // 默认：假设库在上一级目录
            format!("[dependencies.{}]\npath = \"..\"", self.config.target_crate)
        };

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

{}

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
            lib_dependency,
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
