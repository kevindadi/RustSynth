#![allow(unused)]
use clap::{ArgGroup, Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "sypetype")]
#[command(about = "SyPetype: 解析 Rust API 文档并生成 IR Graph 和 Petri Net")]
#[command(
    long_about = "SyPetype 工具用于解析 Rust 项目的 rustdoc JSON 输出，构建中间表示图(IR Graph)，
并转换为 Petri Net 用于分析和测试。

使用示例:
  # 基本用法：解析 JSON 并构建 IR Graph
  sypetype input.json

  # 导出 IR Graph 为 DOT 格式
  sypetype input.json --export-ir-graph-dot

  # 构建 Petri Net 并导出为 DOT 格式（默认）
  sypetype input.json --stop-at export-petri-net --petri-net-format dot

  # 导出为 JSON 格式
  sypetype input.json --stop-at export-petri-net --petri-net-format json

  # 打印统计信息
  sypetype input.json --stats

输入文件:
  需要提供 rustdoc 生成的 JSON 文件，通常通过以下命令生成:
    cargo +nightly rustdoc -- --output-format json
"
)]
#[command(version)]
pub struct Config {
    /// rustdoc JSON 文件路径
    /// 
    /// 这是通过 'cargo +nightly rustdoc -- --output-format json' 生成的 JSON 文件
    #[arg(value_name = "INPUT", required = true)]
    pub input_json: PathBuf,

    /// 输出目录(默认为当前目录)
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub output_dir: PathBuf,

    /// 目标 crate 名称(用于生成 fuzz target)
    #[arg(long, value_name = "NAME", default_value = "my_crate")]
    pub target_crate: String,

    /// 被测库的路径(相对于 fuzz 目录,用于 Cargo.toml 依赖)
    /// 如果未指定,则使用上一级目录
    #[arg(long, value_name = "PATH")]
    pub lib_path: Option<String>,

    /// 生成的 fuzz target 名称
    #[arg(long, value_name = "NAME", default_value = "fuzz_target_1")]
    pub fuzz_target_name: String,

    /// Fuzz 目录(相对于 output_dir)
    #[arg(long, value_name = "DIR", default_value = "fuzz")]
    pub fuzz_dir: PathBuf,

    /// 执行阶段控制
    /// 
    /// 指定执行到哪个阶段后停止。可选值:
    /// - parse: 仅解析 JSON
    /// - ir-graph: 构建 IR Graph
    /// - export-ir-graph: 导出 IR Graph
    /// - petri-net: 构建 PT-Net
    /// - export-petri-net: 导出 PT-Net
    /// - fuzz-target: 生成 Fuzz Target
    /// - fuzz-project: 生成 Fuzz 项目结构
    #[arg(
        long,
        value_enum,
        default_value = "export-petri-net",
        help = "指定执行到哪个阶段后停止"
    )]
    pub stop_at: PipelineStage,

    /// 打印统计信息
    #[arg(long)]
    pub stats: bool,

    /// 打印类型摘要
    #[arg(long)]
    pub print_type_summary: bool,

    /// 导出选项组
    #[command(flatten)]
    pub export: ExportConfig,
}

/// 导出配置
#[derive(Parser, Debug, Clone)]
pub struct ExportConfig {
    /// 导出 IR Graph DOT 文件
    #[arg(long)]
    pub export_ir_graph_dot: bool,

    /// IR Graph DOT 文件名
    #[arg(long, value_name = "NAME", default_value = "ir_graph.dot")]
    pub ir_graph_dot_name: String,

    /// 导出 IR Graph JSON 文件
    #[arg(long)]
    pub export_ir_graph_json: bool,

    /// IR Graph JSON 文件名
    #[arg(long, value_name = "NAME", default_value = "ir_graph.json")]
    pub ir_graph_json_name: String,

    /// PT-Net 导出格式
    /// 
    /// 选择 Petri Net 的导出格式，只能选择一种:
    /// - dot: Graphviz DOT 格式（默认，用于可视化）
    /// - json: JSON 格式（用于程序处理）
    #[arg(
        long,
        value_enum,
        default_value = "dot",
        help = "PT-Net 导出格式，只能选择一种: dot (默认) 或 json"
    )]
    pub petri_net_format: PetriNetFormat,

    /// PT-Net 文件名（不含扩展名）
    /// 
    /// 扩展名会根据 --petri-net-format 自动添加
    #[arg(long, value_name = "NAME", default_value = "petri_net")]
    pub petri_net_name: String,
}

/// Petri Net 导出格式
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum PetriNetFormat {
    Pnml,
    Dot,
    Json,
}

impl ExportConfig {
    /// 获取 PT-Net 的完整文件名（包含扩展名）
    pub fn petri_net_filename(&self) -> String {
        let ext = match self.petri_net_format {
            PetriNetFormat::Pnml => "pnml",
            PetriNetFormat::Dot => "dot",
            PetriNetFormat::Json => "json",
        };
        format!("{}.{}", self.petri_net_name, ext)
    }

    /// 检查是否应该导出 PT-Net
    pub fn should_export_petri_net(&self) -> bool {
        // 如果指定了格式，就导出
        true // 默认导出，因为格式有默认值
    }
}

/// Pipeline 执行阶段
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, PartialOrd, Ord)]
pub enum PipelineStage {
    /// 仅解析 JSON
    Parse,
    /// 构建 IR Graph
    IrGraph,
    /// 导出 IR Graph
    ExportIrGraph,
    /// 构建 PT-Net
    PetriNet,
    /// 导出 PT-Net
    ExportPetriNet,
    /// 生成 Fuzz Target
    FuzzTarget,
    /// 生成 Fuzz 项目结构
    FuzzProject,
}

impl Config {
    /// 获取完整的 fuzz 目录路径
    pub fn fuzz_dir_path(&self) -> PathBuf {
        self.output_dir.join(&self.fuzz_dir)
    }

    /// 获取 fuzz targets 目录路径
    pub fn fuzz_targets_dir(&self) -> PathBuf {
        self.fuzz_dir_path().join("fuzz_targets")
    }

    /// 检查是否应该停止在指定阶段
    pub fn should_stop_at(&self, stage: PipelineStage) -> bool {
        // 如果当前阶段等于或大于停止阶段，则停止
        stage >= self.stop_at
    }

    /// 检查是否应该执行指定阶段
    pub fn should_execute(&self, stage: PipelineStage) -> bool {
        // 如果阶段小于等于停止阶段，则执行
        stage <= self.stop_at
    }
}
