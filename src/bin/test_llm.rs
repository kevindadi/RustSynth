//! LLM Client 测试程序
//! 
//! 运行: cargo run --bin test_llm

// 由于 bin 文件无法直接访问 crate 模块,我们需要重新实现测试逻辑
use std::fs;
use reqwest;
use serde_json;
use regex;

async fn test_deepseek_connection() -> Result<(), Box<dyn std::error::Error>> {
    println!("开始测试 DeepSeek API 连接...");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let payload = serde_json::json!({
        "model": "deepseek-chat",
        "messages": [
            {
                "role": "system",
                "content": "你是一个 Rust 编程专家"
            },
            {
                "role": "user",
                "content": "请用 Rust 写一个简单的 hello world 函数"
            }
        ],
        "temperature": 0.7,
        "max_tokens": 2000
    });

    println!("发送测试请求到 DeepSeek API...");
    let response = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", "Bearer sk-2b8baf51868941d69ac8cf835a6a0710")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API 错误: {}", error_text).into());
    }

    let json: serde_json::Value = response.json().await?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("无法解析响应")?;

    println!("✓ API 连接成功！");
    println!("响应长度: {} 字符", content.len());
    println!("响应预览: {}", &content[..content.len().min(200)]);

    fs::write("test_llm_response.txt", content)?;
    println!("✓ 响应已保存到 test_llm_response.txt");

    Ok(())
}

async fn test_code_generation() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n开始测试代码生成功能...");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let system_prompt = r#"你是一个专业的 Rust 代码生成专家.你的任务是根据提供的 API 调用序列生成完整的、可执行的测试用例代码.

要求：
1. 生成的代码必须是完整、可编译的 Rust 代码
2. 使用提供的 API 调用序列,按照顺序生成测试用例
3. 代码应该包含必要的导入语句(use 语句)
4. 为测试函数生成合理的测试数据
5. 添加适当的注释说明测试的目的
6. 如果 API 调用可能失败,应该包含错误处理
7. 生成的代码应该可以直接运行,不需要额外的修改

代码格式要求：
- 使用标准的 Rust 测试格式(#[cfg(test)] 和 #[test])
- 使用合适的命名(测试函数名应该描述测试内容)
- 添加必要的错误处理和断言
- 使用清晰的变量命名

请只返回 Rust 代码,不要包含 markdown 代码块标记(```rust 或 ```),直接返回代码内容."#;

    let user_prompt = r#"## Crate 信息

- Crate 名称: `test_crate`

## 上下文信息

1. 这是一个向量操作的测试
2. 需要测试基本的 push 和 get 操作

## API 调用序列

请按照以下顺序生成测试用例,每个 API 调用都应该在测试中体现：

1. `Vec::new()`
2. `vec.push(42)`
3. `vec.len()`
4. `vec.get(0)`

## 生成要求

请生成一个完整的 Rust 测试函数,满足以下要求：

1. 函数名为 `test_api_sequence`
2. 使用 `#[test]` 属性
3. 按照提供的顺序调用所有 API
4. 为每个 API 调用生成合适的参数
5. 处理可能的错误情况
6. 添加必要的断言来验证结果
7. 包含所有必需的导入语句

请直接返回 Rust 代码,不要包含任何 markdown 格式或代码块标记."#;

    let payload = serde_json::json!({
        "model": "deepseek-chat",
        "messages": [
            {
                "role": "system",
                "content": system_prompt
            },
            {
                "role": "user",
                "content": user_prompt
            }
        ],
        "temperature": 0.7,
        "max_tokens": 4000
    });

    println!("生成测试用例,API 序列: Vec::new(), vec.push(42), vec.len(), vec.get(0)");
    
    let response = client
        .post("https://api.deepseek.com/v1/chat/completions")
        .header("Authorization", "Bearer sk-2b8baf51868941d69ac8cf835a6a0710")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API 错误: {}", error_text).into());
    }

    let json: serde_json::Value = response.json().await?;
    let raw_response = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("无法解析响应")?;

    // 提取代码(移除 markdown 标记)
    let mut code = raw_response.to_string();
    let code_block_re = regex::Regex::new(r"```(?:rust)?\s*\n").unwrap();
    code = code_block_re.replace_all(&code, "").to_string();
    code = code.replace("```", "");
    code = code.trim().to_string();

    println!("✓ 代码生成成功！");
    println!("代码长度: {} 字符", code.len());
    println!("原始响应长度: {} 字符", raw_response.len());

    fs::write("generated_test_case.rs", &code)?;
    println!("✓ 生成的测试代码已保存到 generated_test_case.rs");

    fs::write("generated_test_case_full.txt", raw_response)?;
    println!("✓ 完整响应已保存到 generated_test_case_full.txt");

    println!("\n代码预览(前 500 字符):");
    let preview_len = code.chars().count().min(500);
    let preview: String = code.chars().take(preview_len).collect();
    println!("{}", preview);
    if code.chars().count() > 500 {
        println!("... (还有 {} 字符)", code.chars().count() - 500);
    }

    Ok(())
}

async fn run_all_tests() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "=".repeat(60));
    println!("LLM Client 测试套件");
    println!("{}", "=".repeat(60));

    match test_deepseek_connection().await {
        Ok(_) => println!("\n✓ 测试 1 通过: API 连接测试"),
        Err(e) => {
            eprintln!("\n✗ 测试 1 失败: {}", e);
            return Err(e);
        }
    }

    match test_code_generation().await {
        Ok(_) => println!("\n✓ 测试 2 通过: 代码生成测试"),
        Err(e) => {
            eprintln!("\n✗ 测试 2 失败: {}", e);
            return Err(e);
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("所有测试完成！");
    println!("{}", "=".repeat(60));

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    
    run_all_tests().await?;
    
    Ok(())
}
