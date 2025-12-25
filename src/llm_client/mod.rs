//! 外部大模型客户端模块
//!
//! - GPT
//! - Claude
//! - DeepSeek
//! - Qwen

pub mod client;
pub mod code_generator;
pub mod integration;
pub mod prompts;

pub use client::{LlmClient, LlmConfig, LlmError, LlmProvider};
pub use code_generator::{BatchCodeGenerator, CodeGenerator, GeneratedTestCase};
pub use integration::{
    PetriNetTestGenerator, create_claude_config, create_deepseek_config, create_gpt4_config,
};
pub use prompts::{PromptBuilder, TestGenerationPrompt};
