//! Witness（见证）定义
//!
//! Witness 是从初始状态到目标状态的变迁序列

use std::fmt;
use super::transition::TransitionId;

/// Witness（见证）
///
/// 表示从初始配置到目标配置的变迁序列
#[derive(Debug, Clone)]
pub struct Witness {
    /// 步骤序列
    pub steps: Vec<WitnessStep>,
}

impl Witness {
    /// 创建空的 witness
    pub fn empty() -> Self {
        Witness { steps: Vec::new() }
    }

    /// 添加步骤
    pub fn push(&mut self, step: WitnessStep) {
        self.steps.push(step);
    }

    /// 长度
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 只获取 API 调用步骤
    pub fn api_calls(&self) -> Vec<&WitnessStep> {
        self.steps.iter().filter(|s| s.is_api_call).collect()
    }

    /// 转换为 API 序列字符串
    pub fn to_api_sequence_string(&self) -> String {
        self.api_calls()
            .iter()
            .map(|s| s.transition_name.clone())
            .collect::<Vec<_>>()
            .join(" -> ")
    }

    /// 生成 Rust 代码
    pub fn to_rust_code(&self) -> String {
        let mut code = String::new();
        code.push_str("// Auto-generated API sequence\n");
        code.push_str("fn generated_test() {\n");

        for (i, step) in self.api_calls().iter().enumerate() {
            code.push_str(&format!("    // Step {}: {}\n", i + 1, step.transition_name));
            // 实际代码生成需要更多信息
            code.push_str(&format!("    // {}();\n", step.transition_name));
        }

        code.push_str("}\n");
        code
    }
}

impl fmt::Display for Witness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.steps.is_empty() {
            write!(f, "(empty)")
        } else {
            let steps: Vec<String> = self.steps
                .iter()
                .map(|s| s.transition_name.clone())
                .collect();
            write!(f, "{}", steps.join(" -> "))
        }
    }
}

/// Witness 步骤
#[derive(Debug, Clone)]
pub struct WitnessStep {
    /// 变迁 ID
    pub transition_id: TransitionId,
    /// 变迁名称
    pub transition_name: String,
    /// 是否是 API 调用（用于过滤结构变迁）
    pub is_api_call: bool,
}

impl WitnessStep {
    pub fn new(id: TransitionId, name: String, is_api: bool) -> Self {
        WitnessStep {
            transition_id: id,
            transition_name: name,
            is_api_call: is_api,
        }
    }
}

impl fmt::Display for WitnessStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.transition_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_empty() {
        let witness = Witness::empty();
        assert!(witness.is_empty());
        assert_eq!(witness.len(), 0);
    }

    #[test]
    fn test_witness_display() {
        let mut witness = Witness::empty();
        witness.push(WitnessStep::new(
            TransitionId::new(0),
            "new".to_string(),
            true,
        ));
        witness.push(WitnessStep::new(
            TransitionId::new(1),
            "push".to_string(),
            true,
        ));

        assert_eq!(format!("{}", witness), "new -> push");
    }

    #[test]
    fn test_api_calls_filter() {
        let mut witness = Witness::empty();
        witness.push(WitnessStep::new(
            TransitionId::new(0),
            "Move".to_string(),
            false, // 结构变迁
        ));
        witness.push(WitnessStep::new(
            TransitionId::new(1),
            "Vec::new".to_string(),
            true, // API 调用
        ));

        let api_calls = witness.api_calls();
        assert_eq!(api_calls.len(), 1);
        assert_eq!(api_calls[0].transition_name, "Vec::new");
    }
}

