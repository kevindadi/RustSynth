use crate::pushdown_colored_pt_net::net::PushdownColoredPetriNet;

pub struct TestGenerationPrompt {
    pub system_prompt: String,
    pub user_prompt: String,
}

pub struct PromptBuilder {
    crate_name: Option<String>,
    crate_path: Option<String>,
    api_sequence: Vec<String>,
    context_info: Vec<String>,
    target_language: String,
}

impl PromptBuilder {
    pub fn new() -> Self {
        Self {
            crate_name: None,
            crate_path: None,
            api_sequence: Vec::new(),
            context_info: Vec::new(),
            target_language: "rust".to_string(),
        }
    }

    pub fn with_crate_name(mut self, name: String) -> Self {
        self.crate_name = Some(name);
        self
    }

    pub fn with_crate_path(mut self, path: String) -> Self {
        self.crate_path = Some(path);
        self
    }

    pub fn with_api_sequence(mut self, sequence: Vec<String>) -> Self {
        self.api_sequence = sequence;
        self
    }

    pub fn add_context(mut self, info: String) -> Self {
        self.context_info.push(info);
        self
    }

    pub fn with_language(mut self, lang: String) -> Self {
        self.target_language = lang;
        self
    }

    pub fn build(self) -> TestGenerationPrompt {
        let system_prompt = self.build_system_prompt();
        let user_prompt = self.build_user_prompt();
        
        TestGenerationPrompt {
            system_prompt,
            user_prompt,
        }
    }

    fn build_system_prompt(&self) -> String {
        format!(
            r#"你是一个专业的 Rust 代码生成专家。你的任务是根据提供的 API 调用序列生成完整的、可执行的测试用例代码。

要求：
1. 生成的代码必须是完整、可编译的 Rust 代码
2. 使用提供的 API 调用序列，按照顺序生成测试用例
3. 代码应该包含必要的导入语句（use 语句）
4. 为测试函数生成合理的测试数据
5. 添加适当的注释说明测试的目的
6. 如果 API 调用可能失败，应该包含错误处理
7. 生成的代码应该可以直接运行，不需要额外的修改

代码格式要求：
- 使用标准的 Rust 测试格式（#[cfg(test)] 和 #[test]）
- 使用合适的命名（测试函数名应该描述测试内容）
- 添加必要的错误处理和断言
- 使用清晰的变量命名

请只返回 Rust 代码，不要包含 markdown 代码块标记（```rust 或 ```），直接返回代码内容。"#
        )
    }

    fn build_user_prompt(&self) -> String {
        let mut prompt = String::new();

        // Crate 信息
        if let Some(ref crate_name) = self.crate_name {
            prompt.push_str(&format!("## Crate 信息\n\n"));
            prompt.push_str(&format!("- Crate 名称: `{}`\n", crate_name));
            if let Some(ref crate_path) = self.crate_path {
                prompt.push_str(&format!("- Crate 路径: `{}`\n", crate_path));
            }
            prompt.push_str("\n");
        }

        // 上下文信息
        if !self.context_info.is_empty() {
            prompt.push_str("## 上下文信息\n\n");
            for (i, info) in self.context_info.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, info));
            }
            prompt.push_str("\n");
        }

        // API 序列
        prompt.push_str("## API 调用序列\n\n");
        prompt.push_str("请按照以下顺序生成测试用例，每个 API 调用都应该在测试中体现：\n\n");
        for (i, api_call) in self.api_sequence.iter().enumerate() {
            prompt.push_str(&format!("{}. `{}`\n", i + 1, api_call));
        }
        prompt.push_str("\n");

        // 生成要求
        prompt.push_str("## 生成要求\n\n");
        prompt.push_str(r#"请生成一个完整的 Rust 测试函数，满足以下要求：

1. 函数名为 `test_api_sequence`
2. 使用 `#[test]` 属性
3. 按照提供的顺序调用所有 API
4. 为每个 API 调用生成合适的参数
5. 处理可能的错误情况
6. 添加必要的断言来验证结果
7. 包含所有必需的导入语句

请直接返回 Rust 代码，不要包含任何 markdown 格式或代码块标记。"#);

        prompt
    }
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_prompt_from_sequence(
    sequence: Vec<String>,
    crate_name: Option<String>,
    additional_context: Option<Vec<String>>,
) -> TestGenerationPrompt {
    let mut builder = PromptBuilder::new()
        .with_api_sequence(sequence);

    if let Some(name) = crate_name {
        builder = builder.with_crate_name(name);
    }

    if let Some(context) = additional_context {
        for info in context {
            builder = builder.add_context(info);
        }
    }

    builder.build()
}

pub fn build_advanced_prompt(
    sequence: Vec<String>,
    pcpn: Option<&PushdownColoredPetriNet>,
    crate_name: String,
    crate_path: Option<String>,
) -> TestGenerationPrompt {
    let mut builder = PromptBuilder::new()
        .with_crate_name(crate_name)
        .with_api_sequence(sequence);

    if let Some(path) = crate_path {
        builder = builder.with_crate_path(path);
    }

    if let Some(net) = pcpn {
        let stats = net.stats();
        builder = builder.add_context(format!(
            "Petri 网包含 {} 个 places 和 {} 个 transitions，有 {} 种不同的 token 颜色",
            stats.place_count, stats.transition_count, stats.color_count
        ));
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_builder() {
        let prompt = PromptBuilder::new()
            .with_crate_name("my_crate".to_string())
            .with_api_sequence(vec!["foo()".to_string(), "bar()".to_string()])
            .build();

        assert!(prompt.system_prompt.contains("Rust"));
        assert!(prompt.user_prompt.contains("foo()"));
        assert!(prompt.user_prompt.contains("bar()"));
    }
}
