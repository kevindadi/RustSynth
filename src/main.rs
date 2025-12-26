use anyhow::Result;
mod config;
pub mod ir_graph; 
pub mod pcpn;  // 新的下推着色 Petri 网模块
pub mod parse; 
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
