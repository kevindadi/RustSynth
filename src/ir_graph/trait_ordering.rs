//! Trait 偏序关系系统
//!
//! 用于判定某个类型是否满足泛型要求。支持：
//! 1. 直接实现检查（通过 Implements 边）
//! 2. 间接实现检查（通过 supertrait 关系）
//! 3. 标准库 Trait 支持（如 AsRef, Clone 等）
//! 4. 泛型约束满足性检查
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use crate::ir_graph::{IrGraph, TraitOrdering, TraitBound};
//! use petgraph::graph::NodeIndex;
//!
//! // 假设已经构建了 IR Graph
//! let graph: IrGraph = /* ... */;
//!
//! // 创建 Trait 偏序关系系统
//! let ordering = TraitOrdering::new(&graph);
//!
//! // 方式1: 通过 Trait 节点检查
//! let type_node: NodeIndex = /* ... */;
//! let trait_node: NodeIndex = /* ... */;
//! if ordering.type_implements(type_node, trait_node) {
//!     println!("类型实现了该 Trait");
//! }
//!
//! // 方式2: 通过 Trait 名称检查
//! if ordering.satisfies_bound(type_node, TraitBound::from("AsRef")) {
//!     println!("类型满足 AsRef 约束");
//! }
//!
//! // 方式3: 检查多个约束（所有约束都必须满足）
//! let bounds = vec![
//!     TraitBound::from("Clone"),
//!     TraitBound::from("Debug"),
//! ];
//! if ordering.satisfies_all_bounds(type_node, &bounds) {
//!     println!("类型满足所有约束");
//! }
//!
//! // 方式4: 查找所有满足约束的类型
//! let satisfying_types = ordering.find_types_satisfying(TraitBound::from("AsRef"));
//! println!("找到 {} 个类型满足 AsRef 约束", satisfying_types.len());
//! ```

use crate::ir_graph::structure::{EdgeMode, IrGraph};
use crate::ir_graph::node_info::NodeInfo;
use crate::ir_graph::utils::extract_type_name_from_path;
use crate::support_types::primitives::get_primitive_default_traits;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

/// Trait 偏序关系系统
///
/// 用于判定类型是否满足 Trait 要求，支持：
/// - 直接实现关系（通过 Implements 边）
/// - 间接实现关系（通过 supertrait 继承）
/// - 标准库 Trait 的隐式实现
#[derive(Debug, Clone)]
pub struct TraitOrdering {
    /// IR Graph 的引用（只读）
    graph: *const IrGraph,
    /// Trait 继承关系图：supertrait -> subtrait（子 Trait 继承父 Trait）
    /// 如果 A 是 B 的 supertrait，则 A -> B
    trait_inheritance: HashMap<NodeIndex, HashSet<NodeIndex>>,
    /// 类型到 Trait 的实现关系缓存：type_node -> 所有实现的 trait 节点集合
    type_impl_cache: HashMap<NodeIndex, HashSet<NodeIndex>>,
    /// Trait 名称到节点的映射（用于快速查找标准库 Trait）
    trait_name_to_node: HashMap<String, NodeIndex>,
    /// 标准库 Trait 列表（这些 Trait 可能不在 IR Graph 中，但需要支持）
    std_traits: HashSet<String>,
}

impl TraitOrdering {
    /// 从 IR Graph 构建 Trait 偏序关系系统
    pub fn new(graph: &IrGraph) -> Self {
        let mut ordering = Self {
            graph: graph as *const IrGraph,
            trait_inheritance: HashMap::new(),
            type_impl_cache: HashMap::new(),
            trait_name_to_node: HashMap::new(),
            std_traits: Self::init_std_traits(),
        };

        ordering.build_trait_inheritance();
        ordering.build_trait_name_map();
        ordering.build_type_impl_cache();

        ordering
    }

