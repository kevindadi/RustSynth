//! 生命周期分析器 - 提取和分析函数签名中的生命周期信息
//!
//! 主要功能:
//! 1. 从 rustdoc JSON 提取生命周期参数
//! 2. 分析参数类型中的生命周期标注
//! 3. 分析返回值类型中的生命周期标注
//! 4. 建立返回值生命周期到参数的绑定映射
//!
//! ## 典型场景
//!
//! ```text
//! // 场景 1: 返回值绑定到 self
//! fn get<'a>(&'a self) -> &'a i32 { ... }
//! // 分析结果: return lifetime 'a -> param 0 (self)
//!
//! // 场景 2: 返回值绑定到特定参数
//! fn first<'a>(data: &'a [i32]) -> &'a i32 { ... }
//! // 分析结果: return lifetime 'a -> param 0 (data)
//!
//! // 场景 3: 多个生命周期
//! fn longer<'a, 'b>(x: &'a str, y: &'b str) -> &'a str { ... }
//! // 分析结果: return lifetime 'a -> param 0 (x)
//! ```

use rustdoc_types::{FunctionSignature, GenericParamDefKind, Generics, Type};

/// 生命周期信息
#[derive(Clone, Debug)]
pub struct LifetimeInfo {
    /// 生命周期参数名(如 'a, 'b)
    pub name: String,
    /// 是否是 'static
    pub is_static: bool,
}

/// 参数的生命周期标注
#[derive(Clone, Debug)]
pub struct ParamLifetimes {
    /// 参数索引
    pub param_index: usize,
    /// 参数中出现的生命周期列表
    pub lifetimes: Vec<String>,
}

/// 返回值的生命周期标注
#[derive(Clone, Debug)]
pub struct ReturnLifetimes {
    /// 返回值中出现的生命周期列表
    pub lifetimes: Vec<String>,
}

/// 生命周期绑定映射
/// 表示返回值的生命周期绑定到哪个参数
#[derive(Clone, Debug)]
pub struct LifetimeBinding {
    /// 生命周期名称(如 'a)
    pub lifetime: String,
    /// 绑定到的参数索引(0 表示 self,1+ 表示其他参数)
    pub bound_to_param: usize,
}

/// 函数签名的生命周期分析结果
#[derive(Clone, Debug, Default)]
pub struct FunctionLifetimeAnalysis {
    /// 所有声明的生命周期参数
    pub lifetime_params: Vec<LifetimeInfo>,
    /// 每个参数的生命周期标注
    pub param_lifetimes: Vec<ParamLifetimes>,
    /// 返回值的生命周期标注
    pub return_lifetimes: Option<ReturnLifetimes>,
    /// 返回值生命周期到参数的绑定
    pub lifetime_bindings: Vec<LifetimeBinding>,
}

impl FunctionLifetimeAnalysis {
    /// 检查函数是否返回引用
    pub fn returns_reference(&self) -> bool {
        self.return_lifetimes.is_some()
            && !self.return_lifetimes.as_ref().unwrap().lifetimes.is_empty()
    }

    /// 获取返回值绑定到的主要参数索引
    /// 如果返回值有多个生命周期,返回第一个绑定的参数
    pub fn primary_source_param(&self) -> Option<usize> {
        self.lifetime_bindings.first().map(|b| b.bound_to_param)
    }

    /// 获取返回值的主要生命周期名称
    pub fn primary_return_lifetime(&self) -> Option<&str> {
        self.return_lifetimes
            .as_ref()
            .and_then(|rl| rl.lifetimes.first())
            .map(|s| s.as_str())
    }
}

/// 生命周期分析器
pub struct LifetimeAnalyzer;

