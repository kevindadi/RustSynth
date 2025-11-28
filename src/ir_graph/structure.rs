use petgraph::dot::{Config, Dot};
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
/// IR Graph 数据结构定义  
///
/// 核心设计:使用 rustdoc Id 作为节点标识，详细信息通过 ParsedCrate 查询
use rustdoc_types::{Id, Item, ItemEnum};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 边的模式:定义数据如何传递或关系
///
/// 所有权信息存储在这里,而不是类型节点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeMode {
    /// 按值移动(所有权转移)
    Move,
    /// 共享引用 &T
    Ref,
    /// 可变引用 &mut T
    MutRef,
    /// 裸指针 *const T
    Ptr,
    /// 可变裸指针 *mut T
    MutPtr,
    /// 实现关系（类型实现 Trait）
    Implements,
    /// 约束关系（泛型需要满足 Trait）
    Require,
    /// 包含关系（类型包含泛型参数）
    Include,
    /// 类型别名（Associated Type 或 type alias）
    Alias,
}

impl EdgeMode {
    /// 判断是否是引用类型
    pub fn is_reference(&self) -> bool {
        matches!(self, EdgeMode::Ref | EdgeMode::MutRef)
    }

    /// 判断是否是裸指针
    pub fn is_raw_pointer(&self) -> bool {
        matches!(self, EdgeMode::Ptr | EdgeMode::MutPtr)
    }

    #[allow(unused)]
    pub fn is_mutable(&self) -> bool {
        matches!(self, EdgeMode::MutRef | EdgeMode::MutPtr)
    }

    /// 判断是否是关系边（不是数据流）
    pub fn is_relationship(&self) -> bool {
        matches!(
            self,
            EdgeMode::Implements | EdgeMode::Require | EdgeMode::Include | EdgeMode::Alias
        )
    }
}

/// 类型关系边（用作 petgraph 边权重）
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeRelation {
    /// 边的模式
    pub mode: EdgeMode,
    /// 可选的标签（例如字段名、变体名等）
    pub label: Option<String>,
}

/// 数据流边:操作节点和类型节点之间的连接
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataEdge {
    /// 目标类型的 Id
    pub type_id: Id,

    /// 数据传递模式
    pub mode: EdgeMode,

    /// 参数名称（如果有）
    pub name: Option<String>,
}

/// 操作类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    /// 函数调用（包括构造函数如 new()）
    FnCall,

    /// 字段访问器
    /// 表示类型之间的"持有"关系：struct A { field: B }
    /// 在类型 A 和类型 B 之间创建边，边上标注字段名
    FieldAccessor { field_name: String },

    /// 方法调用
    MethodCall {
        /// self 参数的类型 Id
        self_id: Id,
    },

    /// 关联函数(Associated function)
    AssocFn {
        /// 关联的类型 Id
        assoc_id: Id,
    },

    /// Constant 别名操作
    /// 提供对 constant 的访问，返回其指向的类型
    ConstantAlias { const_id: Id, const_path: String },

    /// Static 别名操作
    StaticAlias {
        static_id: Id,
        static_path: String,
        is_mutable: bool,
    },

    /// Trait 方法调用
    TraitMethodCall { trait_id: Id, method_name: String },
}

/// 操作节点：表示可调用的操作(函数/方法/构造器/字段访问等)
#[derive(Debug, Clone)]
pub struct OpNode {
    /// 操作的唯一标识符(来自 rustdoc Item Id)
    pub id: Id,

    /// 操作名称
    pub name: String,

    /// 操作类型
    pub kind: OpKind,

    /// 输入参数
    pub inputs: Vec<DataEdge>,

    /// 输出类型（成功情况）
    pub output: Option<DataEdge>,

    /// 错误输出类型（失败情况）
    pub error_output: Option<DataEdge>,

    /// 泛型约束
    pub generic_constraints: Vec<(String, Vec<Id>)>, // (泛型参数名, trait bounds)

    /// 文档注释
    pub docs: Option<String>,

    /// 是否不安全
    pub is_unsafe: bool,

    /// 是否 const
    pub is_const: bool,

    /// 是否公开
    pub is_public: bool,

    /// 是否可能失败（有 Result 返回值）
    pub is_fallible: bool,
}

impl OpNode {
    /// 是否是泛型函数/方法
    pub fn is_generic(&self) -> bool {
        !self.generic_constraints.is_empty()
    }

    /// 是否是字段访问器
    pub fn is_field_accessor(&self) -> bool {
        matches!(self.kind, OpKind::FieldAccessor { .. })
    }
}

/// Trait 实现关系
#[derive(Debug, Clone)]
pub struct TraitImpl {
    /// 实现 Trait 的类型 ID
    pub for_type: Id,
    /// Trait ID
    pub trait_id: Id,
    /// 关联类型别名映射 (关联类型名 -> 具体类型 ID)
    /// 例如: "Config" -> GeneralPurposeConfig 的 ID
    pub assoc_type_aliases: HashMap<String, Id>,
    /// Impl 块 ID
    pub impl_id: Id,
}

/// 节点类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Struct,
    Union,
    Enum,
    Trait,
    TypeAlias,
    Generic,
    Constant,
    Static,
    Tuple,
    Unit,
    Variant,
    Function,
    Primitive,   // 基本类型
    ImplMethod,  // 类型实现的方法
    TraitMethod, // Trait 定义的方法
    UnwrapOp,    // Result/Option 展开操作
}