    /// 初始化标准库 Trait 列表
    fn init_std_traits() -> HashSet<String> {
        [
            // 核心 Trait
            "Copy", "Clone", "Sized", "Send", "Sync",
            // 比较 Trait
            "PartialEq", "Eq", "PartialOrd", "Ord",
            // 哈希 Trait
            "Hash",
            // 格式化 Trait
            "Debug", "Display",
            // 转换 Trait
            "AsRef", "AsMut", "Into", "From", "TryInto", "TryFrom",
            // 借用 Trait
            "Borrow", "BorrowMut", "ToOwned",
            // 默认值 Trait
            "Default",
            // 错误处理 Trait
            "Error",
            // 字符串 Trait
            "ToString",
            // 迭代器 Trait
            "Iterator", "IntoIterator",
            // 数值运算 Trait
            "Add", "Sub", "Mul", "Div", "Rem",
            "BitAnd", "BitOr", "BitXor", "Shl", "Shr",
            "Not", "Neg",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// 获取 IR Graph 的引用（安全访问）
    fn graph(&self) -> &IrGraph {
        unsafe { &*self.graph }
    }

    /// 构建 Trait 继承关系图
    ///
    /// 从 TraitInfo 的 super_traits 字段构建继承关系
    fn build_trait_inheritance(&mut self) {
        let graph = self.graph();
        
        // 先收集所有继承关系，避免借用冲突
        let mut inheritance_pairs = Vec::new();
        for (node_idx, node_info) in &graph.node_infos {
            if let NodeInfo::Trait(trait_info) = node_info {
                // 对于每个 supertrait，建立继承关系
                for &super_trait_idx in &trait_info.super_traits {
                    inheritance_pairs.push((super_trait_idx, *node_idx));
                }
            }
        }
        
        // 然后插入到 HashMap 中
        for (super_trait_idx, node_idx) in inheritance_pairs {
            self.trait_inheritance
                .entry(super_trait_idx)
                .or_insert_with(HashSet::new)
                .insert(node_idx);
        }
    }

    /// 构建 Trait 名称到节点的映射
    fn build_trait_name_map(&mut self) {
        let graph = self.graph();
        
        // 先收集所有映射关系，避免借用冲突
        let mut name_mappings = Vec::new();
        for (node_idx, node_info) in &graph.node_infos {
            if let NodeInfo::Trait(trait_info) = node_info {
                let trait_name = trait_info.path.name.clone();
                name_mappings.push((trait_name, *node_idx));
                
                // 也存储完整路径
                let full_path = trait_info.path.full_path.clone();
                if full_path != trait_info.path.name {
                    name_mappings.push((full_path, *node_idx));
                }
            }
        }
        
        // 然后插入到 HashMap 中
        for (name, node_idx) in name_mappings {
            self.trait_name_to_node.insert(name, node_idx);
        }
    }

    /// 构建类型到 Trait 的实现关系缓存
    ///
    /// 通过遍历 Implements 边来构建缓存
    fn build_type_impl_cache(&mut self) {
        let graph = self.graph();
        
        // 先收集所有实现关系，避免借用冲突
        let mut impl_pairs = Vec::new();
        for edge_ref in graph.type_graph.edge_references() {
            if edge_ref.weight().mode == EdgeMode::Implements {
                let type_node = edge_ref.source();
                let trait_node = edge_ref.target();
                impl_pairs.push((type_node, trait_node));
            }
        }
        
        // 然后插入到 HashMap 中
        for (type_node, trait_node) in impl_pairs {
            self.type_impl_cache
                .entry(type_node)
                .or_insert_with(HashSet::new)
                .insert(trait_node);
        }
    }

    /// 获取 Trait 的所有子 Trait（通过继承关系）
    ///
    /// 返回所有直接或间接继承自给定 Trait 的 Trait 集合
    fn get_all_subtraits(&self, trait_node: NodeIndex) -> HashSet<NodeIndex> {
        let mut result = HashSet::new();
        result.insert(trait_node); // 包含自身
        
        // 使用 DFS 遍历继承关系
        let mut visited = HashSet::new();
        let mut stack = vec![trait_node];
        
        while let Some(current) = stack.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current);
            result.insert(current);
            
            // 查找所有继承自 current 的 Trait
            if let Some(subtraits) = self.trait_inheritance.get(&current) {
                for &subtrait in subtraits {
                    if !visited.contains(&subtrait) {
                        stack.push(subtrait);
                    }
                }
            }
        }
        
        result
    }

    /// 检查类型是否直接实现某个 Trait
    ///
    /// 通过 Implements 边检查
    fn type_implements_direct(&self, type_node: NodeIndex, trait_node: NodeIndex) -> bool {
        let graph = self.graph();
        
        // 检查是否有直接的 Implements 边
        graph.type_graph
            .edges_connecting(type_node, trait_node)
            .any(|edge| edge.weight().mode == EdgeMode::Implements)
    }

