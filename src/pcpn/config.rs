//! PCPN 分析配置
//!
//! 控制分析行为的各种开关

use serde::{Deserialize, Serialize};

/// PCPN 分析配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcpnConfig {
    // ==================== 搜索配置 ====================
    
    /// 最大搜索步数
    pub max_steps: usize,
    /// 最大栈深度
    pub max_stack_depth: usize,
    /// 每个库所最大 token 数
    pub max_tokens_per_place: usize,

    // ==================== 类型能力开关 ====================
    
    /// 启用 Copy 类型自动复制
    pub enable_copy: bool,
    /// 启用 Clone 类型克隆
    pub enable_clone: bool,
    /// Clone 的预算（每个值最多 clone 次数）
    pub clone_budget: usize,
    /// 启用 Default trait 自动构造
    pub enable_default: bool,

    // ==================== 借用和生命周期 ====================
    
    /// 启用生命周期跟踪
    pub enable_lifetime: bool,
    /// 启用字段投影（重借用）
    pub enable_field_projection: bool,
    /// 启用可变重借用
    pub enable_mut_reborrow: bool,
    /// 最大借用嵌套深度
    pub max_borrow_depth: usize,

    // ==================== 代码生成配置 ====================
    
    /// 生成模式
    pub generation_mode: GenerationMode,
    /// 是否生成完整的可编译代码
    pub generate_compilable_code: bool,
    /// 是否包含类型注解
    pub include_type_annotations: bool,
    /// 是否生成测试框架代码
    pub generate_test_harness: bool,

    // ==================== LLM 配置 ====================
    
    /// 使用 LLM 补全
    pub use_llm_completion: bool,
    /// LLM 补全的提示词模板
    pub llm_prompt_template: LlmPromptTemplate,
    /// 每个 trace 的最大 token 数
    pub llm_max_tokens: usize,
}

impl Default for PcpnConfig {
    fn default() -> Self {
        PcpnConfig {
            max_steps: 1000,
            max_stack_depth: 20,
            max_tokens_per_place: 10,

            enable_copy: true,
            enable_clone: true,
            clone_budget: 3,
            enable_default: true,

            enable_lifetime: false,
            enable_field_projection: false,
            enable_mut_reborrow: false,
            max_borrow_depth: 3,

            generation_mode: GenerationMode::FullSequence,
            generate_compilable_code: true,
            include_type_annotations: true,
            generate_test_harness: true,

            use_llm_completion: false,
            llm_prompt_template: LlmPromptTemplate::default(),
            llm_max_tokens: 2048,
        }
    }
}

impl PcpnConfig {
    /// 创建保守配置（禁用高级特性）
    pub fn conservative() -> Self {
        PcpnConfig {
            enable_lifetime: false,
            enable_field_projection: false,
            enable_mut_reborrow: false,
            enable_clone: false,
            ..Default::default()
        }
    }

    /// 创建完整配置（启用所有特性）
    pub fn full() -> Self {
        PcpnConfig {
            enable_lifetime: true,
            enable_field_projection: true,
            enable_mut_reborrow: true,
            ..Default::default()
        }
    }

    /// 创建 LLM 辅助配置
    pub fn with_llm() -> Self {
        PcpnConfig {
            use_llm_completion: true,
            generation_mode: GenerationMode::TraceOnly,
            ..Default::default()
        }
    }
}

/// 代码生成模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenerationMode {
    /// 生成完整的 API 调用序列代码
    FullSequence,
    /// 只生成 trace（API 调用名称序列），需要 LLM 补全
    TraceOnly,
    /// 生成带占位符的代码，需要 LLM 填充参数
    WithPlaceholders,
    /// 生成最小化的 harness，让 LLM 生成完整实现
    MinimalHarness,
}