/// IR 图:整个程序的中间表示
#[derive(Debug)]
pub struct IrGraph {
    /// 节点是字符串（Id.0.to_string()），边是 TypeRelation
    pub type_graph: DiGraph<String, TypeRelation>,
    pub node_types: HashMap<NodeIndex, NodeType>,
}

impl IrGraph {
    /// 创建新的 IR 图
    pub fn new() -> Self {
        Self {
            type_graph: DiGraph::new(),
            node_types: HashMap::new(),
        }
    }

    /// 添加或获取类型节点
    /// 如果节点已存在，返回其 NodeIndex；否则创建新节点
    pub fn add_type_node(&mut self, id: &str) -> NodeIndex {
        self.type_graph.add_node(id.to_string())
    }

    /// 添加类型关系边
    pub fn add_type_relation(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        mode: EdgeMode,
        label: Option<String>,
    ) -> EdgeIndex {
        self.type_graph
            .add_edge(from, to, TypeRelation { mode, label })
    }

    /// 打印统计信息
    pub fn print_stats(&self) {
        println!("=== IR Graph 统计 ===");
        println!("节点数: {}", self.type_graph.node_count());
        println!("类型关系边数: {}", self.type_graph.edge_count());

        let mut move_edges = 0;
        let mut ref_edges = 0;
        let mut mut_ref_edges = 0;
        let mut implements_edges = 0;
        let mut require_edges = 0;

        for edge in self.type_graph.edge_weights() {
            match edge.mode {
                EdgeMode::Move => move_edges += 1,
                EdgeMode::Ref => ref_edges += 1,
                EdgeMode::MutRef => mut_ref_edges += 1,
                EdgeMode::Implements => implements_edges += 1,
                EdgeMode::Require => require_edges += 1,
                EdgeMode::Include | EdgeMode::Alias | EdgeMode::Ptr | EdgeMode::MutPtr => {}
            }
        }

        println!("\n边类型分布:");
        println!("  - Move: {}", move_edges);
        println!("  - Ref: {}", ref_edges);
        println!("  - MutRef: {}", mut_ref_edges);
        println!("  - Implements: {}", implements_edges);
        println!("  - Require: {}", require_edges);
    }

    pub fn export_dot<P: AsRef<std::path::Path>>(
        &self,
        parsed_crate: &crate::parse::ParsedCrate,
        path: P,
    ) -> std::io::Result<()> {
        use petgraph::visit::EdgeRef;
        use std::fmt::Write;

        let mut dot = String::from("digraph IrGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [style=filled];\n\n");

        // 遍历所有节点，根据类型设置样式
        for node_idx in self.type_graph.node_indices() {
            let node_label = &self.type_graph[node_idx];
            let node_type = self.node_types.get(&node_idx);

            let (shape, color) = match node_type {
                Some(NodeType::Struct) => ("circle", "lightcyan"),
                Some(NodeType::Enum) => ("circle", "lightyellow"),
                Some(NodeType::Union) => ("circle", "lightpink"),
                Some(NodeType::Trait) => ("circle", "lavender"),
                Some(NodeType::Generic) => ("circle", "lightgray"),
                Some(NodeType::Constant) => ("circle", "lightsalmon"),
                Some(NodeType::Static) => ("circle", "lightcoral"),
                Some(NodeType::Primitive) => ("circle", "lightblue"),
                Some(NodeType::ImplMethod) => ("box", "palegreen"),
                Some(NodeType::TraitMethod) => ("box", "plum"),
                Some(NodeType::UnwrapOp) => ("diamond", "wheat"),
                Some(NodeType::Function) => ("box", "lightgreen"),
                _ => ("circle", "white"),
            };

            writeln!(
                dot,
                "  \"{}\" [shape={}, fillcolor=\"{}\", label=\"{}\"];",
                node_idx.index(),
                shape,
                color,
                node_label.replace("\"", "\\\"")
            )
            .ok();
        }

        dot.push_str("\n  // Edges\n");

        // 遍历所有边
        for edge_ref in self.type_graph.edge_references() {
            let from = edge_ref.source();
            let to = edge_ref.target();
            let relation = edge_ref.weight();

            let (color, style, label) = match relation.mode {
                EdgeMode::Move => ("blue", "solid", "move"),
                EdgeMode::Ref => ("lightblue", "dashed", "&"),
                EdgeMode::MutRef => ("orange", "dashed", "&mut"),
                EdgeMode::Implements => ("green", "bold", "impl"),
                EdgeMode::Require => ("purple", "dashed", "requires"),
                EdgeMode::Include => ("brown", "solid", "has"),
                EdgeMode::Alias => ("pink", "dashed", "alias"),
                EdgeMode::Ptr => ("gray", "dotted", "*const"),
                EdgeMode::MutPtr => ("gray", "dotted", "*mut"),
            };

            let edge_label = relation.label.as_deref().unwrap_or(label);

            writeln!(
                dot,
                "  \"{}\" -> \"{}\" [label=\"{}\", color={}, style={}];",
                from.index(),
                to.index(),
                edge_label.replace("\"", "\\\""),
                color,
                style
            )
            .ok();
        }

        dot.push_str("}\n");
        std::fs::write(path, dot)?;
        Ok(())
    }

    pub fn export_json<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        let json = serde_json::json!({
            "nodes": self.type_graph.node_count(),
            "edges": self.type_graph.edge_count(),
            "node_types": self.node_types.len(),
        });

        let json_str = serde_json::to_string_pretty(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json_str)?;
        Ok(())
    }
}
