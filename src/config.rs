use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// SyPetype: Rust API → Petri Net 转换工具
#[derive(Parser, Debug, Clone)]
#[command(name = "sypetype", version)]
#[command(about = "解析 rustdoc JSON，生成 IR Graph 和 Petri Net")]
#[command(after_help = "\
示例:
  sypetype lib.json                 # 导出 Petri Net (DOT)
  sypetype lib.json -o out -f all   # 导出所有格式到 out/
  sypetype lib.json -e ir -v        # 同时导出 IR Graph，详细输出
  sypetype lib.json -s parse        # 仅解析，打印统计
  sypetype lib.json --fuzz -c foo   # 生成 fuzz 项目 (crate: foo)
")]
pub struct Config {
    /// rustdoc JSON 文件
    #[arg(value_name = "JSON")]
    pub input: PathBuf,

    /// 输出目录
    #[arg(short, long, default_value = "graph")]
    pub output: PathBuf,

    /// 导出格式: dot, json, pnml, all
    #[arg(short, long, value_enum, default_value = "dot")]
    pub format: Format,

    /// 额外导出: ir, all
    #[arg(short, long, value_enum)]
    pub extra: Option<Extra>,

    /// 停止阶段: parse, ir, pn
    #[arg(short, long, value_enum)]
    pub stop: Option<Stage>,

    /// 生成 cargo-fuzz 项目
    #[arg(long)]
    pub fuzz: bool,

    /// 目标 crate 名称 (--fuzz 时使用)
    #[arg(short, long)]
    pub crate_name: Option<String>,

    /// 被测库路径 (--fuzz 时，相对于 fuzz 目录)
    #[arg(long)]
    pub lib_path: Option<String>,

    /// 详细输出
    #[arg(short, long)]
    pub verbose: bool,

    /// 静默模式
    #[arg(short, long)]
    pub quiet: bool,
}

/// 导出格式
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Dot,
    Json,
    Pnml,
    All,
}

/// 额外导出
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Extra {
    /// IR Graph
    Ir,
    /// 所有中间产物
    All,
}

/// 执行阶段
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// 仅解析
    Parse,
    /// 到 IR Graph
    Ir,
    /// 到 Petri Net
    Pn,
}

impl Config {
    /// 目标 crate 名称
    pub fn crate_name(&self) -> String {
        self.crate_name.clone().unwrap_or_else(|| {
            self.input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("my_crate")
                .to_string()
        })
    }

    /// 是否导出 IR Graph
    pub fn export_ir(&self) -> bool {
        matches!(self.extra, Some(Extra::Ir | Extra::All))
    }

    /// 获取 Petri Net 导出格式列表
    pub fn pn_formats(&self) -> Vec<Format> {
        match self.format {
            Format::All => vec![Format::Dot, Format::Json, Format::Pnml],
            f => vec![f],
        }
    }

    /// 计算最终执行阶段
    pub fn final_stage(&self) -> Stage {
        if self.fuzz {
            Stage::Pn // fuzz 需要完整构建
        } else {
            self.stop.unwrap_or(Stage::Pn)
        }
    }

    /// 是否应执行到指定阶段
    pub fn should_run(&self, stage: Stage) -> bool {
        stage <= self.final_stage()
    }

    /// Fuzz 目录路径
    pub fn fuzz_dir(&self) -> PathBuf {
        self.output.join("fuzz")
    }

    /// 日志输出
    pub fn log(&self, msg: &str) {
        if !self.quiet {
            log::info!("{}", msg);
        }
    }

    /// 详细日志
    pub fn log_verbose(&self, msg: &str) {
        if self.verbose && !self.quiet {
            log::info!("{}", msg);
        }
    }
}