/// LLM 提示词模板
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmPromptTemplate {
    /// 系统提示词
    pub system_prompt: String,
    /// 任务描述模板
    pub task_template: String,
    /// API 序列前缀
    pub sequence_prefix: String,
    /// 约束说明
    pub constraints: Vec<String>,
    /// 示例代码（few-shot）
    pub examples: Vec<LlmExample>,
}

impl Default for LlmPromptTemplate {
    fn default() -> Self {
        LlmPromptTemplate {
            system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
            task_template: DEFAULT_TASK_TEMPLATE.to_string(),
            sequence_prefix: "// API sequence:".to_string(),
            constraints: vec![
                "生成的代码必须是有效的 Rust 代码".to_string(),
                "遵循 Rust 所有权和借用规则".to_string(),
                "不要使用 unsafe 代码，除非 API 明确要求".to_string(),
                "所有变量必须正确初始化".to_string(),
                "确保所有借用在使用前有效".to_string(),
            ],
            examples: Vec::new(),
        }
    }
}

impl LlmPromptTemplate {
    /// 生成完整的提示词
    pub fn generate_prompt(
        &self,
        crate_name: &str,
        api_trace: &[String],
        type_context: &str,
    ) -> String {
        let mut prompt = String::new();

        // 系统提示
        prompt.push_str(&self.system_prompt);
        prompt.push_str("\n\n");

        // 任务描述
        let task = self.task_template
            .replace("{crate_name}", crate_name)
            .replace("{api_count}", &api_trace.len().to_string());
        prompt.push_str(&task);
        prompt.push_str("\n\n");

        // 类型上下文
        prompt.push_str("## 类型定义\n\n```rust\n");
        prompt.push_str(type_context);
        prompt.push_str("\n```\n\n");

        // API 序列
        prompt.push_str("## API 调用序列\n\n");
        prompt.push_str(&self.sequence_prefix);
        prompt.push('\n');
        for (i, api) in api_trace.iter().enumerate() {
            prompt.push_str(&format!("// {}. {}\n", i + 1, api));
        }
        prompt.push('\n');

        // 约束
        prompt.push_str("## 约束条件\n\n");
        for constraint in &self.constraints {
            prompt.push_str(&format!("- {}\n", constraint));
        }
        prompt.push('\n');

        // 示例
        if !self.examples.is_empty() {
            prompt.push_str("## 示例\n\n");
            for example in &self.examples {
                prompt.push_str(&format!("### {}\n\n```rust\n{}\n```\n\n", 
                    example.description, example.code));
            }
        }

        // 最终指令
        prompt.push_str("## 任务\n\n");
        prompt.push_str("请根据上述 API 序列，生成完整的、可编译的 Rust 测试代码。\n");
        prompt.push_str("代码应该放在一个 `#[test]` 函数中。\n\n");
        prompt.push_str("```rust\n");

        prompt
    }
}

/// LLM 示例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmExample {
    /// 示例描述
    pub description: String,
    /// 示例代码
    pub code: String,
}

// ==================== 默认提示词 ====================

const DEFAULT_SYSTEM_PROMPT: &str = r#"你是一个 Rust 代码生成专家。你的任务是根据给定的 API 调用序列，生成完整的、可编译的 Rust 测试代码。

你需要：
1. 正确处理 Rust 的所有权和借用规则
2. 为每个 API 调用提供合适的参数
3. 处理返回值，确保变量正确绑定
4. 生成能够通过编译的代码

注意事项：
- 基本类型（u8, i32, bool 等）可以直接使用字面量
- 对于需要 String 的地方，使用 String::from("test") 或 "test".to_string()
- 对于需要 Vec 的地方，使用 vec![] 宏
- 对于需要 Option 的地方，使用 Some(value) 或 None
- 对于需要 Result 的地方，使用 Ok(value) 或处理 Err"#;

const DEFAULT_TASK_TEMPLATE: &str = r#"## 任务描述

请为 crate `{crate_name}` 生成测试代码。这个测试应该依次调用 {api_count} 个 API。"#;