impl LifetimeAnalyzer {
    /// 分析函数签名的生命周期
    pub fn analyze(generics: &Generics, sig: &FunctionSignature) -> FunctionLifetimeAnalysis {
        let mut analysis = FunctionLifetimeAnalysis::default();

        // 1. 提取生命周期参数声明
        analysis.lifetime_params = Self::extract_lifetime_params(generics);

        // 2. 分析每个参数的生命周期
        analysis.param_lifetimes = Self::analyze_param_lifetimes(&sig.inputs);

        // 3. 分析返回值的生命周期
        if let Some(output) = &sig.output {
            analysis.return_lifetimes = Some(Self::analyze_type_lifetimes(output));
        }

        // 4. 建立返回值生命周期到参数的绑定
        analysis.lifetime_bindings = Self::build_lifetime_bindings(&analysis);

        analysis
    }

    /// 提取生命周期参数声明
    fn extract_lifetime_params(generics: &Generics) -> Vec<LifetimeInfo> {
        let mut lifetimes = Vec::new();

        for param in &generics.params {
            if let GenericParamDefKind::Lifetime { outlives: _ } = &param.kind {
                lifetimes.push(LifetimeInfo {
                    name: param.name.clone(),
                    is_static: param.name == "'static",
                });
            }
        }

        lifetimes
    }

    /// 分析参数列表的生命周期
    fn analyze_param_lifetimes(inputs: &[(String, Type)]) -> Vec<ParamLifetimes> {
        inputs
            .iter()
            .enumerate()
            .map(|(i, (_name, ty))| {
                let lifetimes = Self::extract_lifetimes_from_type(ty);
                ParamLifetimes {
                    param_index: i,
                    lifetimes,
                }
            })
            .collect()
    }

    /// 分析类型中的生命周期
    fn analyze_type_lifetimes(ty: &Type) -> ReturnLifetimes {
        let lifetimes = Self::extract_lifetimes_from_type(ty);
        ReturnLifetimes { lifetimes }
    }

    /// 从类型中提取生命周期标注
    fn extract_lifetimes_from_type(ty: &Type) -> Vec<String> {
        let mut lifetimes = Vec::new();
        Self::extract_lifetimes_recursive(ty, &mut lifetimes);
        lifetimes
    }

