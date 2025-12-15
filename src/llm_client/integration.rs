use crate::llm_client::code_generator::CodeGenerator;
use crate::llm_client::client::{LlmConfig, LlmProvider};
use crate::pushdown_colored_pt_net::unfolding_fuzz::UnfoldingBasedFuzzer;
use crate::pushdown_colored_pt_net::unfolding::UnfoldingConfig;
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

    pub async fn generate_from_unfolding(
        &self,
        pcpn: &PushdownColoredPetriNet,
        unfolding_config: UnfoldingConfig,
        max_sequences: usize,
    ) -> Result<Vec<crate::llm_client::code_generator::GeneratedTestCase>, crate::llm_client::client::LlmError> {
        // 1. 展开 Petri 网
        let fuzzer = UnfoldingBasedFuzzer::new(pcpn, unfolding_config);
        
        // 2. 生成所有序列(限制数量)
        let all_sequences = fuzzer.generate_all_sequences(10);
        let sequences_to_generate = all_sequences.into_iter().take(max_sequences).collect::<Vec<_>>();

        // 3. 为每个序列生成测试用例
        let mut results = Vec::new();
        for sequence in sequences_to_generate {
            match self.code_generator.generate_from_sequence(
                sequence,
                Some(self.crate_name.clone()),
                None,
            ).await {
                Ok(test_case) => results.push(test_case),
                Err(e) => {
                    eprintln!("生成测试用例失败: {}", e);
                    // 继续处理其他序列
                }
            }
        }

        Ok(results)
    }

    /// 生成覆盖所有事件的测试用例
    pub async fn generate_coverage_tests(
        &self,
        pcpn: &PushdownColoredPetriNet,
        unfolding_config: UnfoldingConfig,
    ) -> Result<Vec<crate::llm_client::code_generator::GeneratedTestCase>, crate::llm_client::client::LlmError> {
        let fuzzer = UnfoldingBasedFuzzer::new(pcpn, unfolding_config);
        let sequences = fuzzer.generate_coverage_sequences();

        let mut results = Vec::new();
        for sequence in sequences {
            match self.code_generator.generate_from_sequence(
                sequence,
                Some(self.crate_name.clone()),
                None,
            ).await {
                Ok(test_case) => results.push(test_case),
                Err(e) => {
                    eprintln!("生成测试用例失败: {}", e);
                }
            }
        }

        Ok(results)
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

