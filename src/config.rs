//! 任务配置文件解析
//!
//! 支持 TOML 格式的任务配置,包括搜索参数、过滤规则和目标定义.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::types::{Capability, TyGround, TypeForm};

#[derive(Debug, Clone, Deserialize)]
pub struct TaskConfig {
    pub inputs: InputsConfig,
    pub search: SearchConfig,
    #[serde(default)]
    pub filter: FilterConfig,
    pub goal: GoalConfig,
    #[serde(default)]
    pub initial: InitialConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputsConfig {
    pub doc_json: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    #[serde(default = "default_stack_depth")]
    pub stack_depth: usize,
    #[serde(default = "default_place_bound")]
    pub default_place_bound: usize,
    #[serde(default)]
    pub place_bounds: HashMap<String, usize>,
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_max_traces")]
    pub max_traces: usize,
}

fn default_stack_depth() -> usize {
    8
}
fn default_place_bound() -> usize {
    2
}
fn default_max_steps() -> usize {
    100
}
fn default_strategy() -> String {
    "bfs".to_string()
}
fn default_max_traces() -> usize {
    1
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FilterConfig {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoalConfig {
    pub want: String,
    #[serde(default = "default_goal_count")]
    pub count: usize,
}

fn default_goal_count() -> usize {
    1
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InitialConfig {
    #[serde(default)]
    pub tokens: Vec<InitialToken>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InitialToken {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default = "default_token_count")]
    pub count: usize,
}

fn default_token_count() -> usize {
    1
}

impl TaskConfig {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content =
            std::fs::read_to_string(&path).context("Failed to read task configuration file")?;
        let config: TaskConfig = toml::from_str(&content).context("Failed to parse TOML")?;
        Ok(config)
    }

    pub fn get_place_bound(&self, place_key: &str) -> usize {
        self.search
            .place_bounds
            .get(place_key)
            .copied()
            .unwrap_or(self.search.default_place_bound)
    }

    pub fn is_function_allowed(&self, fn_path: &str) -> bool {
        if !self.filter.deny.is_empty() {
            for pattern in &self.filter.deny {
                if fn_path.contains(pattern) || pattern.contains(fn_path) {
                    return false;
                }
            }
        }

        if self.filter.allow.is_empty() {
            return true;
        }

        for pattern in &self.filter.allow {
            if fn_path.contains(pattern) || pattern.contains(fn_path) {
                return true;
            }
        }

        false
    }
}

#[derive(Debug, Clone)]
pub struct ParsedGoal {
    pub cap: Capability,
    pub form: TypeForm,
    pub base_type: TyGround,
    pub count: usize,
}

impl ParsedGoal {
    pub fn parse(goal: &GoalConfig) -> Result<Self> {
        let parts: Vec<&str> = goal.want.split_whitespace().collect();
        if parts.len() < 2 {
            anyhow::bail!(
                "Invalid goal format: '{}'. Expected 'cap type' (e.g., 'own i32')",
                goal.want
            );
        }

        let cap = match parts[0].to_lowercase().as_str() {
            "own" => Capability::Own,
            "frz" => Capability::Frz,
            "blk" => Capability::Blk,
            _ => anyhow::bail!("Invalid capability: {}. Expected own/frz/blk", parts[0]),
        };

        let type_str = parts[1..].join(" ");
        let (form, base_type) = parse_type_with_form(&type_str)?;

        Ok(ParsedGoal {
            cap,
            form,
            base_type,
            count: goal.count,
        })
    }
}

fn parse_type_with_form(type_str: &str) -> Result<(TypeForm, TyGround)> {
    let type_str = type_str.trim();

    if type_str.starts_with("&mut ") {
        let inner = &type_str[5..];
        let base = parse_base_type(inner)?;
        return Ok((TypeForm::RefMut, base));
    }

    if type_str.starts_with('&') {
        let inner = &type_str[1..];
        let base = parse_base_type(inner)?;
        return Ok((TypeForm::RefShr, base));
    }

    let base = parse_base_type(type_str)?;
    Ok((TypeForm::Value, base))
}

fn parse_base_type(type_str: &str) -> Result<TyGround> {
    let type_str = type_str.trim();

    if type_str == "()" {
        return Ok(TyGround::Unit);
    }

    if let Some(lt_pos) = type_str.find('<') {
        if let Some(gt_pos) = type_str.rfind('>') {
            let name = &type_str[..lt_pos];
            let args_str = &type_str[lt_pos + 1..gt_pos];

            let args: Result<Vec<TyGround>> = args_str
                .split(',')
                .map(|s| parse_base_type(s.trim()))
                .collect();

            return Ok(TyGround::path_with_args(name, args?));
        }
    }

    let primitives = [
        "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32",
        "i64", "i128", "isize", "f32", "f64",
    ];

    if primitives.contains(&type_str) {
        return Ok(TyGround::primitive(type_str));
    }

    Ok(TyGround::path(type_str))
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            stack_depth: default_stack_depth(),
            default_place_bound: default_place_bound(),
            place_bounds: HashMap::new(),
            max_steps: default_max_steps(),
            strategy: default_strategy(),
            max_traces: default_max_traces(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_goal() {
        let goal = GoalConfig {
            want: "own i32".to_string(),
            count: 1,
        };
        let parsed = ParsedGoal::parse(&goal).unwrap();
        assert_eq!(parsed.cap, Capability::Own);
        assert_eq!(parsed.form, TypeForm::Value);
        assert!(matches!(parsed.base_type, TyGround::Primitive(ref s) if s == "i32"));
    }

    #[test]
    fn test_parse_goal_with_ref() {
        let goal = GoalConfig {
            want: "own &Counter".to_string(),
            count: 1,
        };
        let parsed = ParsedGoal::parse(&goal).unwrap();
        assert_eq!(parsed.cap, Capability::Own);
        assert_eq!(parsed.form, TypeForm::RefShr);
        assert!(matches!(parsed.base_type, TyGround::Path { ref name, .. } if name == "Counter"));
    }

    #[test]
    fn test_parse_goal_with_generic() {
        let goal = GoalConfig {
            want: "own Vec<u8>".to_string(),
            count: 2,
        };
        let parsed = ParsedGoal::parse(&goal).unwrap();
        assert_eq!(parsed.cap, Capability::Own);
        assert_eq!(parsed.form, TypeForm::Value);
        assert!(
            matches!(parsed.base_type, TyGround::Path { ref name, ref args } 
            if name == "Vec" && args.len() == 1)
        );
    }

    #[test]
    fn test_filter_config() {
        let config = TaskConfig {
            inputs: InputsConfig {
                doc_json: "test.json".to_string(),
            },
            search: SearchConfig::default(),
            filter: FilterConfig {
                allow: vec!["Counter::".to_string(), "make_counter".to_string()],
                deny: vec![],
            },
            goal: GoalConfig {
                want: "own i32".to_string(),
                count: 1,
            },
            initial: InitialConfig::default(),
        };

        assert!(config.is_function_allowed("Counter::new"));
        assert!(config.is_function_allowed("make_counter"));
        assert!(!config.is_function_allowed("other_function"));
    }

    #[test]
    fn test_place_bounds() {
        let mut place_bounds = HashMap::new();
        place_bounds.insert("own i32".to_string(), 5);

        let config = TaskConfig {
            inputs: InputsConfig {
                doc_json: "test.json".to_string(),
            },
            search: SearchConfig {
                stack_depth: 8,
                default_place_bound: 2,
                place_bounds,
                max_steps: 100,
                ..Default::default()
            },
            filter: FilterConfig::default(),
            goal: GoalConfig {
                want: "own i32".to_string(),
                count: 1,
            },
            initial: InitialConfig::default(),
        };

        assert_eq!(config.get_place_bound("own i32"), 5);
        assert_eq!(config.get_place_bound("own Counter"), 2);
    }
}