    /// 递归提取类型中的生命周期
    fn extract_lifetimes_recursive(ty: &Type, lifetimes: &mut Vec<String>) {
        match ty {
            Type::BorrowedRef {
                lifetime, type_, ..
            } => {
                // 引用类型:提取生命周期
                if let Some(lt) = lifetime {
                    if !lifetimes.contains(lt) {
                        lifetimes.push(lt.clone());
                    }
                }
                // 递归处理内部类型
                Self::extract_lifetimes_recursive(type_, lifetimes);
            }
            Type::ResolvedPath(path) => {
                // 路径类型:检查泛型参数
                if let Some(ref args) = path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = args.as_ref() {
                        for arg in args {
                            if let rustdoc_types::GenericArg::Type(t) = arg {
                                Self::extract_lifetimes_recursive(t, lifetimes);
                            } else if let rustdoc_types::GenericArg::Lifetime(lt) = arg {
                                if !lifetimes.contains(lt) {
                                    lifetimes.push(lt.clone());
                                }
                            }
                        }
                    }
                }
            }
            Type::Tuple(elems) => {
                for elem in elems {
                    Self::extract_lifetimes_recursive(elem, lifetimes);
                }
            }
            Type::Slice(inner) | Type::Array { type_: inner, .. } => {
                Self::extract_lifetimes_recursive(inner, lifetimes);
            }
            Type::RawPointer { type_, .. } => {
                Self::extract_lifetimes_recursive(type_, lifetimes);
            }
            _ => {
                // 其他类型不包含生命周期
            }
        }
    }

    /// 建立返回值生命周期到参数的绑定
    ///
    /// 使用 Rust 的生命周期省略规则（Lifetime Elision Rules）:
    /// 1. 每个引用参数获得自己的生命周期参数
    /// 2. 如果只有一个输入生命周期参数，它被赋给所有输出生命周期
    /// 3. 如果有 &self 或 &mut self，self 的生命周期被赋给所有输出生命周期
    ///
    /// 当有显式标注时，使用显式标注匹配；否则应用省略规则推断。
    fn build_lifetime_bindings(analysis: &FunctionLifetimeAnalysis) -> Vec<LifetimeBinding> {
        let mut bindings = Vec::new();

        let return_lts = match &analysis.return_lifetimes {
            Some(rl) if !rl.lifetimes.is_empty() => rl,
            _ => return bindings,
        };

        for ret_lifetime in &return_lts.lifetimes {
            if ret_lifetime == "'static" {
                continue;
            }

            let mut matching_params = Vec::new();
            for param_lt in &analysis.param_lifetimes {
                if param_lt.lifetimes.contains(ret_lifetime) {
                    matching_params.push(param_lt.param_index);
                }
            }

            if !matching_params.is_empty() {
                let bound_to = if matching_params.len() == 1 {
                    matching_params[0]
                } else {
                    matching_params[0]
                };
                bindings.push(LifetimeBinding {
                    lifetime: ret_lifetime.clone(),
                    bound_to_param: bound_to,
                });
            }
        }

        if !bindings.is_empty() {
            return bindings;
        }

        // Apply Rust lifetime elision rules when no explicit binding was found
        Self::apply_elision_rules(analysis, return_lts, &mut bindings);

        bindings
    }

    /// Apply Rust lifetime elision rules to infer bindings
    fn apply_elision_rules(
        analysis: &FunctionLifetimeAnalysis,
        return_lts: &ReturnLifetimes,
        bindings: &mut Vec<LifetimeBinding>,
    ) {
        let ref_params: Vec<usize> = analysis
            .param_lifetimes
            .iter()
            .filter(|p| !p.lifetimes.is_empty())
            .map(|p| p.param_index)
            .collect();

        // Elision rule 3: if there is &self / &mut self (param 0), bind to self
        let has_self_ref = analysis
            .param_lifetimes
            .iter()
            .any(|p| p.param_index == 0 && !p.lifetimes.is_empty());

        if has_self_ref {
            for ret_lt in &return_lts.lifetimes {
                if ret_lt != "'static" {
                    bindings.push(LifetimeBinding {
                        lifetime: ret_lt.clone(),
                        bound_to_param: 0,
                    });
                }
            }
            return;
        }

        // Elision rule 2: exactly one input lifetime -> bind all output lifetimes to it
        if ref_params.len() == 1 {
            for ret_lt in &return_lts.lifetimes {
                if ret_lt != "'static" {
                    bindings.push(LifetimeBinding {
                        lifetime: ret_lt.clone(),
                        bound_to_param: ref_params[0],
                    });
                }
            }
            return;
        }

        // Fallback: bind to first parameter
        if !analysis.param_lifetimes.is_empty() {
            for ret_lt in &return_lts.lifetimes {
                if ret_lt != "'static" {
                    bindings.push(LifetimeBinding {
                        lifetime: ret_lt.clone(),
                        bound_to_param: 0,
                    });
                }
            }
        }
    }

    /// 检查类型是否包含引用
    pub fn is_reference_type(ty: &Type) -> bool {
        matches!(ty, Type::BorrowedRef { .. })
    }

    /// 检查类型是否包含可变引用
    pub fn is_mutable_reference(ty: &Type) -> bool {
        matches!(
            ty,
            Type::BorrowedRef {
                is_mutable: true,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustdoc_types::{GenericParamDef, GenericParamDefKind};

    #[test]
    fn test_extract_lifetime_params() {
        let generics = Generics {
            params: vec![
                GenericParamDef {
                    name: "'a".to_string(),
                    kind: GenericParamDefKind::Lifetime {
                        outlives: vec![],
                    },
                },
                GenericParamDef {
                    name: "'b".to_string(),
                    kind: GenericParamDefKind::Lifetime {
                        outlives: vec!["'a".to_string()],
                    },
                },
            ],
            where_predicates: vec![],
        };

        let params = LifetimeAnalyzer::extract_lifetime_params(&generics);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "'a");
        assert!(!params[0].is_static);
        assert_eq!(params[1].name, "'b");
    }

    #[test]
    fn test_extract_lifetime_params_empty() {
        let generics = Generics {
            params: vec![],
            where_predicates: vec![],
        };
        let params = LifetimeAnalyzer::extract_lifetime_params(&generics);
        assert!(params.is_empty());
    }

    #[test]
    fn test_explicit_binding_single_param() {
        // fn get<'a>(&'a self) -> &'a i32
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![LifetimeInfo {
                name: "'a".to_string(),
                is_static: false,
            }],
            param_lifetimes: vec![
                ParamLifetimes {
                    param_index: 0,
                    lifetimes: vec!["'a".to_string()],
                },
                ParamLifetimes {
                    param_index: 1,
                    lifetimes: vec![],
                },
            ],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'a".to_string()],
            }),
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].lifetime, "'a");
        assert_eq!(bindings[0].bound_to_param, 0);
    }

    #[test]
    fn test_elision_rule2_single_input_lifetime() {
        // fn first(data: &[i32]) -> &i32
        // Elision rule 2: one input lifetime -> output gets the same
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![ParamLifetimes {
                param_index: 0,
                lifetimes: vec!["'elided_0".to_string()],
            }],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'elided_ret".to_string()],
            }),
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].bound_to_param, 0);
    }

    #[test]
    fn test_elision_rule3_self_reference() {
        // fn get(&self) -> &i32
        // Elision rule 3: &self lifetime -> output gets self's lifetime
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![
                ParamLifetimes {
                    param_index: 0,
                    lifetimes: vec!["'self".to_string()],
                },
                ParamLifetimes {
                    param_index: 1,
                    lifetimes: vec!["'other".to_string()],
                },
            ],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'ret".to_string()],
            }),
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].bound_to_param, 0, "Should bind to self (param 0)");
    }

    #[test]
    fn test_static_lifetime_skipped() {
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'static".to_string()],
            }),
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert!(bindings.is_empty(), "'static should not produce bindings");
    }

    #[test]
    fn test_no_return_lifetime() {
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![ParamLifetimes {
                param_index: 0,
                lifetimes: vec!["'a".to_string()],
            }],
            return_lifetimes: None,
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert!(bindings.is_empty());
    }

    #[test]
    fn test_multiple_return_lifetimes() {
        // fn foo<'a, 'b>(x: &'a T, y: &'b U) -> (&'a T, &'b U)
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![
                LifetimeInfo { name: "'a".to_string(), is_static: false },
                LifetimeInfo { name: "'b".to_string(), is_static: false },
            ],
            param_lifetimes: vec![
                ParamLifetimes { param_index: 0, lifetimes: vec!["'a".to_string()] },
                ParamLifetimes { param_index: 1, lifetimes: vec!["'b".to_string()] },
            ],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'a".to_string(), "'b".to_string()],
            }),
            lifetime_bindings: vec![],
        };

        let bindings = LifetimeAnalyzer::build_lifetime_bindings(&analysis);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].bound_to_param, 0);
        assert_eq!(bindings[1].bound_to_param, 1);
    }

    #[test]
    fn test_returns_reference() {
        let with_ref = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![],
            return_lifetimes: Some(ReturnLifetimes {
                lifetimes: vec!["'a".to_string()],
            }),
            lifetime_bindings: vec![],
        };
        assert!(with_ref.returns_reference());

        let without_ref = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![],
            return_lifetimes: None,
            lifetime_bindings: vec![],
        };
        assert!(!without_ref.returns_reference());

        let empty_ref = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![],
            return_lifetimes: Some(ReturnLifetimes { lifetimes: vec![] }),
            lifetime_bindings: vec![],
        };
        assert!(!empty_ref.returns_reference());
    }

    #[test]
    fn test_primary_source_param() {
        let analysis = FunctionLifetimeAnalysis {
            lifetime_params: vec![],
            param_lifetimes: vec![],
            return_lifetimes: None,
            lifetime_bindings: vec![
                LifetimeBinding { lifetime: "'a".to_string(), bound_to_param: 2 },
            ],
        };
        assert_eq!(analysis.primary_source_param(), Some(2));
    }
}
