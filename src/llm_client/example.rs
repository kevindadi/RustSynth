//! LLM 客户端使用示例

use crate::llm_client::code_generator::CodeGenerator;
use crate::llm_client::client::{LlmConfig, LlmProvider};
use crate::llm_client::prompts::build_advanced_prompt;
use crate::pushdown_colored_pt_net::net::PushdownColoredPetriNet;

pub async fn example_deepseek_generation() -> Result<(), Box<dyn std::error::Error>> {
    let config = LlmConfig {
        provider: LlmProvider::DeepSeek,
        api_key: std::env::var("DEEPSEEK_API_KEY")?,
        base_url: None,
        model: "deepseek-chat".to_string(),
        temperature: 0.7,
        max_tokens: Some(4000),
    };

    let generator = CodeGenerator::new(config)?;

    let sequence = vec![
        "Vec::new()".to_string(),
        "vec.push(42)".to_string(), 
        "vec.len()".to_string(),
    ];

    let test_case = generator
        .generate_from_sequence(
            sequence,
            Some("my_crate".to_string()),
            Some(vec!["这是一个向量操作的测试".to_string()]),
        )
        .await?;

    println!("生成的测试代码：");
    println!("{}", test_case.code);

    Ok(())
}

pub async fn example_gpt4_generation() -> Result<(), Box<dyn std::error::Error>> {
    let config = LlmConfig {
        provider: LlmProvider::OpenAi,
        api_key: std::env::var("OPENAI_API_KEY")?,
        base_url: None,
        model: "gpt-4".to_string(),
        temperature: 0.7,
        max_tokens: Some(4000),
    };

    let generator = CodeGenerator::new(config)?;

    let sequence = vec![
        "String::from(\"hello\")".to_string(),
        "s.push_str(\" world\")".to_string(),
    ];

    let test_case = generator
        .generate_from_sequence(sequence, Some("my_crate".to_string()), None)
        .await?;

    println!("生成的测试代码：");
    println!("{}", test_case.code);

    Ok(())
}

pub async fn example_claude_generation() -> Result<(), Box<dyn std::error::Error>> {
    let config = LlmConfig {
        provider: LlmProvider::Claude,
        api_key: std::env::var("ANTHROPIC_API_KEY")?,
        base_url: None,
        model: "claude-3-opus-20240229".to_string(),
        temperature: 0.7,
        max_tokens: Some(4096),
    };

    let generator = CodeGenerator::new(config)?;

    let pcpn: Option<&PushdownColoredPetriNet> = None;
    let prompt = build_advanced_prompt(
        vec!["MyStruct::new()".to_string(), "s.do_something()".to_string()],
        pcpn,
        "my_crate".to_string(),
        Some("./my_crate".to_string()),
    );

    let test_case = generator.generate_with_prompt(prompt).await?;

    println!("生成的测试代码：");
    println!("{}", test_case.code);

    Ok(())
}

/// 示例：从 Petri 网序列批量生成测试用例
pub async fn example_batch_generation(
    sequences: Vec<Vec<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::llm_client::code_generator::BatchCodeGenerator;

    let config = LlmConfig {
        provider: LlmProvider::DeepSeek,
        api_key: std::env::var("DEEPSEEK_API_KEY")?,
        base_url: None,
        model: "deepseek-chat".to_string(),
        temperature: 0.7,
        max_tokens: Some(4000),
    };

    let batch_generator = BatchCodeGenerator::new(config, 5)?; // 最多 5 个并发

    let results = batch_generator
        .generate_batch(
            sequences,
            Some("my_crate".to_string()),
            None,
        )
        .await;

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(test_case) => {
                println!("序列 {} 生成成功", i);
                println!("代码长度: {} 字符", test_case.code.len());
            }
            Err(e) => {
                eprintln!("序列 {} 生成失败: {}", i, e);
            }
        }
    }

    Ok(())
}
