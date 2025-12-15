### 1. 基本使用

```rust
use crate::llm_client::code_generator::CodeGenerator;
use crate::llm_client::client::{LlmConfig, LlmProvider};

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
];

let test_case = generator.generate_from_sequence(
    sequence,
    Some("my_crate".to_string()),
    None,
).await?;

println!("生成的代码：\n{}", test_case.code);
```

### 2. 与 Petri 网集成

```rust
use crate::llm_client::integration::PetriNetTestGenerator;
use crate::llm_client::integration::create_deepseek_config;
use crate::pushdown_colored_pt_net::unfolding::UnfoldingConfig;

let llm_config = create_deepseek_config(std::env::var("DEEPSEEK_API_KEY")?);
let generator = PetriNetTestGenerator::new(llm_config, "my_crate".to_string())?;

// 从 Petri 网展开生成测试用例
let unfolding_config = UnfoldingConfig::default();
let test_cases = generator.generate_from_unfolding(
    &pcpn,
    unfolding_config,
    10, // 最多生成 10 个测试用例
).await?;

for (i, test_case) in test_cases.iter().enumerate() {
    println!("测试用例 {}: {} 字符", i, test_case.code.len());
}
```

## 环境变量

需要设置相应的 API key：

- `DEEPSEEK_API_KEY` - DeepSeek API key
- `OPENAI_API_KEY` - OpenAI API key
- `ANTHROPIC_API_KEY` - Anthropic API key

## 提示词设计

提示词模板设计用于：
1. 生成完整、可编译的 Rust 测试代码
2. 按照提供的 API 序列顺序生成
3. 包含必要的导入和错误处理
4. 添加适当的注释和断言

提示词会根据以下信息自动构建：
- Crate 名称和路径
- API 调用序列
- 额外的上下文信息(可选)



