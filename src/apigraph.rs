//! API Graph - 二分图结构
//!
//! 节点分为两类:
//! - FunctionNode: 函数/方法
//! - TypeNode: 类型(不区分 own/shr/mut,借用是边的属性)
//!
//! 边上标注值传递模式 (PassingMode)

// indexmap 暂时未使用,保留以备扩展
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::type_model::{PassingMode, TypeKey};

/// 函数节点 ID
pub type FnNodeId = usize;

/// 类型节点 ID
pub type TypeNodeId = usize;

/// 函数节点
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionNode {
    /// 节点 ID
    pub id: FnNodeId,
    /// 函数全路径名
    pub path: String,
    /// 函数短名
    pub name: String,
    /// 是否是方法(有 self 参数)
    pub is_method: bool,
    /// 是否是入口函数(无需非 primitive 类型参数)
    pub is_entry: bool,
    /// 是否是 const fn
    pub is_const: bool,
    /// 是否是 0-ary producer(无输入参数的 const fn)
    pub is_const_producer: bool,
    /// 参数类型(不含 self)
    pub params: Vec<ParamInfo>,
    /// self 参数(如果是方法)
    pub self_param: Option<SelfParam>,
    /// 返回类型
    pub return_type: Option<TypeKey>,
    /// 返回模式
    pub return_mode: Option<PassingMode>,
    /// 生命周期绑定信息(支持多个生命周期)
    /// 如果返回引用,记录返回值绑定到哪个参数(0 = self, 1+ = params)
    pub lifetime_bindings: Vec<LifetimeBinding>,
}

/// 参数信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParamInfo {
    /// 参数名
    pub name: String,
    /// 参数的 base 类型(不含引用)
    pub base_type: TypeKey,
    /// 传递模式
    pub passing_mode: PassingMode,
}

/// Self 参数
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelfParam {
    /// Self 的 base 类型
    pub base_type: TypeKey,
    /// 传递模式
    pub passing_mode: PassingMode,
}

/// 生命周期绑定
/// 表示返回值的生命周期绑定到哪个参数
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifetimeBinding {
    /// 生命周期名称(如 'a)
    pub lifetime: String,
    /// 绑定到的参数索引(0 = self, 1+ = params)
    pub source_param_index: usize,
    /// 返回值是否是共享引用
    pub is_shared: bool,
}

impl LifetimeBinding {
    /// 获取主要绑定来源参数索引
    pub fn primary_source(bindings: &[LifetimeBinding]) -> Option<usize> {
        bindings.first().map(|b| b.source_param_index)
    }
}

/// 类型节点
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypeNode {
    /// 节点 ID
    pub id: TypeNodeId,
    /// 类型键
    pub type_key: TypeKey,
    /// 是否是 primitive 类型
    pub is_primitive: bool,
    /// 是否是 Copy 类型
    pub is_copy: bool,
}

/// 所有权类型(用于边的标注)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OwnershipType {
    /// 所有权 (own) - 持有值本身
    Own,
    /// 共享引用 (shr) - &T
    Shr,
    /// 可变借用 (mut) - &mut T
    Mut,
}

impl std::fmt::Display for OwnershipType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OwnershipType::Own => write!(f, "own"),
            OwnershipType::Shr => write!(f, "shr"),
            OwnershipType::Mut => write!(f, "mut"),
        }
    }
}

/// API Graph 边(函数 <-> 类型)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiEdge {
    /// 函数节点 ID
    pub fn_node: FnNodeId,
    /// 类型节点 ID
    pub type_node: TypeNodeId,
    /// 边的方向
    pub direction: EdgeDirection,
    /// 传递模式
    pub passing_mode: PassingMode,
    /// 所有权类型(own/shr/mut)
    pub ownership: OwnershipType,
    /// 是否需要解引用操作(从 shr/mut 获取 own 时)
    pub requires_deref: bool,
    /// 参数位置(如果是输入边)
    pub param_index: Option<usize>,
    /// 生命周期(可选,暂时忽略)
    pub lifetime: Option<String>,
}

