/// SyPetype 完整工作流管道
use crate::config::{Config, PipelineStage};
// TODO: 更新 generate 模块以适配 LabeledPetriNet
// use crate::generate::FuzzTargetGenerator;
use crate::ir_graph::builder::IrGraphBuilder;
use crate::ir_graph::structure::IrGraph;
use crate::label_pt_net::net::LabeledPetriNet;
use crate::petri_net_traits::PetriNetKind;
use crate::parse::ParsedCrate;
use crate::petri_net_traits::{FromIrGraph, PetriNetExport};
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

        if self.config.should_stop_at(PipelineStage::Parse) {
            log::info!("停止在阶段: Parse");
            return Ok(());
        }

        // Step 2: Build IR Graph
        log::info!("Step 2: Build IR Graph");
        let ir_graph = self.build_ir_graph(parsed_crate)?;

        if self.config.should_stop_at(PipelineStage::IrGraph) {
            log::info!("停止在阶段: IrGraph");
            ir_graph.print_stats();
            return Ok(());
        }

        // Step 3: Export IR Graph (if enabled)
        if self.config.should_execute(PipelineStage::ExportIrGraph)
            && (self.config.export.export_ir_graph_dot || self.config.export.export_ir_graph_json)
        {
            log::info!("Step 3: Export IR Graph");
            self.export_ir_graph(&ir_graph)?;
        }

        if self.config.should_stop_at(PipelineStage::ExportIrGraph) {
            log::info!("停止在阶段: ExportIrGraph");
            return Ok(());
        }

        // Step 4: Convert to PT-Net (Place/Transition Net)
        if self.config.should_execute(PipelineStage::PetriNet) {
            log::info!("Step 4: Convert to {:?}", LabeledPetriNet::kind_name());
            log::info!("  {:?}", LabeledPetriNet::description());
            let petri_net = self.build_petri_net(&ir_graph)?;

            // Step 5: Export PT-Net
            if self.config.should_execute(PipelineStage::ExportPetriNet)
                && self.config.export.should_export_petri_net()
            {
                log::info!("Step 5: Export PT-Net");
                self.export_petri_net(&petri_net)?;
            }

            if self.config.should_stop_at(PipelineStage::ExportPetriNet) {
                log::info!("停止在阶段: ExportPetriNet");
                return Ok(());
            }

            // Step 6: Generate Fuzz Target
            // TODO: 更新 generate 模块以适配 LabeledPetriNet
            // if self.config.should_execute(PipelineStage::FuzzTarget) {
            //     log::info!("Step 6: Generate Fuzz Target");
            //     self.generate_fuzz_target(&petri_net)?;
            // }

            // if self.config.should_stop_at(PipelineStage::FuzzTarget) {
            //     log::info!("停止在阶段: FuzzTarget");
            //     return Ok(());
            // }

            // Step 7: Generate Fuzz Project Structure
            // TODO: 更新 generate 模块以适配 LabeledPetriNet
            // if self.config.should_execute(PipelineStage::FuzzProject) {
            //     log::info!("Step 7: Generate Fuzz Project Structure");
            //     self.setup_fuzz_project()?;
            // }
        } else {
            log::info!("跳过 PT-Net 构建阶段");
        }

        log::info!("SyPetype workflow completed");
        Ok(())
    }

    /// 解析 rustdoc JSON 文件
    fn parse_json(&self) -> Result<ParsedCrate> {
        log::info!("  从文件加载: {}", self.config.input_json.display());

        let parsed_crate = ParsedCrate::from_json_file(&self.config.input_json)
            .map_err(|e| anyhow::anyhow!("无法解析 rustdoc JSON 文件: {}", e))?;

        if self.config.stats {
            parsed_crate.print_stats();
        }
        Ok(parsed_crate)
    }

    /// 构建 IR Graph
    fn build_ir_graph(&self, parsed_crate: ParsedCrate) -> Result<IrGraph> {
        let builder = IrGraphBuilder::new(&parsed_crate);
        let ir_graph = builder.build();

        if self.config.stats {
            log::info!("  IR Graph 统计:");
            log::info!("    节点数: {}", ir_graph.type_graph.node_count());
            log::info!("    边数: {}", ir_graph.type_graph.edge_count());
        }

        Ok(ir_graph)
    }

    /// 导出 IR Graph
    fn export_ir_graph(&self, ir_graph: &IrGraph) -> Result<()> {
        fs::create_dir_all(&self.config.output_dir)?;

        let parsed_crate = ParsedCrate::from_json_file(&self.config.input_json)
            .map_err(|e| anyhow::anyhow!("无法解析 rustdoc JSON 文件: {}", e))?;

        if self.config.export.export_ir_graph_dot {
            let dot_path = self
                .config
                .output_dir
                .join(&self.config.export.ir_graph_dot_name);
            ir_graph.export_dot(&parsed_crate, &dot_path)?;
            log::info!("  ✓ IR Graph DOT 已导出: {}", dot_path.display());
        }

        if self.config.export.export_ir_graph_json {
            let json_path = self
                .config
                .output_dir
                .join(&self.config.export.ir_graph_json_name);
            ir_graph.export_json(&json_path)?;
            log::info!("  ✓ IR Graph JSON 已导出: {}", json_path.display());
        }

        Ok(())
    }

    fn build_petri_net(&self, ir_graph: &IrGraph) -> Result<LabeledPetriNet> {
        let petri_net = LabeledPetriNet::from_ir_graph(ir_graph);

        if self.config.stats {
            let stats = petri_net.get_stats_string();
            log::info!("  {:?} 统计: {}", LabeledPetriNet::kind_name(), stats);
        }

        Ok(petri_net)
    }

    fn export_petri_net(&self, petri_net: &LabeledPetriNet) -> Result<()> {
        fs::create_dir_all(&self.config.output_dir)?;

        use crate::config::PetriNetFormat;
        use crate::petri_net_traits::ExportFormat;

        let format = match self.config.export.petri_net_format {
            PetriNetFormat::Pnml => ExportFormat::Pnml,
            PetriNetFormat::Dot => ExportFormat::Dot,
            PetriNetFormat::Json => ExportFormat::Json,
        };

        let filename = self.config.export.petri_net_filename();
        let output_path = self.config.output_dir.join(&filename);

        PetriNetExport::export(petri_net, &output_path, format)?;
        log::info!(
            "  ✓ {:?} {:?} 已导出: {}",
            LabeledPetriNet::kind_name(),
            match self.config.export.petri_net_format {
                PetriNetFormat::Pnml => "PNML",
                PetriNetFormat::Dot => "DOT",
                PetriNetFormat::Json => "JSON",
            },
            output_path.display()
        );

        Ok(())
    }

    // /// 生成 Fuzz Target
    // /// TODO: 更新以适配 LabeledPetriNet
    // fn generate_fuzz_target(&self, petri_net: &LabeledPetriNet) -> Result<()> {
    //     let generator = FuzzTargetGenerator::new(self.config.target_crate.clone());
    //     let fuzz_code = generator.generate(petri_net);

    //     let fuzz_targets_dir = self.config.fuzz_targets_dir();
    //     fs::create_dir_all(&fuzz_targets_dir)?;

    //     let target_file =
    //         fuzz_targets_dir.join(format!("{}.rs", self.config.fuzz_target_name));
    //     fs::write(&target_file, fuzz_code)?;

    //     log::info!("  ✓ Fuzz target 已生成: {}", target_file.display());

    //     Ok(())
    // }

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
            // 默认:假设库在上一级目录
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
            self.config.fuzz_target_name,
            self.config.fuzz_target_name
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
