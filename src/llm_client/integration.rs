use crate::llm_client::code_generator::CodeGenerator;
use crate::llm_client::client::{LlmConfig, LlmProvider};
use crate::pushdown_colored_pt_net::net::PushdownColoredPetriNet;

pub struct PetriNetTestGenerator {
    code_generator: CodeGenerator,
    crate_name: String,
}

impl PetriNetTestGenerator {
    pub fn new(
        llm_config: LlmConfig,
        crate_name: String,
    ) -> Result<Self, crate::llm_client::client::LlmError> {
        let code_generator = CodeGenerator::new(llm_config)?;
        Ok(Self {
            code_generator,
            crate_name,
        })
    }
}

pub fn create_deepseek_config(api_key: String) -> LlmConfig {
    LlmConfig {
        provider: LlmProvider::DeepSeek,
        api_key,
        base_url: None,
        model: "deepseek-chat".to_string(),
        temperature: 0.7,
        max_tokens: Some(4000),
    }
}

pub fn create_gpt4_config(api_key: String) -> LlmConfig {
    LlmConfig {
        provider: LlmProvider::OpenAi,
        api_key,
        base_url: None,
        model: "gpt-4".to_string(),
        temperature: 0.7,
        max_tokens: Some(4000),
    }
}

pub fn create_claude_config(api_key: String) -> LlmConfig {
    LlmConfig {
        provider: LlmProvider::Claude,
        api_key,
        base_url: None,
        model: "claude-3-opus-20240229".to_string(),
        temperature: 0.7,
        max_tokens: Some(4096),
    }
}

