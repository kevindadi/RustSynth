/// Guard 验证和评估引擎
///
/// Guards 用于在 Petri 网的转换执行前进行条件检查,
/// 例如所有权验证、类型约束检查等.
use serde_json::Value;
use std::collections::HashMap;

use super::schema::{JsonGuard, JsonGuardCondition};

/// Guard 评估上下文 - 包含执行 guard 检查所需的所有信息
#[derive(Debug, Clone)]
pub struct GuardContext {
    /// 变量绑定(变量名 -> 值)
    pub variables: HashMap<String, Value>,
}

impl GuardContext {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    /// 添加或更新变量绑定
    pub fn set_variable(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    /// 获取变量值
    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// 从路径获取值(支持 "token.ownership" 这样的路径)
    pub fn get_value_from_path(&self, path: &str) -> Option<Value> {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return None;
        }

        // 获取根变量
        let root = self.variables.get(parts[0])?;

        // 如果只有一个部分,直接返回
        if parts.len() == 1 {
            return Some(root.clone());
        }

        // 遍历路径
        let mut current = root;
        for part in &parts[1..] {
            current = current.get(part)?;
        }

        Some(current.clone())
    }
}

impl Default for GuardContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard 评估器
pub struct GuardEvaluator<'a> {
    guards: &'a [JsonGuard],
}

impl<'a> GuardEvaluator<'a> {
    pub fn new(guards: &'a [JsonGuard]) -> Self {
        Self { guards }
    }

    /// 评估指定的 guard 是否满足
    pub fn evaluate_guard(&self, guard_id: &str, context: &GuardContext) -> Result<bool, String> {
        let guard = self
            .guards
            .iter()
            .find(|g| g.id == guard_id)
            .ok_or_else(|| format!("Guard not found: {}", guard_id))?;

        self.evaluate_guard_conditions(&guard.conditions, context)
    }

