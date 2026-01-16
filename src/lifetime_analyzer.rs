//! 生命周期分析器 - 提取和分析函数签名中的生命周期信息
//!
//! 主要功能：
//! 1. 从 rustdoc JSON 提取生命周期参数
//! 2. 分析参数类型中的生命周期标注
//! 3. 分析返回值类型中的生命周期标注
//! 4. 建立返回值生命周期到参数的绑定映射
//!
//! ## 典型场景
//!
//! ```rust
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
    /// 生命周期参数名（如 'a, 'b）
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
    /// 生命周期名称（如 'a）
    pub lifetime: String,
    /// 绑定到的参数索引（0 表示 self，1+ 表示其他参数）
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
            && !self
                .return_lifetimes
                .as_ref()
                .unwrap()
                .lifetimes
                .is_empty()
    }

    /// 获取返回值绑定到的主要参数索引
    /// 如果返回值有多个生命周期，返回第一个绑定的参数
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
                // 引用类型：提取生命周期
                if let Some(lt) = lifetime {
                    if !lifetimes.contains(lt) {
                        lifetimes.push(lt.clone());
                    }
                }
                // 递归处理内部类型
                Self::extract_lifetimes_recursive(type_, lifetimes);
            }
            Type::ResolvedPath(path) => {
                // 路径类型：检查泛型参数
                if let Some(ref args) = path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = args.as_ref()
                    {
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
    /// 规则：
    /// 1. 如果返回值包含生命周期 'a，找到所有参数中也包含 'a 的参数
    /// 2. 如果只有一个参数包含 'a，则绑定到该参数
    /// 3. 如果多个参数包含 'a，优先绑定到 self（如果存在）
    /// 4. 如果返回值有多个生命周期，为每个生命周期建立绑定
    fn build_lifetime_bindings(analysis: &FunctionLifetimeAnalysis) -> Vec<LifetimeBinding> {
        let mut bindings = Vec::new();

        if let Some(return_lts) = &analysis.return_lifetimes {
            for ret_lifetime in &return_lts.lifetimes {
                // 跳过 'static（不需要绑定）
                if ret_lifetime == "'static" {
                    continue;
                }

                // 找到所有包含此生命周期的参数
                let mut matching_params = Vec::new();
                for param_lt in &analysis.param_lifetimes {
                    if param_lt.lifetimes.contains(ret_lifetime) {
                        matching_params.push(param_lt.param_index);
                    }
                }

                // 确定绑定的参数
                let bound_to = if matching_params.is_empty() {
                    // 没有显式标注，推断为第一个参数（通常是 self）
                    0
                } else if matching_params.len() == 1 {
                    // 只有一个匹配，绑定到它
                    matching_params[0]
                } else {
                    // 多个匹配，优先绑定到第一个（self）
                    matching_params[0]
                };

                bindings.push(LifetimeBinding {
                    lifetime: ret_lifetime.clone(),
                    bound_to_param: bound_to,
                });
            }
        }

        bindings
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

    #[test]
    fn test_extract_lifetime_params() {
        // 这里需要构造 Generics 对象进行测试
        // 由于 rustdoc_types 的结构较复杂，这里先留空
    }

    #[test]
    fn test_lifetime_binding_logic() {
        // 测试绑定逻辑
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
}
