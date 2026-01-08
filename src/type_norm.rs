//! 类型归一化：将 rustdoc Type 转换为全称路径 TypeKey
//!
//! 关键策略：
//! - 所有类型用全称路径字符串作为 TypeKey
//! - 泛型参数不展开 (工程化简化)
//! - Primitive 用固定字符串
//! - &T / &mut T 提取 base type + capability
//! - Self 归约为 impl 的主体类型

use anyhow::{Context, Result};
use indexmap::IndexMap;
use rustdoc_types::{Crate, Id, Item, ItemEnum, Type};
use std::collections::{HashMap, HashSet};

use crate::model::{Capability, TypeKey};

/// 类型归一化上下文
pub struct TypeContext {
    /// Item ID -> 全称路径
    pub id_to_path: IndexMap<Id, String>,
    /// Item ID -> Item (用于查询)
    pub items: HashMap<Id, Item>,
    /// Copy 类型集合 (近似判断)
    pub copy_types: HashSet<String>,
    /// Crate 名称
    pub crate_name: String,
}

impl TypeContext {
    /// 从 Crate 构建类型上下文
    pub fn from_crate(krate: &Crate) -> Result<Self> {
        let mut id_to_path = IndexMap::new();
        let mut items = HashMap::new();
        let mut copy_types = HashSet::new();

        // 遍历所有 items，构建 ID -> 路径映射
        for (id, item) in &krate.index {
            items.insert(id.clone(), item.clone());

            // 构建全称路径
            let path = Self::build_item_path(item, &items)?;
            id_to_path.insert(id.clone(), path.clone());

            // 近似判断 Copy 类型
            if Self::is_likely_copy(item, &items) {
                copy_types.insert(path);
            }
        }

        // 添加常见的 Copy 类型
        for prim in &[
            "bool", "char", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32",
            "i64", "i128", "isize", "f32", "f64", "()",
        ] {
            copy_types.insert(prim.to_string());
        }

        // 获取 crate 名称
        let crate_name = if let Some(root_item) = items.get(&krate.root) {
            root_item.name.clone().unwrap_or_else(|| "crate".to_string())
        } else {
            "crate".to_string()
        };

        Ok(TypeContext {
            id_to_path,
            items,
            copy_types,
            crate_name,
        })
    }

    /// 构建 Item 的全称路径
    fn build_item_path(item: &Item, _items: &HashMap<Id, Item>) -> Result<String> {
        // 如果 item 有 links，优先使用
        if let Some(name) = &item.name {
            // 收集路径组件
            let components = vec![name.clone()];

            // 尝试从 crate_id 开始构建路径 (简化实现：使用 item.links)
            // 对于大多数情况，rustdoc 会提供合理的 name
            // 完整实现需要遍历父节点，这里简化为使用 name

            // 如果是模块/结构体等，可能需要前缀
            // 简化：直接使用 crate::name 格式
            if item.crate_id == 0 {
                // 当前 crate (crate_id 通常从 0 开始)
                return Ok(format!("crate::{}", components.join("::")));
            }
        }

        // 降级：使用 item id 作为标识
        Ok(format!("item_{:?}", item.id))
    }

    /// 近似判断类型是否实现 Copy
    fn is_likely_copy(item: &Item, _items: &HashMap<Id, Item>) -> bool {
        match &item.inner {
            ItemEnum::Struct(_) => {
                // 简化：假设小型结构体可能是 Copy
                // 完整实现需要检查 trait bounds
                false
            }
            ItemEnum::Enum(_) => false,
            ItemEnum::Primitive(_) => true,
            _ => false,
        }
    }

    /// 归一化类型：Type -> (TypeKey, Capability)
    ///
    /// 返回：(base_type_key, capability, is_copy)
    /// - 对 T: (typekey, Own, is_copy)
    /// - 对 &T: (typekey, Shr, true)
    /// - 对 &mut T: (typekey, Mut, false)
    pub fn normalize_type(&self, ty: &Type) -> Result<(TypeKey, Capability, bool)> {
        self.normalize_type_with_context(ty, None)
    }
    
