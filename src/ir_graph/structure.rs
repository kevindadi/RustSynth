/// IR Graph 数据结构定义
///
/// 核心设计：类型节点是规范的（canonical），所有权信息在边上
use rustdoc_types::Id;
use std::collections::{HashMap, HashSet};

use crate::parse::ParsedCrate;

/// 类型节点：代表类型的规范形式
///
/// 重要：不为引用类型创建单独节点
/// 例如：u32, &u32, &mut u32 都映射到同一个 TypeNode(u32)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeNode {
    /// 原始类型：i32, u64, bool, str 等
    Primitive(String),

    /// 用户定义的结构体
    Struct(Id),

    /// 枚举类型
    Enum(Id),

    /// 联合体
    Union(Id),

    /// Trait 对象 (dyn Trait)
    TraitObject(Id),

    /// 泛型参数节点（作用域化的一等类型）
    ///
    /// 例如：impl<T: Clone> Container<T> 中的 T
    /// 每个泛型参数都有明确的所有者和约束
    GenericParam {
        /// 参数名（如 "T", "U"）
        name: String,
        /// 所有者 ID（定义该泛型的 Struct/Fn/Impl 的 Id）
        /// 用于区分不同作用域的同名泛型
        owner_id: Id,
        /// Trait 约束（该泛型必须实现的 Trait）
        trait_bounds: Vec<Id>,
    },

    /// 元组类型（例如 (i32, String)）
    Tuple(Vec<TypeNode>),

    /// 数组/切片的元素类型
    /// 注意：[T] 和 &[T] 的区别在 EdgeMode，这里只存 T
    Array(Box<TypeNode>),

    /// 函数指针类型（fn(A) -> B）
    FnPointer {
        inputs: Vec<DataEdge>,
        output: Option<Box<DataEdge>>,
    },

    /// 单元类型 ()
    Unit,

    /// Never 类型 !
    Never,

    /// 未知/不支持的类型
    Unknown,
}

/// 边的模式：定义数据如何传递
///
/// 这是设计的核心：所有权信息存储在这里，而不是类型节点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeMode {
    /// 按值移动（所有权转移）
    Move,

    /// 共享引用 &T
    Ref,

    /// 可变引用 &mut T
    MutRef,

    /// 裸指针 *const T
    RawPtr,

    /// 可变裸指针 *mut T
    MutRawPtr,
}

impl EdgeMode {
    /// 判断是否是引用类型
    pub fn is_reference(&self) -> bool {
        matches!(self, EdgeMode::Ref | EdgeMode::MutRef)
    }

    /// 判断是否是裸指针
    pub fn is_raw_pointer(&self) -> bool {
        matches!(self, EdgeMode::RawPtr | EdgeMode::MutRawPtr)
    }

    /// 判断是否可变
    pub fn is_mutable(&self) -> bool {
        matches!(self, EdgeMode::MutRef | EdgeMode::MutRawPtr)
    }
}

/// 数据边：连接类型节点和操作节点
///
/// 包含类型信息和所有权模式
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataEdge {
    /// 指向的类型节点（规范类型）
    pub type_node: TypeNode,

    /// 数据传递方式
    pub mode: EdgeMode,

    /// 可选的名称（参数名）
    pub name: Option<String>,
}

impl DataEdge {
    /// 创建一个按值传递的边
    pub fn move_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::Move,
            name,
        }
    }

    /// 创建一个共享引用边
    pub fn ref_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::Ref,
            name,
        }
    }

    /// 创建一个可变引用边
    pub fn mut_ref_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::MutRef,
            name,
        }
    }
}

/// 操作节点类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    /// 函数调用
    FnCall,

    /// 结构体构造器
    /// 输入：所有字段值 -> 输出：结构体实例
    StructCtor,

    /// 枚举变体构造器
    VariantCtor {
        /// 枚举类型 Id
        enum_id: Id,
        /// 变体名称
        variant_name: String,
    },

    /// 联合体构造器
    UnionCtor,

    /// 字段访问器
    /// 输入：结构体引用 &S 或 &mut S
    /// 输出：字段引用 &T 或 &mut T
    ///
    /// 这允许 Petri Net 使用 struct S { field: T } 来满足需要 &T 的转换
    FieldAccessor {
        /// 字段名
        field_name: String,
        /// 所属结构体类型
        struct_type: Id,
    },

    /// 方法调用
    MethodCall {
        /// self 参数的类型
        self_type: TypeNode,
    },

    /// 关联函数（Associated function）
    AssocFn {
        /// 关联的类型
        assoc_type: Id,
    },
}

/// 操作节点：表示一个可调用的转换
///
/// 统一处理函数、构造器等
#[derive(Debug, Clone)]
pub struct OpNode {
    /// 操作的唯一标识符（来自 rustdoc Item Id）
    pub id: Id,

    /// 操作名称
    pub name: String,

    /// 操作类型
    pub kind: OpKind,

    /// 输入边（参数）
    pub inputs: Vec<DataEdge>,

    /// 输出边（返回值）
    /// 注意：函数可以返回引用，所以这里用 DataEdge 而不是 TypeNode
    pub output: Option<DataEdge>,

    /// 泛型参数约束
    /// 例如：fn foo<T: Display + Clone>(t: T)
    /// 这里存储 T -> [Display, Clone]
    pub generic_constraints: HashMap<String, Vec<Id>>,

    /// 是否是 unsafe 函数
    pub is_unsafe: bool,

    /// 是否是 const 函数
    pub is_const: bool,

    /// 可见性
    pub is_public: bool,
}

