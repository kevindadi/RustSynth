use anyhow::Result;
mod config;
pub mod ir_graph; // 改为 pub 以便 bin 文件访问
pub mod llm_client;
pub mod parse; // 改为 pub 以便 bin 文件访问
mod petri_net_traits;
mod pipeline;
pub mod pushdown_colored_pt_net;
pub mod support_types;

use crate::config::Config;
use crate::pipeline::Pipeline;
use clap::Parser;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::parse();

    let pipeline = Pipeline::new(config);
    pipeline.run()?;

    Ok(())
}