    /// 评估多个 guard(所有 guard 都必须满足)
    pub fn evaluate_guards(
        &self,
        guard_ids: &[String],
        context: &GuardContext,
    ) -> Result<bool, String> {
        for guard_id in guard_ids {
            if !self.evaluate_guard(guard_id, context)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 评估 guard 的所有条件(所有条件都必须满足)
    fn evaluate_guard_conditions(
        &self,
        conditions: &[JsonGuardCondition],
        context: &GuardContext,
    ) -> Result<bool, String> {
        for condition in conditions {
            let result = self.evaluate_condition(condition, context)?;
            if !result {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// 评估单个条件
    fn evaluate_condition(
        &self,
        condition: &JsonGuardCondition,
        context: &GuardContext,
    ) -> Result<bool, String> {
        // 获取左侧值
        let lhs_value = context
            .get_value_from_path(&condition.lhs)
            .ok_or_else(|| format!("Variable not found: {}", condition.lhs))?;

        // 根据操作符评估
        let result = match condition.op.as_str() {
            "==" | "eq" => self.eval_equals(&lhs_value, &condition.rhs),
            "!=" | "ne" => !self.eval_equals(&lhs_value, &condition.rhs),
            "in" => self.eval_in(&lhs_value, &condition.rhs),
            "not_in" => !self.eval_in(&lhs_value, &condition.rhs),
            ">" | "gt" => self.eval_greater_than(&lhs_value, &condition.rhs)?,
            ">=" | "ge" => self.eval_greater_equal(&lhs_value, &condition.rhs)?,
            "<" | "lt" => self.eval_less_than(&lhs_value, &condition.rhs)?,
            "<=" | "le" => self.eval_less_equal(&lhs_value, &condition.rhs)?,
            "contains" => self.eval_contains(&lhs_value, &condition.rhs),
            "matches" => self.eval_matches(&lhs_value, &condition.rhs)?,
            _ => return Err(format!("Unknown operator: {}", condition.op)),
        };

        // 应用 negate
        Ok(if condition.negate { !result } else { result })
    }

    /// 相等比较
    fn eval_equals(&self, lhs: &Value, rhs: &Value) -> bool {
        lhs == rhs
    }

    /// 检查值是否在集合中
    fn eval_in(&self, lhs: &Value, rhs: &Value) -> bool {
        if let Some(arr) = rhs.as_array() {
            arr.contains(lhs)
        } else {
            false
        }
    }

    /// 大于比较(仅用于数字)
    fn eval_greater_than(&self, lhs: &Value, rhs: &Value) -> Result<bool, String> {
        match (lhs.as_f64(), rhs.as_f64()) {
            (Some(l), Some(r)) => Ok(l > r),
            _ => Err("Greater than comparison requires numeric values".to_string()),
        }
    }

    /// 大于等于比较(仅用于数字)
    fn eval_greater_equal(&self, lhs: &Value, rhs: &Value) -> Result<bool, String> {
        match (lhs.as_f64(), rhs.as_f64()) {
            (Some(l), Some(r)) => Ok(l >= r),
            _ => Err("Greater or equal comparison requires numeric values".to_string()),
        }
    }

    /// 小于比较(仅用于数字)
    fn eval_less_than(&self, lhs: &Value, rhs: &Value) -> Result<bool, String> {
        match (lhs.as_f64(), rhs.as_f64()) {
            (Some(l), Some(r)) => Ok(l < r),
            _ => Err("Less than comparison requires numeric values".to_string()),
        }
    }

    /// 小于等于比较(仅用于数字)
    fn eval_less_equal(&self, lhs: &Value, rhs: &Value) -> Result<bool, String> {
        match (lhs.as_f64(), rhs.as_f64()) {
            (Some(l), Some(r)) => Ok(l <= r),
            _ => Err("Less or equal comparison requires numeric values".to_string()),
        }
    }

    /// 检查字符串是否包含另一个字符串
    fn eval_contains(&self, lhs: &Value, rhs: &Value) -> bool {
        match (lhs.as_str(), rhs.as_str()) {
            (Some(l), Some(r)) => l.contains(r),
            _ => false,
        }
    }

    /// 简单的模式匹配(通配符支持: * 匹配任意字符)
    /// 注意: 这是一个简化版本,不支持完整的正则表达式
    fn eval_matches(&self, lhs: &Value, rhs: &Value) -> Result<bool, String> {
        let text = lhs
            .as_str()
            .ok_or_else(|| "Left-hand side must be a string for pattern matching".to_string())?;
        let pattern = rhs
            .as_str()
            .ok_or_else(|| "Right-hand side must be a pattern string".to_string())?;

        // 简单的通配符匹配: * 匹配任意字符
        Ok(self.wildcard_match(text, pattern))
    }

    /// 简单的通配符匹配实现
    fn wildcard_match(&self, text: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.is_empty() {
            return text.is_empty();
        }

        let parts: Vec<&str> = pattern.split('*').collect();

        if parts.len() == 1 {
            // 没有通配符,直接比较
            return text == pattern;
        }

        // 有通配符
        let mut pos = 0;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            if i == 0 {
                // 第一部分必须在开头
                if !text.starts_with(part) {
                    return false;
                }
                pos = part.len();
            } else if i == parts.len() - 1 {
                // 最后一部分必须在结尾
                if !text.ends_with(part) {
                    return false;
                }
            } else {
                // 中间部分
                if let Some(found_pos) = text[pos..].find(part) {
                    pos += found_pos + part.len();
                } else {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_guard_evaluation_ownership() {
        let guards = vec![JsonGuard {
            id: "g1".to_string(),
            kind: Some("ownership".to_string()),
            description: Some("Check ownership is owned or shared".to_string()),
            conditions: vec![JsonGuardCondition {
                lhs: "input_token.ownership".to_string(),
                op: "in".to_string(),
                rhs: json!(["owned", "shared"]),
                negate: false,
            }],
            scope: None,
        }];

        let evaluator = GuardEvaluator::new(&guards);

        // 测试: ownership = "owned" (应该通过)
        let mut context = GuardContext::new();
        context.set_variable(
            "input_token".to_string(),
            json!({
                "ownership": "owned"
            }),
        );
        assert!(evaluator.evaluate_guard("g1", &context).unwrap());

        // 测试: ownership = "borrowed" (应该失败)
        let mut context2 = GuardContext::new();
        context2.set_variable(
            "input_token".to_string(),
            json!({
                "ownership": "borrowed"
            }),
        );
        assert!(!evaluator.evaluate_guard("g1", &context2).unwrap());
    }

    #[test]
    fn test_guard_evaluation_comparison() {
        let guards = vec![JsonGuard {
            id: "g2".to_string(),
            kind: Some("numeric".to_string()),
            description: Some("Check value is greater than 10".to_string()),
            conditions: vec![JsonGuardCondition {
                lhs: "value".to_string(),
                op: ">".to_string(),
                rhs: json!(10),
                negate: false,
            }],
            scope: None,
        }];

        let evaluator = GuardEvaluator::new(&guards);

        // 测试: value = 20 (应该通过)
        let mut context = GuardContext::new();
        context.set_variable("value".to_string(), json!(20));
        assert!(evaluator.evaluate_guard("g2", &context).unwrap());

        // 测试: value = 5 (应该失败)
        let mut context2 = GuardContext::new();
        context2.set_variable("value".to_string(), json!(5));
        assert!(!evaluator.evaluate_guard("g2", &context2).unwrap());
    }

    #[test]
    fn test_guard_evaluation_negate() {
        let guards = vec![JsonGuard {
            id: "g3".to_string(),
            kind: Some("negation".to_string()),
            description: Some("Check value is NOT equal to 'invalid'".to_string()),
            conditions: vec![JsonGuardCondition {
                lhs: "status".to_string(),
                op: "==".to_string(),
                rhs: json!("invalid"),
                negate: true, // 取反
            }],
            scope: None,
        }];

        let evaluator = GuardEvaluator::new(&guards);

        // 测试: status = "valid" (应该通过,因为不等于 "invalid")
        let mut context = GuardContext::new();
        context.set_variable("status".to_string(), json!("valid"));
        assert!(evaluator.evaluate_guard("g3", &context).unwrap());

        // 测试: status = "invalid" (应该失败)
        let mut context2 = GuardContext::new();
        context2.set_variable("status".to_string(), json!("invalid"));
        assert!(!evaluator.evaluate_guard("g3", &context2).unwrap());
    }
}