    /// 检查类型是否实现某个 Trait（包括直接和间接实现）
    ///
    /// 间接实现通过 supertrait 关系判断：
    /// 如果类型实现 Trait A，而 Trait B 是 Trait A 的 supertrait，则类型也实现 Trait B
    pub fn type_implements(&self, type_node: NodeIndex, trait_node: NodeIndex) -> bool {
        // 1. 检查直接实现
        if self.type_implements_direct(type_node, trait_node) {
            return true;
        }

        // 2. 检查间接实现（通过 supertrait 关系）
        // 如果类型实现了某个 Trait，而该 Trait 继承自目标 Trait，则类型也实现目标 Trait
        
        // 获取类型实现的所有 Trait
        let implemented_traits = self.type_impl_cache
            .get(&type_node)
            .cloned()
            .unwrap_or_default();
        
        // 对于每个实现的 Trait，检查其是否继承自目标 Trait
        for impl_trait in implemented_traits {
            // 检查 impl_trait 是否继承自 trait_node（通过反向查找）
            if self.is_supertrait_of(trait_node, impl_trait) {
                return true;
            }
        }

        false
    }

    /// 检查 Trait A 是否是 Trait B 的 supertrait（直接或间接）
    ///
    /// 如果 A 是 B 的 supertrait，则实现 B 的类型也实现 A
    fn is_supertrait_of(&self, super_trait: NodeIndex, sub_trait: NodeIndex) -> bool {
        if super_trait == sub_trait {
            return true;
        }

        // 使用 DFS 查找继承链
        let graph = self.graph();
        let mut visited = HashSet::new();
        let mut stack = vec![sub_trait];
        
        while let Some(current) = stack.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current);
            
