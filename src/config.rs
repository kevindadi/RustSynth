/// 配置管理模块
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub input_json: PathBuf,

    /// 目标 crate 名称(用于生成 fuzz target)
    pub target_crate: String,

    /// 被测库的路径(相对于 fuzz 目录,用于 Cargo.toml 依赖)
    /// 如果为 None,则使用 crates.io 依赖
    pub lib_path: Option<String>,

    pub output: OutputConfig,
    pub export: ExportConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// 输出目录(所有生成文件的根目录)
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,

    /// Fuzz target 输出目录(相对于 output_dir)
    #[serde(default = "default_fuzz_dir")]
    pub fuzz_dir: PathBuf,

    /// 生成的 fuzz target 名称
    #[serde(default = "default_fuzz_target")]
    pub fuzz_target_name: String,
}

/// 导出配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    /// IR Graph Export
    #[serde(default)]
    pub export_ir_graph_dot: bool,
    #[serde(default = "default_ir_dot_name")]
    pub ir_graph_dot_name: String,
    #[serde(default)]
    pub export_ir_graph_json: bool,
    #[serde(default = "default_ir_json_name")]
    pub ir_graph_json_name: String,

    /// PT-Net (Place/Transition Net) Export
    #[serde(default)]
    pub export_petri_net_dot: bool,
    #[serde(default = "default_petri_dot_name")]
    pub petri_net_dot_name: String,
    #[serde(default)]
    pub export_petri_net_json: bool,
    #[serde(default = "default_petri_json_name")]
    pub petri_net_json_name: String,

    /// CP-Net (Colored Petri Net with Trait Hub) Export
    #[serde(default)]
    pub export_cp_net_dot: bool,
    #[serde(default = "default_cp_net_dot_name")]
    pub cp_net_dot_name: String,
    #[serde(default)]
    pub export_cp_net_json: bool,
    #[serde(default = "default_cp_net_json_name")]
    pub cp_net_json_name: String,

    #[serde(default = "default_true")]
    pub print_stats: bool,

    #[serde(default)]
    pub print_type_summary: bool,
}

fn default_output_dir() -> PathBuf {
    PathBuf::from(".")
}

fn default_fuzz_dir() -> PathBuf {
    PathBuf::from("fuzz")
}

fn default_fuzz_target() -> String {
    "fuzz_target_1".to_string()
}

fn default_ir_dot_name() -> String {
    "ir_graph.dot".to_string()
}

fn default_ir_json_name() -> String {
    "ir_graph.json".to_string()
}

fn default_petri_dot_name() -> String {
    "petri_net.dot".to_string()
}

fn default_petri_json_name() -> String {
    "petri_net.json".to_string()
}

fn default_cp_net_dot_name() -> String {
    "cp_net.dot".to_string()
}

fn default_cp_net_json_name() -> String {
    "cp_net.json".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input_json: PathBuf::from("./target/doc/my_crate.json"),
            target_crate: "my_crate".to_string(),
            lib_path: None,
            output: OutputConfig::default(),
            export: ExportConfig::default(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            fuzz_dir: default_fuzz_dir(),
            fuzz_target_name: default_fuzz_target(),
        }
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            export_ir_graph_dot: false,
            ir_graph_dot_name: default_ir_dot_name(),
            export_ir_graph_json: false,
            ir_graph_json_name: default_ir_json_name(),
            export_petri_net_dot: true,
            petri_net_dot_name: default_petri_dot_name(),
            export_petri_net_json: false,
            petri_net_json_name: default_petri_json_name(),
            export_cp_net_dot: false,
            cp_net_dot_name: default_cp_net_dot_name(),
            export_cp_net_json: false,
            cp_net_json_name: default_cp_net_json_name(),
            print_stats: false,
            print_type_summary: false,
        }
    }
}

impl Config {
    /// 从 TOML 文件加载配置
    pub fn from_toml_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// 保存配置到 TOML 文件
    pub fn save_toml(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 获取完整的 fuzz 目录路径
    pub fn fuzz_dir_path(&self) -> PathBuf {
        self.output.output_dir.join(&self.output.fuzz_dir)
    }

    /// 获取 fuzz targets 目录路径
    pub fn fuzz_targets_dir(&self) -> PathBuf {
        self.fuzz_dir_path().join("fuzz_targets")
    }
}