/// 边的方向
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeDirection {
    /// 类型 → 函数(参数输入)
    Input,
    /// 函数 → 类型(返回值输出)
    Output,
}

/// API Graph(二分图)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiGraph {
    /// 函数节点
    pub fn_nodes: Vec<FunctionNode>,
    /// 类型节点
    pub type_nodes: Vec<TypeNode>,
    /// 边
    pub edges: Vec<ApiEdge>,
    /// 类型到节点 ID 的映射
    #[serde(skip)]
    pub type_to_node: HashMap<TypeKey, TypeNodeId>,
    /// 函数路径到节点 ID 的映射
    #[serde(skip)]
    pub fn_to_node: HashMap<String, FnNodeId>,
}

impl Default for ApiGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiGraph {
    /// 创建空的 API Graph
    pub fn new() -> Self {
        ApiGraph {
            fn_nodes: Vec::new(),
            type_nodes: Vec::new(),
            edges: Vec::new(),
            type_to_node: HashMap::new(),
            fn_to_node: HashMap::new(),
        }
    }

    /// 添加或获取类型节点
    pub fn get_or_create_type_node(&mut self, type_key: TypeKey) -> TypeNodeId {
        if let Some(&id) = self.type_to_node.get(&type_key) {
            return id;
        }

        let id = self.type_nodes.len();
        let is_primitive = type_key.is_primitive();
        let is_copy = type_key.is_copy();

        self.type_nodes.push(TypeNode {
            id,
            type_key: type_key.clone(),
            is_primitive,
            is_copy,
        });
        self.type_to_node.insert(type_key, id);
        id
    }

    /// 添加函数节点
    pub fn add_function_node(&mut self, node: FunctionNode) -> FnNodeId {
        let id = self.fn_nodes.len();
        self.fn_to_node.insert(node.path.clone(), id);
        self.fn_nodes.push(FunctionNode { id, ..node });
        id
    }

    /// 添加边
    pub fn add_edge(&mut self, edge: ApiEdge) {
        self.edges.push(edge);
    }

    /// 获取函数的输入边
    pub fn get_input_edges(&self, fn_id: FnNodeId) -> Vec<&ApiEdge> {
        self.edges
            .iter()
            .filter(|e| e.fn_node == fn_id && e.direction == EdgeDirection::Input)
            .collect()
    }

    /// 获取函数的输出边
    pub fn get_output_edges(&self, fn_id: FnNodeId) -> Vec<&ApiEdge> {
        self.edges
            .iter()
            .filter(|e| e.fn_node == fn_id && e.direction == EdgeDirection::Output)
            .collect()
    }

    /// 获取类型的生产者函数
    pub fn get_producers(&self, type_id: TypeNodeId) -> Vec<FnNodeId> {
        self.edges
            .iter()
            .filter(|e| e.type_node == type_id && e.direction == EdgeDirection::Output)
            .map(|e| e.fn_node)
            .collect()
    }

    /// 获取类型的消费者函数
    pub fn get_consumers(&self, type_id: TypeNodeId) -> Vec<FnNodeId> {
        self.edges
            .iter()
            .filter(|e| e.type_node == type_id && e.direction == EdgeDirection::Input)
            .map(|e| e.fn_node)
            .collect()
    }