    /// 归一化类型（带上下文，可替换 Self）
    pub fn normalize_type_with_context(
        &self,
        ty: &Type,
        self_type: Option<&TypeKey>,
    ) -> Result<(TypeKey, Capability, bool)> {
        match ty {
            Type::ResolvedPath(path) => {
                // 直接使用 path.path 字段，它已经包含了完整的类型路径
                // 例如: "Counter", "std::vec::Vec", "simple_counter::Counter"
                tracing::debug!("Normalizing ResolvedPath: path.path = '{}', path.id = {:?}", path.path, path.id);
                
                let type_key = if path.path.is_empty() {
                    // 降级：如果 path 为空，尝试通过 id 查找
                    let key = self.resolve_path_to_key(&path.id)?;
                    tracing::debug!("  path.path is empty, resolved via id: '{}'", key);
                    key
                } else {
                    // 标准化路径：去除 crate 前缀，统一使用 crate:: 格式
                    let normalized = if path.path.starts_with("crate::") {
                        path.path.clone()
                    } else if let Some(item) = self.items.get(&path.id) {
                        // 如果是当前 crate 的类型，添加 crate:: 前缀
                        if item.crate_id == 0 {
                            let result = format!("crate::{}", path.path);
                            tracing::debug!("  normalized to: '{}'", result);
                            result
                        } else {
                            tracing::debug!("  external crate, keeping: '{}'", path.path);
                            path.path.clone()
                        }
                    } else {
                        tracing::debug!("  item not found, keeping: '{}'", path.path);
                        path.path.clone()
                    };
                    normalized
                };
                
                let is_copy = self.copy_types.contains(&type_key);
                tracing::debug!("  final type_key = '{}', is_copy = {}", type_key, is_copy);
                Ok((type_key, Capability::Own, is_copy))
            }
            Type::Primitive(name) => {
                let type_key = name.clone();
                Ok((type_key, Capability::Own, true)) // primitives 都是 Copy
            }
            Type::BorrowedRef {
                is_mutable,
                type_: inner,
                ..
            } => {
                // 提取 base type
                let (base_key, _, _) = self.normalize_type_with_context(inner, self_type)?;
                let cap = if *is_mutable {
                    Capability::Mut
                } else {
                    Capability::Shr
                };
                let is_copy = cap == Capability::Shr; // 共享引用是 Copy
                Ok((base_key, cap, is_copy))
            }
            Type::Tuple(elements) => {
                if elements.is_empty() {
                    Ok(("()".to_string(), Capability::Own, true))
                } else {
                    // 简化：tuple 统一为 "tuple"
                    Ok(("tuple".to_string(), Capability::Own, false))
                }
            }
            Type::Slice(_) => Ok(("slice".to_string(), Capability::Own, false)),
            Type::Array { .. } => Ok(("array".to_string(), Capability::Own, true)),
            Type::Generic(name) => {
                // 泛型参数：如果有 self_type 上下文且名称是 Self，则替换
                tracing::debug!("Normalizing Generic: name = '{}', self_type = {:?}", name, self_type);
                if name == "Self" {
                    if let Some(self_ty) = self_type {
                        let is_copy = self.copy_types.contains(self_ty);
                        tracing::debug!("  Replacing Self with: '{}'", self_ty);
                        return Ok((self_ty.clone(), Capability::Own, is_copy));
                    } else {
                        tracing::debug!("  No self_type context, using $Self");
                    }
                }
                // 其他泛型参数：使用参数名作为 key
                Ok((format!("${}", name), Capability::Own, false))
            }
            Type::QualifiedPath { .. } => {
                // 简化处理
                Ok(("qualified_path".to_string(), Capability::Own, false))
            }
            Type::RawPointer { .. } => Ok(("raw_ptr".to_string(), Capability::Own, true)),
            Type::FunctionPointer(_) => Ok(("fn_ptr".to_string(), Capability::Own, true)),
            Type::ImplTrait(_) => Ok(("impl_trait".to_string(), Capability::Own, false)),
            Type::Infer => anyhow::bail!("无法归一化 Infer 类型"),
            Type::DynTrait(_) => Ok(("dyn_trait".to_string(), Capability::Own, false)),
            Type::Pat { .. } => Ok(("pattern_type".to_string(), Capability::Own, false)),
        }
    }

    /// 解析 Path ID 到 TypeKey
    fn resolve_path_to_key(&self, id: &Id) -> Result<TypeKey> {
        self.id_to_path
            .get(id)
            .cloned()
            .context(format!("找不到 ID {:?} 对应的类型路径", id))
    }

    /// 检查类型是否是 Copy
    pub fn is_copy(&self, type_key: &TypeKey) -> bool {
        self.copy_types.contains(type_key)
    }

    /// 获取 Item
    pub fn get_item(&self, id: &Id) -> Option<&Item> {
        self.items.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_normalization() {
        // 基础测试 (需要完整 crate 数据)
    }
}

