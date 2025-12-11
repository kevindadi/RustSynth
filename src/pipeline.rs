//! SyPetype 工作流管道

use crate::config::{Config, Format, Stage};
use crate::ir_graph::builder::IrGraphBuilder;
use crate::ir_graph::structure::IrGraph;
use crate::label_pt_net::net::LabeledPetriNet;
use crate::parse::ParsedCrate;
use crate::petri_net_traits::{ExportFormat, FromIrGraph, PetriNetExport, PetriNetKind};
use anyhow::Result;
use std::fs;

pub struct Pipeline {
    config: Config,
}

impl Pipeline {
    pub fn new(config: Config) -> Self {
        Self { config }
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

        // Step 3: 构建 Petri Net
        let pn_kind = format!("Step 3: Build {:?}", LabeledPetriNet::kind_name(),);
        self.config.log(&pn_kind);
        let pn = self.build_pn(&ir)?;

        // Step 4: 导出 Petri Net
        let pn_desc = format!("Step 4: Export {:?}", LabeledPetriNet::description());
        self.config.log(&pn_desc);
        self.export_pn(&pn)?;

        // Step 5: 生成 Fuzz 项目 (可选)
        if self.config.fuzz {
            self.config.log("Step 5: Generate Fuzz Project");
            self.gen_fuzz()?;
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

    fn build_pn(&self, ir: &IrGraph) -> Result<LabeledPetriNet> {
        let mut pn = LabeledPetriNet::from_ir_graph(ir);
        pn.add_primitive_shims(ir);

        self.config.log_verbose(&format!(
            "  {:?}: {}",
            LabeledPetriNet::kind_name(),
            pn.get_stats_string()
        ));
        Ok(pn)
    }

    fn export_pn(&self, pn: &LabeledPetriNet) -> Result<()> {
        fs::create_dir_all(&self.config.output)?;

        for fmt in self.config.pn_formats() {
            let (ext, export_fmt) = match fmt {
                Format::Dot => ("dot", ExportFormat::Dot),
                Format::Json => ("json", ExportFormat::Json),
                Format::Pnml => ("pnml", ExportFormat::Pnml),
                Format::All => continue, // 已展开
            };

            let path = self.config.output.join(format!("petri_net.{}", ext));
            PetriNetExport::export(pn, &path, export_fmt)?;
            self.config.log(&format!(
                "  ✓ PN {}: {}",
                ext.to_uppercase(),
                path.display()
            ));
        }
        Ok(())
    }

    fn gen_fuzz(&self) -> Result<()> {
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

        // fuzz target
        let target = r#"#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // TODO: 使用 Petri Net 生成的 API 序列
    let _ = data;
});
"#;
        fs::write(targets_dir.join("fuzz_target_1.rs"), target)?;

        self.config
            .log(&format!("  ✓ Fuzz 项目: {}", fuzz_dir.display()));
        Ok(())
    }
}
