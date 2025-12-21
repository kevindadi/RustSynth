//! 环境 trait
//!
//! 定义类型检查和谓词查询的接口

use crate::pushdown_colored_pt_net::types::TypeExpr;

/// 环境 trait
/// 
/// 提供类型检查和谓词查询功能
pub trait Env: std::fmt::Debug {
    /// 检查类型是否是 Copy 类型
    fn is_copy(&self, ty: &TypeExpr) -> bool;

    /// 检查类型是否实现了指定的 trait
    fn entails_trait(&self, ty: &TypeExpr, trait_name: &str) -> bool;

    /// 解析关联类型
    /// 
    /// 参数:
    /// - ty: 基础类型
    /// - trait_name: trait 名称
    /// - assoc_name: 关联类型名称
    /// 
    /// 返回:
    /// - Some(TypeExpr): 解析得到的关联类型
    /// - None: 无法解析
    fn assoc_res(&self, ty: &TypeExpr, trait_name: &str, assoc_name: &str) -> Option<TypeExpr>;

    /// 类型统一 (存根实现)
    /// 
    /// 检查两个类型是否可以统一
    fn unify_types(&self, ty1: &TypeExpr, ty2: &TypeExpr) -> bool {
        ty1 == ty2
    }

    /// 生命周期统一 (存根实现)
    /// 
    /// 检查两个生命周期是否可以统一
    fn unify_lifetimes(&self, lt1: &str, lt2: &str) -> bool {
        lt1 == lt2
    }
}

/// Mock 环境实现
/// 
/// 用于测试的简化环境实现
#[derive(Debug, Clone, Default)]
pub struct MockEnv {
    /// Copy 类型集合
    copy_types: Vec<String>,
    /// Trait 实现映射: (类型名, trait名) -> true
    trait_impls: Vec<(String, String)>,
}

impl MockEnv {
    /// 创建新的 Mock 环境
    pub fn new() -> Self {
        let mut env = MockEnv {
            copy_types: Vec::new(),
            trait_impls: Vec::new(),
        };

        // 添加基本 Copy 类型
        env.copy_types.extend(vec![
            "u8".to_string(),
            "u16".to_string(),
            "u32".to_string(),
            "u64".to_string(),
            "usize".to_string(),
            "i8".to_string(),
            "i16".to_string(),
            "i32".to_string(),
            "i64".to_string(),
            "isize".to_string(),
            "bool".to_string(),
            "char".to_string(),
            "f32".to_string(),
            "f64".to_string(),
        ]);

        env
    }

    /// 添加 Copy 类型
    pub fn add_copy_type(&mut self, ty: String) {
        self.copy_types.push(ty);
    }

    /// 添加 trait 实现
    pub fn add_trait_impl(&mut self, ty: String, trait_name: String) {
        self.trait_impls.push((ty, trait_name));
    }
}

impl Env for MockEnv {
    fn is_copy(&self, ty: &TypeExpr) -> bool {
        match ty {
            TypeExpr::Primitive(name) => self.copy_types.contains(name),
            TypeExpr::Reference { .. } => {
                // 引用类型是 Copy
                true
            }
            TypeExpr::Generic { .. } => {
                // 默认泛型不是 Copy (除非有约束)
                false
            }
            _ => false,
        }
    }

    fn entails_trait(&self, ty: &TypeExpr, trait_name: &str) -> bool {
        let ty_name = match ty {
            TypeExpr::Primitive(name) => name.clone(),
            TypeExpr::Composite { name, .. } => name.clone(),
            _ => return false,
        };

        self.trait_impls
            .iter()
            .any(|(t, tr)| t == &ty_name && tr == trait_name)
    }

    fn assoc_res(&self, ty: &TypeExpr, trait_name: &str, assoc_name: &str) -> Option<TypeExpr> {
        // Mock 实现: 返回一个简单的关联类型
        if self.entails_trait(ty, trait_name) {
            Some(TypeExpr::AssociatedType {
                owner: trait_name.to_string(),
                assoc_name: assoc_name.to_string(),
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_env() {
        let env = MockEnv::new();
        let u8_ty = TypeExpr::Primitive("u8".to_string());
        assert!(env.is_copy(&u8_ty));

        let string_ty = TypeExpr::Composite {
            name: "String".to_string(),
            type_args: Vec::new(),
        };
        assert!(!env.is_copy(&string_ty));
    }

    #[test]
    fn test_trait_impl() {
        let mut env = MockEnv::new();
        env.add_trait_impl("String".to_string(), "Clone".to_string());

        let string_ty = TypeExpr::Composite {
            name: "String".to_string(),
            type_args: Vec::new(),
        };
        assert!(env.entails_trait(&string_ty, "Clone"));
        assert!(!env.entails_trait(&string_ty, "Copy"));
    }
}
