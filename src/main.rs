use anyhow::Result;
mod config;
// mod generate;  // TODO: 更新以适配 LabeledPetriNet
pub mod ir_graph;  // 改为 pub 以便 bin 文件访问
pub mod label_pt_net;
pub mod pushdown_colored_pt_net;
pub mod parse;  // 改为 pub 以便 bin 文件访问
mod petri_net_traits;
mod pipeline;
pub mod support_types;
pub mod llm_client;

use crate::config::Config;
use crate::pipeline::Pipeline;
use clap::Parser;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::parse();

    // 使用 Pipeline 执行工作流
    let pipeline = Pipeline::new(config);
    pipeline.run()?;

    Ok(())
}