    /// 生成 DOT 格式
    pub fn to_dot(&self) -> String {
        let mut dot = String::new();
        dot.push_str("digraph ApiGraph {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  fontname=\"Helvetica\";\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");
        dot.push_str("  edge [fontname=\"Helvetica\"];\n\n");

        // 类型节点 - 用椭圆表示
        dot.push_str("  // Type nodes (椭圆)\n");
        dot.push_str("  subgraph cluster_types {\n");
        dot.push_str("    label=\"Types\";\n");
        dot.push_str("    style=dashed;\n");
        dot.push_str("    color=gray;\n");
        for type_node in &self.type_nodes {
            let color = if type_node.is_primitive {
                "lightgray"
            } else {
                "lightblue"
            };
            let label = type_node.type_key.short_name();
            dot.push_str(&format!(
                "    T{} [label=\"{}\", shape=ellipse, style=filled, fillcolor={}];\n",
                type_node.id, label, color
            ));
        }
        dot.push_str("  }\n\n");

        // 函数节点 - 用方框表示
        dot.push_str("  // Function nodes (方框)\n");
        dot.push_str("  subgraph cluster_functions {\n");
        dot.push_str("    label=\"Functions\";\n");
        dot.push_str("    style=dashed;\n");
        dot.push_str("    color=blue;\n");
        for fn_node in &self.fn_nodes {
            let shape = if fn_node.is_entry {
                "doubleoctagon"
            } else {
                "box"
            };
            dot.push_str(&format!(
                "    F{} [label=\"{}\", shape={}, style=filled, fillcolor=palegreen];\n",
                fn_node.id, fn_node.path, shape
            ));
        }
        dot.push_str("  }\n\n");

        // 边
        dot.push_str("  // Edges\n");
        for edge in &self.edges {
            let (from, to, color, style) = match edge.direction {
                EdgeDirection::Input => {
                    let color = match edge.ownership {
                        OwnershipType::Own => "black",
                        OwnershipType::Shr => "blue",
                        OwnershipType::Mut => "red",
                    };
                    (
                        format!("T{}", edge.type_node),
                        format!("F{}", edge.fn_node),
                        color,
                        "solid",
                    )
                }
                EdgeDirection::Output => {
                    let color = match edge.ownership {
                        OwnershipType::Own => "black",
                        OwnershipType::Shr => "blue",
                        OwnershipType::Mut => "red",
                    };
                    (
                        format!("F{}", edge.fn_node),
                        format!("T{}", edge.type_node),
                        color,
                        "solid",
                    )
                }
            };

            let deref_mark = if edge.requires_deref { "*" } else { "" };
            let label = format!("{}{}[{}]", deref_mark, edge.passing_mode, edge.ownership);
            dot.push_str(&format!(
                "  {} -> {} [label=\"{}\", color={}, style={}];\n",
                from, to, label, color, style
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// 统计信息
    pub fn stats(&self) -> ApiGraphStats {
        let entry_fns = self.fn_nodes.iter().filter(|f| f.is_entry).count();
        let primitive_types = self.type_nodes.iter().filter(|t| t.is_primitive).count();

        ApiGraphStats {
            num_fn_nodes: self.fn_nodes.len(),
            num_type_nodes: self.type_nodes.len(),
            num_edges: self.edges.len(),
            num_entry_fns: entry_fns,
            num_primitive_types: primitive_types,
        }
    }
}

/// API Graph 统计
#[derive(Debug)]
pub struct ApiGraphStats {
    pub num_fn_nodes: usize,
    pub num_type_nodes: usize,
    pub num_edges: usize,
    pub num_entry_fns: usize,
    pub num_primitive_types: usize,
}

impl std::fmt::Display for ApiGraphStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ApiGraph: {} functions ({} entry), {} types ({} primitive), {} edges",
            self.num_fn_nodes,
            self.num_entry_fns,
            self.num_type_nodes,
            self.num_primitive_types,
            self.num_edges
        )
    }
}

/// 构造完整 Counter API Graph 的辅助函数，供多处测试复用。
///
/// API 设计：
/// ```text
/// Counter::new() -> Counter          // const producer, 0-ary
/// Counter::get(&self) -> i32         // &self → i32
/// Counter::inc(&mut self)            // &mut self → ()
/// Counter::into_value(self) -> i32   // self → i32
/// ```
/// 供测试使用的 Counter API Graph 构造函数
pub fn build_counter_api_graph() -> ApiGraph {
    let mut graph = ApiGraph::new();

    // --- 类型节点 ---
    let counter_tid = graph.get_or_create_type_node(TypeKey::path("Counter"));
    let i32_tid = graph.get_or_create_type_node(TypeKey::primitive("i32"));

    // --- Counter::new() -> Counter ---
    let new_fn = FunctionNode {
        id: 0,
        path: "Counter::new".to_string(),
        name: "new".to_string(),
        is_method: false,
        is_entry: true,
        is_const: true,
        is_const_producer: true,
        params: vec![],
        self_param: None,
        return_type: Some(TypeKey::path("Counter")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let new_fid = graph.add_function_node(new_fn);
    graph.add_edge(ApiEdge {
        fn_node: new_fid,
        type_node: counter_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    // --- Counter::get(&self) -> i32 ---
    let get_fn = FunctionNode {
        id: 0,
        path: "Counter::get".to_string(),
        name: "get".to_string(),
        is_method: true,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![],
        self_param: Some(SelfParam {
            base_type: TypeKey::path("Counter"),
            passing_mode: PassingMode::BorrowShr,
        }),
        return_type: Some(TypeKey::primitive("i32")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let get_fid = graph.add_function_node(get_fn);
    graph.add_edge(ApiEdge {
        fn_node: get_fid,
        type_node: counter_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::BorrowShr,
        ownership: OwnershipType::Shr,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });
    graph.add_edge(ApiEdge {
        fn_node: get_fid,
        type_node: i32_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    // --- Counter::inc(&mut self) ---
    let inc_fn = FunctionNode {
        id: 0,
        path: "Counter::inc".to_string(),
        name: "inc".to_string(),
        is_method: true,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![],
        self_param: Some(SelfParam {
            base_type: TypeKey::path("Counter"),
            passing_mode: PassingMode::BorrowMut,
        }),
        return_type: None,
        return_mode: None,
        lifetime_bindings: vec![],
    };
    let inc_fid = graph.add_function_node(inc_fn);
    graph.add_edge(ApiEdge {
        fn_node: inc_fid,
        type_node: counter_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::BorrowMut,
        ownership: OwnershipType::Mut,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });

    // --- Counter::into_value(self) -> i32 ---
    let into_fn = FunctionNode {
        id: 0,
        path: "Counter::into_value".to_string(),
        name: "into_value".to_string(),
        is_method: true,
        is_entry: false,
        is_const: false,
        is_const_producer: false,
        params: vec![],
        self_param: Some(SelfParam {
            base_type: TypeKey::path("Counter"),
            passing_mode: PassingMode::Move,
        }),
        return_type: Some(TypeKey::primitive("i32")),
        return_mode: Some(PassingMode::ReturnOwned),
        lifetime_bindings: vec![],
    };
    let into_fid = graph.add_function_node(into_fn);
    graph.add_edge(ApiEdge {
        fn_node: into_fid,
        type_node: counter_tid,
        direction: EdgeDirection::Input,
        passing_mode: PassingMode::Move,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: Some(0),
        lifetime: None,
    });
    graph.add_edge(ApiEdge {
        fn_node: into_fid,
        type_node: i32_tid,
        direction: EdgeDirection::Output,
        passing_mode: PassingMode::ReturnOwned,
        ownership: OwnershipType::Own,
        requires_deref: false,
        param_index: None,
        lifetime: None,
    });

    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_graph_creation() {
        let mut graph = ApiGraph::new();

        // 添加类型节点
        let counter_id = graph.get_or_create_type_node(TypeKey::path("Counter"));
        let _i32_id = graph.get_or_create_type_node(TypeKey::primitive("i32"));

        // 添加函数节点
        let new_fn = FunctionNode {
            id: 0,
            path: "Counter::new".to_string(),
            name: "new".to_string(),
            is_method: false,
            is_entry: true,
            is_const: true,
            is_const_producer: true,
            params: vec![],
            self_param: None,
            return_type: Some(TypeKey::path("Counter")),
            return_mode: Some(PassingMode::ReturnOwned),
            lifetime_bindings: vec![],
        };
        let new_id = graph.add_function_node(new_fn);

        // 添加边
        graph.add_edge(ApiEdge {
            fn_node: new_id,
            type_node: counter_id,
            direction: EdgeDirection::Output,
            passing_mode: PassingMode::ReturnOwned,
            ownership: OwnershipType::Own,
            requires_deref: false,
            param_index: None,
            lifetime: None,
        });

        assert_eq!(graph.fn_nodes.len(), 1);
        assert_eq!(graph.type_nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn test_counter_api_graph_full() {
        let graph = build_counter_api_graph();

        // --- 类型节点 ---
        assert_eq!(graph.type_nodes.len(), 2, "should have Counter and i32");
        let counter_tn = graph.type_nodes.iter().find(|t| t.type_key == TypeKey::path("Counter")).unwrap();
        let i32_tn = graph.type_nodes.iter().find(|t| t.type_key == TypeKey::primitive("i32")).unwrap();
        assert!(!counter_tn.is_primitive);
        assert!(!counter_tn.is_copy);
        assert!(i32_tn.is_primitive);
        assert!(i32_tn.is_copy);

        // --- 函数节点 ---
        assert_eq!(graph.fn_nodes.len(), 4, "should have new/get/inc/into_value");

        let new_fn = graph.fn_nodes.iter().find(|f| f.name == "new").unwrap();
        assert!(new_fn.is_entry, "new is entry (0-ary)");
        assert!(new_fn.is_const_producer, "new is const producer");
        assert!(!new_fn.is_method, "new is associated fn, not method");

        let get_fn = graph.fn_nodes.iter().find(|f| f.name == "get").unwrap();
        assert!(get_fn.is_method, "get takes &self");
        assert!(!get_fn.is_entry);
        assert_eq!(get_fn.self_param.as_ref().unwrap().passing_mode, PassingMode::BorrowShr);

        let inc_fn = graph.fn_nodes.iter().find(|f| f.name == "inc").unwrap();
        assert!(inc_fn.is_method, "inc takes &mut self");
        assert_eq!(inc_fn.self_param.as_ref().unwrap().passing_mode, PassingMode::BorrowMut);
        assert!(inc_fn.return_type.is_none(), "inc returns ()");

        let into_fn = graph.fn_nodes.iter().find(|f| f.name == "into_value").unwrap();
        assert!(into_fn.is_method, "into_value takes self");
        assert_eq!(into_fn.self_param.as_ref().unwrap().passing_mode, PassingMode::Move);

        // --- 边 ---
        assert_eq!(graph.edges.len(), 6, "new→Counter, get←&Counter, get→i32, inc←&mutCounter, into←Counter, into→i32");

        // producers of Counter: new
        let counter_producers = graph.get_producers(counter_tn.id);
        assert_eq!(counter_producers.len(), 1);
        assert_eq!(graph.fn_nodes[counter_producers[0]].name, "new");

        // consumers of Counter: get, inc, into_value
        let counter_consumers = graph.get_consumers(counter_tn.id);
        assert_eq!(counter_consumers.len(), 3, "get, inc, into_value all consume Counter");

        // producers of i32: get, into_value
        let i32_producers = graph.get_producers(i32_tn.id);
        assert_eq!(i32_producers.len(), 2, "get and into_value produce i32");

        // stats
        let stats = graph.stats();
        assert_eq!(stats.num_fn_nodes, 4);
        assert_eq!(stats.num_type_nodes, 2);
        assert_eq!(stats.num_edges, 6);
        assert_eq!(stats.num_entry_fns, 1);
        assert_eq!(stats.num_primitive_types, 1);
    }
}
