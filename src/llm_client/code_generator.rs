//! 代码生成器
//!
//! 使用 LLM 根据 Petri 网序列生成测试用例代码

use crate::llm_client::client::{LlmClient, LlmConfig, LlmError};
use crate::llm_client::prompts::{TestGenerationPrompt, build_prompt_from_sequence};
use regex::Regex;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct GeneratedTestCase {
    /// 测试代码
    pub code: String,
    pub raw_response: String,
    pub parsed: bool,
}

pub struct CodeGenerator {
    llm_client: LlmClient,
}

impl CodeGenerator {
    pub fn new(config: LlmConfig) -> Result<Self, LlmError> {
        let llm_client = LlmClient::new(config)?;
        Ok(Self { llm_client })
    }

    /// 从 API 序列生成测试用例
    pub async fn generate_from_sequence(
        &self,
        sequence: Vec<String>,
        crate_name: Option<String>,
        additional_context: Option<Vec<String>>,
    ) -> Result<GeneratedTestCase, LlmError> {
        let prompt = build_prompt_from_sequence(sequence, crate_name, additional_context);

        let raw_response = self.llm_client.generate(
            &prompt.user_prompt,
            Some(&prompt.system_prompt),
        ).await?;

        let code = Self::extract_code(&raw_response);
        let parsed = !code.is_empty();

        Ok(GeneratedTestCase {
            code,
            raw_response,
            parsed,
        })
    }

    /// 使用自定义提示词生成测试用例
    pub async fn generate_with_prompt(
        &self,
        prompt: TestGenerationPrompt,
    ) -> Result<GeneratedTestCase, LlmError> {
        let raw_response = self.llm_client.generate(
            &prompt.user_prompt,
            Some(&prompt.system_prompt),
        ).await?;

        let code = Self::extract_code(&raw_response);
        let parsed = !code.is_empty();

        Ok(GeneratedTestCase {
            code,
            raw_response,
            parsed,
        })
    }

    fn extract_code(response: &str) -> String {
        let mut code = response.to_string();

        let code_block_re = Regex::new(r"```(?:rust)?\s*\n").unwrap();
        code = code_block_re.replace_all(&code, "").to_string();
        code = code.replace("```", "");

        code = code.trim().to_string();

        code
    }

    pub fn save_to_file(&self, test_case: &GeneratedTestCase, path: PathBuf) -> std::io::Result<()> {
        std::fs::write(path, &test_case.code)
    }
}

pub struct BatchCodeGenerator {
    generator: CodeGenerator,
    _max_concurrent: usize,
}

impl BatchCodeGenerator {
    pub fn new(config: LlmConfig, max_concurrent: usize) -> Result<Self, LlmError> {
        let generator = CodeGenerator::new(config)?;
        Ok(Self {
            generator,
            _max_concurrent: max_concurrent,
        })
    }

    pub async fn generate_batch(
        &self,
        sequences: Vec<Vec<String>>,
        crate_name: Option<String>,
        additional_context: Option<Vec<String>>,
    ) -> Vec<Result<GeneratedTestCase, LlmError>> {
        use futures::future::join_all;

        let tasks: Vec<_> = sequences
            .into_iter()
            .map(|seq| {
                self.generator.generate_from_sequence(seq, crate_name.clone(), additional_context.clone())
            })
            .collect();

        join_all(tasks).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_code() {
        let response = r#"```rust
fn test() {
    println!("hello");
}
```"#;

        let code = CodeGenerator::extract_code(response);
        assert!(!code.contains("```"));
        assert!(code.contains("fn test()"));
    }

    #[test]
    fn test_extract_code_no_markers() {
        let response = r#"fn test() {
    println!("hello");
}"#;

        let code = CodeGenerator::extract_code(response);
        assert_eq!(code, response);
    }
}
