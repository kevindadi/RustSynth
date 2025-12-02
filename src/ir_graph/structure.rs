use crate::ir_graph::NodeInfo;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
/// IR Graph 数据结构定义  
///
/// 使用 rustdoc Id 作为节点标识,详细信息通过 ParsedCrate 查询
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;

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
    /// 实现关系(类型实现 Trait)
    Implements,
    /// 约束关系(泛型需要满足 Trait)
    Require,
    /// 包含关系(类型包含泛型参数)
    Include,
    /// 类型别名(Associated Type 或 type alias)
    Alias,
    /// 实例化关系(Const/Static 是某个类型的实例)
    Instance,
    /// Result/Option 展开成功(Ok/Some)
    UnwrapOk,
    /// Result 展开失败(Err)
    UnwrapErr,
    /// Option 展开失败(None)
    UnwrapNone,
}

impl EdgeMode {
    #[allow(unused)]
    pub fn is_mutable(&self) -> bool {
        matches!(self, EdgeMode::MutRef | EdgeMode::MutPtr)
    }

    /// 判断是否是关系边(不是数据流)
    #[allow(unused)]
    pub fn is_relationship(&self) -> bool {
        matches!(
            self,
            EdgeMode::Implements
                | EdgeMode::Require
                | EdgeMode::Include
                | EdgeMode::Alias
                | EdgeMode::Instance
        )
    }
}

/// 类型关系边(用作 petgraph 边权重)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeRelation {
    /// 边的模式
    pub mode: EdgeMode,
    /// 可选的标签(例如字段名、变体名等)
    pub label: Option<String>,
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
    #[allow(unused)]
    Unit,
    Variant,
    Function,
    Primitive,     // 基本类型
    ImplMethod,    // 类型实现的方法
    TraitMethod,   // Trait 定义的方法
    UnwrapOp,      // Result/Option 展开操作
    ResultWrapper, // Result<T, E> 包装类型节点
    OptionWrapper, // Option<T> 包装类型节点
}

/// IR 图:整个程序的中间表示
#[derive(Debug)]
pub struct IrGraph {
    /// 节点是字符串(Id.0.to_string()),边是 TypeRelation
    pub type_graph: DiGraph<String, TypeRelation>,
    pub node_types: HashMap<NodeIndex, NodeType>,
    pub node_infos: HashMap<NodeIndex, NodeInfo>,
}

impl IrGraph {
    pub fn new() -> Self {
        Self {
            type_graph: DiGraph::new(),
            node_types: HashMap::new(),
            node_infos: HashMap::new(),
        }
    }

    /// 添加或获取类型节点
    /// 如果节点已存在,返回其 NodeIndex；否则创建新节点
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

    pub fn print_stats(&self) {
        log::info!("=== IR Graph 统计 ===");
        log::info!("节点数: {}", self.type_graph.node_count());
        log::info!("类型关系边数: {}", self.type_graph.edge_count());

        let mut move_edges = 0;
        let mut ref_edges = 0;
        let mut mut_ref_edges = 0;
        let mut implements_edges = 0;
        let mut require_edges = 0;

        let mut unwrap_ok_edges = 0;
        let mut unwrap_err_edges = 0;
        let mut unwrap_none_edges = 0;

        for edge in self.type_graph.edge_weights() {
            match edge.mode {
                EdgeMode::Move => move_edges += 1,
                EdgeMode::Ref => ref_edges += 1,
                EdgeMode::MutRef => mut_ref_edges += 1,
                EdgeMode::Implements => implements_edges += 1,
                EdgeMode::Require => require_edges += 1,
                EdgeMode::UnwrapOk => unwrap_ok_edges += 1,
                EdgeMode::UnwrapErr => unwrap_err_edges += 1,
                EdgeMode::UnwrapNone => unwrap_none_edges += 1,
                EdgeMode::Include
                | EdgeMode::Alias
                | EdgeMode::Instance
                | EdgeMode::Ptr
                | EdgeMode::MutPtr => {}
            }
        }

        log::info!(
            "\n边类型分布: \n
        - Move: {}
        - Ref: {}
        - MutRef: {}
        - Implements: {}
        - Require: {}
        - UnwrapOk: {}
        - UnwrapErr: {}
        - UnwrapNone: {}",
            move_edges,
            ref_edges,
            mut_ref_edges,
            implements_edges,
            require_edges,
            unwrap_ok_edges,
            unwrap_err_edges,
            unwrap_none_edges
        );
    }

    pub fn export_dot<P: AsRef<std::path::Path>>(
        &self,
        _: &crate::parse::ParsedCrate,
        path: P,
    ) -> std::io::Result<()> {
        let mut dot = String::from("digraph IrGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [style=filled];\n\n");

        // 遍历所有节点,根据类型设置样式
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
                Some(NodeType::ResultWrapper) => ("hexagon", "gold"), // Result 包装类型
                Some(NodeType::OptionWrapper) => ("hexagon", "khaki"), // Option 包装类型
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
                EdgeMode::Instance => ("cyan", "dotted", "instance"),
                EdgeMode::Ptr => ("gray", "dotted", "*const"),
                EdgeMode::MutPtr => ("gray", "dotted", "*mut"),
                EdgeMode::UnwrapOk => ("darkgreen", "bold", "Ok/Some"),
                EdgeMode::UnwrapErr => ("red", "bold", "Err"),
                EdgeMode::UnwrapNone => ("darkgray", "bold", "None"),
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
        // 将 NodeIndex 转换为 usize 以便序列化
        let node_infos_serializable: HashMap<usize, &NodeInfo> = self
            .node_infos
            .iter()
            .map(|(k, v)| (k.index(), v))
            .collect();

        let json = serde_json::json!({
            "nodes": self.type_graph.node_count(),
            "edges": self.type_graph.edge_count(),
            "node_types": self.node_types.len(),
            "node_infos": node_infos_serializable,
        });

        let json_str = serde_json::to_string_pretty(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json_str)?;
        Ok(())
    }
}
