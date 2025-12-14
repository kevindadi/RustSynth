//! 外部大模型客户端模块
//!
//! 支持多个 LLM API 提供商：
//! - OpenAI (GPT-4)
//! - Anthropic (Claude)
//! - DeepSeek

pub mod client;
pub mod prompts;
pub mod code_generator;
pub mod integration;

#[cfg(test)]
mod example;

pub use client::{LlmClient, LlmProvider, LlmConfig, LlmError};
pub use prompts::{TestGenerationPrompt, PromptBuilder};
pub use code_generator::{CodeGenerator, GeneratedTestCase, BatchCodeGenerator};
pub use integration::{PetriNetTestGenerator, create_deepseek_config, create_gpt4_config, create_claude_config};