impl OpNode {
    /// 判断操作是否有泛型参数
    pub fn is_generic(&self) -> bool {
        !self.generic_constraints.is_empty()
    }

    /// 判断是否是构造器
    pub fn is_constructor(&self) -> bool {
        matches!(
            self.kind,
            OpKind::StructCtor | OpKind::VariantCtor { .. } | OpKind::UnionCtor
        )
    }

    /// 判断是否是字段访问器
    pub fn is_field_accessor(&self) -> bool {
        matches!(self.kind, OpKind::FieldAccessor { .. })
    }
}

/// IR 图：整个程序的中间表示
#[derive(Debug)]
pub struct IrGraph {
    /// 所有类型节点
    pub type_nodes: HashSet<TypeNode>,

    /// 所有操作节点
    pub operations: Vec<OpNode>,

    /// 类型名称映射（用于调试和导出）
    pub type_names: HashMap<TypeNode, String>,

    /// Trait 实现映射（类型 -> 它实现的 Trait 列表）
    /// 用于解析泛型约束
    pub trait_impls: HashMap<Id, Vec<Id>>,

    /// 原始解析数据
    parsed_crate: ParsedCrate,
}

impl IrGraph {
    /// 创建新的 IR 图
    pub fn new(parsed_crate: ParsedCrate) -> Self {
        Self {
            type_nodes: HashSet::new(),
            operations: Vec::new(),
            type_names: HashMap::new(),
            trait_impls: parsed_crate.trait_implementations.clone(),
            parsed_crate,
        }
    }

    /// 添加类型节点
    pub fn add_type(&mut self, node: TypeNode, name: String) {
        self.type_nodes.insert(node.clone());
        self.type_names.insert(node, name);
    }

    /// 添加操作节点
    pub fn add_operation(&mut self, op: OpNode) {
        // 自动收集操作中涉及的所有类型（包括基本类型）
        for input in &op.inputs {
            self.type_nodes.insert(input.type_node.clone());
            // 为基本类型自动生成名称
            self.ensure_type_name(&input.type_node);
        }
        if let Some(output) = &op.output {
            self.type_nodes.insert(output.type_node.clone());
            self.ensure_type_name(&output.type_node);
        }
        self.operations.push(op);
    }

    /// 确保类型有名称（用于基本类型）
    fn ensure_type_name(&mut self, node: &TypeNode) {
        if !self.type_names.contains_key(node) {
            let name = match node {
                TypeNode::Primitive(name) => name.clone(),
                TypeNode::Unit => "()".to_string(),
                TypeNode::Never => "!".to_string(),
                TypeNode::Array(_) => "Array".to_string(),
                TypeNode::Tuple(_) => "Tuple".to_string(),
                TypeNode::GenericParam { name, .. } => name.clone(),
                TypeNode::Unknown => "unknown".to_string(),
                _ => return, // Struct/Enum/Union/Trait 已经在 build_type_nodes 中添加
            };
            self.type_names.insert(node.clone(), name);
        }
    }

    /// 获取类型名称
    pub fn get_type_name(&self, node: &TypeNode) -> Option<&str> {
        self.type_names.get(node).map(|s| s.as_str())
    }

    /// 检查某个类型是否实现了指定的 Trait
    pub fn implements_trait(&self, type_id: &Id, trait_id: &Id) -> bool {
        self.trait_impls
            .get(trait_id)
            .map(|impls| impls.contains(type_id))
            .unwrap_or(false)
    }

    /// 查找从某个类型出发的所有操作
    pub fn operations_from_type(&self, type_node: &TypeNode) -> Vec<&OpNode> {
        self.operations
            .iter()
            .filter(|op| op.inputs.iter().any(|edge| &edge.type_node == type_node))
            .collect()
    }

    /// 查找产生某个类型的所有操作
    pub fn operations_to_type(&self, type_node: &TypeNode) -> Vec<&OpNode> {
        self.operations
            .iter()
            .filter(|op| {
                op.output
                    .as_ref()
                    .map(|edge| &edge.type_node == type_node)
                    .unwrap_or(false)
            })
            .collect()
    }

    /// 获取原始解析数据
    pub fn parsed_crate(&self) -> &ParsedCrate {
        &self.parsed_crate
    }

    /// 打印统计信息
    pub fn print_stats(&self) {
        println!("=== IR Graph 统计 ===");
        println!("类型节点数: {}", self.type_nodes.len());
        println!("操作节点数: {}", self.operations.len());

        let generic_ops = self.operations.iter().filter(|op| op.is_generic()).count();
        let constructors = self
            .operations
            .iter()
            .filter(|op| op.is_constructor())
            .count();
        let field_accessors = self
            .operations
            .iter()
            .filter(|op| op.is_field_accessor())
            .count();
        let unsafe_ops = self.operations.iter().filter(|op| op.is_unsafe).count();

        println!("  - 泛型操作: {}", generic_ops);
        println!("  - 构造器: {}", constructors);
        println!("  - 字段访问器: {}", field_accessors);
        println!("  - unsafe 操作: {}", unsafe_ops);

        // 统计边模式
        let mut mode_counts: HashMap<EdgeMode, usize> = HashMap::new();
        for op in &self.operations {
            for input in &op.inputs {
                *mode_counts.entry(input.mode).or_insert(0) += 1;
            }
            if let Some(output) = &op.output {
                *mode_counts.entry(output.mode).or_insert(0) += 1;
            }
        }

        println!("\n=== 边模式统计 ===");
        for (mode, count) in mode_counts {
            println!("  {:?}: {}", mode, count);
        }
    }
}