            // 检查当前 Trait 的 super_traits
            if let Some(NodeInfo::Trait(trait_info)) = graph.node_infos.get(&current) {
                for &super_idx in &trait_info.super_traits {
                    if super_idx == super_trait {
                        return true;
                    }
                    if !visited.contains(&super_idx) {
                        stack.push(super_idx);
                    }
                }
            }
        }
        
        false
    }

    /// 通过名称查找 Trait 节点
    ///
    /// 支持完整路径（如 "std::convert::AsRef"）和简单名称（如 "AsRef"）
    pub fn find_trait_node(&self, trait_name: &str) -> Option<NodeIndex> {
        // 先尝试完整路径
        if let Some(&node) = self.trait_name_to_node.get(trait_name) {
            return Some(node);
        }
        
        // 尝试简单名称
        let simple_name = extract_type_name_from_path(trait_name);
        self.trait_name_to_node.get(&simple_name).copied()
    }

    /// 检查类型是否满足泛型约束
    ///
    /// 给定一个类型节点和一个 Trait 节点（或名称），判断类型是否满足该约束
    pub fn satisfies_bound(&self, type_node: NodeIndex, trait_bound: TraitBound) -> bool {
        let graph = self.graph();
        
        // 获取 Trait 节点
        let trait_node = match trait_bound {
            TraitBound::Node(node) => node,
            TraitBound::Name(name) => {
                match self.find_trait_node(&name) {
                    Some(node) => node,
                    None => {
                        // 如果是标准库 Trait 但不在图中，检查基本类型的默认实现
                        if self.std_traits.contains(&name) {
                            return self.check_std_trait_for_type(type_node, &name);
                        }
                        return false;
                    }
                }
            }
        };

        // 检查类型是否实现该 Trait
        if self.type_implements(type_node, trait_node) {
            return true;
        }

        // 如果是基本类型，检查其默认 Trait 实现
        if let Some(NodeInfo::Primitive(prim_info)) = graph.node_infos.get(&type_node) {
            let default_traits = &prim_info.default_traits;
            let trait_name = self.get_trait_name(trait_node);
            
            if let Some(trait_name) = trait_name {
                let simple_name = extract_type_name_from_path(&trait_name);
                if default_traits.contains(&simple_name) {
                    return true;
                }
            }
        }

        false
    }

    /// 检查类型是否满足多个泛型约束（所有约束都必须满足）
    pub fn satisfies_all_bounds(&self, type_node: NodeIndex, bounds: &[TraitBound]) -> bool {
        bounds.iter().all(|bound| self.satisfies_bound(type_node, bound.clone()))
    }

    /// 检查类型是否满足多个泛型约束（至少满足一个）
    pub fn satisfies_any_bound(&self, type_node: NodeIndex, bounds: &[TraitBound]) -> bool {
        bounds.iter().any(|bound| self.satisfies_bound(type_node, bound.clone()))
    }

    /// 获取类型实现的所有 Trait（包括直接和间接实现）
    pub fn get_implemented_traits(&self, type_node: NodeIndex) -> HashSet<NodeIndex> {
        let mut result = HashSet::new();
        let graph = self.graph();
        
        // 获取直接实现的 Trait
        let direct_traits = self.type_impl_cache
            .get(&type_node)
            .cloned()
            .unwrap_or_default();
        
        result.extend(direct_traits.iter());
        
        // 对于每个直接实现的 Trait，添加其所有 supertrait
        for trait_node in direct_traits {
            // 获取该 Trait 的所有 supertrait
            if let Some(NodeInfo::Trait(trait_info)) = graph.node_infos.get(&trait_node) {
                let mut stack = trait_info.super_traits.clone();
                let mut visited = HashSet::new();
                
                while let Some(super_trait) = stack.pop() {
                    if visited.contains(&super_trait) {
                        continue;
                    }
                    visited.insert(super_trait);
                    result.insert(super_trait);
                    
                    // 继续查找 supertrait 的 supertrait
                    if let Some(NodeInfo::Trait(super_trait_info)) = graph.node_infos.get(&super_trait) {
                        stack.extend(super_trait_info.super_traits.iter());
                    }
                }
            }
        }
        
        result
    }

    /// 获取 Trait 的名称
    fn get_trait_name(&self, trait_node: NodeIndex) -> Option<String> {
        let graph = self.graph();
        if let Some(NodeInfo::Trait(trait_info)) = graph.node_infos.get(&trait_node) {
            Some(trait_info.path.name.clone())
        } else {
            None
        }
    }

    /// 检查基本类型是否实现标准库 Trait
    fn check_std_trait_for_type(&self, type_node: NodeIndex, trait_name: &str) -> bool {
        let graph = self.graph();
        
        if let Some(NodeInfo::Primitive(prim_info)) = graph.node_infos.get(&type_node) {
            let default_traits = get_primitive_default_traits(&prim_info.name);
            default_traits.contains(&trait_name.to_string())
        } else {
            false
        }
    }

    /// 查找所有满足给定约束的类型
    ///
    /// 返回所有实现指定 Trait 的类型节点集合
    pub fn find_types_satisfying(&self, trait_bound: TraitBound) -> HashSet<NodeIndex> {
        let graph = self.graph();
        let mut result = HashSet::new();
        
        // 获取 Trait 节点
        let trait_node = match trait_bound {
            TraitBound::Node(node) => node,
            TraitBound::Name(name) => {
                match self.find_trait_node(&name) {
                    Some(node) => node,
                    None => return result, // Trait 不存在
                }
            }
        };

        // 遍历所有类型节点，检查是否实现该 Trait
        for (type_node, _) in &graph.node_infos {
            if self.type_implements(*type_node, trait_node) {
                result.insert(*type_node);
            }
        }

        result
    }
}

/// Trait 约束表示
#[derive(Debug, Clone)]
pub enum TraitBound {
    /// 通过节点索引指定
    Node(NodeIndex),
    /// 通过名称指定（支持完整路径或简单名称）
    Name(String),
}

impl From<NodeIndex> for TraitBound {
    fn from(node: NodeIndex) -> Self {
        TraitBound::Node(node)
    }
}

impl From<String> for TraitBound {
    fn from(name: String) -> Self {
        TraitBound::Name(name)
    }
}

impl From<&str> for TraitBound {
    fn from(name: &str) -> Self {
        TraitBound::Name(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_graph::structure::IrGraph;
    use crate::ir_graph::node_info::*;

    #[test]
    fn test_trait_ordering_creation() {
        let graph = IrGraph::new();
        let ordering = TraitOrdering::new(&graph);
        
        // 应该能创建成功
        assert!(!ordering.std_traits.is_empty());
    }

    #[test]
    fn test_find_trait_node() {
        // 这个测试需要实际的 IR Graph 数据
        // 暂时跳过，等待集成测试
    }
}
