/// IR Graph 数据结构定义
///
/// 核心设计:类型节点是规范的(canonical),所有权信息在边上
use rustdoc_types::Id;
use std::collections::{HashMap, HashSet};

use crate::parse::ParsedCrate;

/// 类型节点:代表类型的规范形式
///
/// 重要:不为引用类型创建单独节点
/// 例如:u32, &u32, &mut u32 都映射到同一个 TypeNode(u32)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeNode {
    /// 原始类型:i32, u64, bool, str 等
    Primitive(String),

    /// 用户定义的结构体
    Struct(Option<Id>),

    /// 枚举类型
    Enum(Option<Id>),

    /// 联合体
    Union(Option<Id>),

    /// Trait 对象 (dyn Trait)
    TraitObject(Option<Id>),

    /// TODO: 多重 Trait 实现
    /// 如 impl<T> MyTrait for T + MyOtherTrait for T
    /// 如 where T: MyTrait + MyOtherTrait
    #[allow(unused)]
    MultiTrait(Vec<Id>),

    /// Constant 节点 (指向其类型)
    /// 例如: pub const STANDARD: Alphabet
    Constant {
        id: Id,
        name: String,
        type_id: Id,
        path: String,
    },

    /// Static 节点 (指向其类型)
    /// 例如: pub static mut GLOBAL: Config
    Static {
        id: Id,
        name: String,
        type_id: Id,
        path: String,
        is_mutable: bool,
    },

    /// 泛型参数节点(作用域化的一等类型)
    ///
    /// 例如:impl<T: Clone> Container<T> 中的 T
    /// 每个泛型参数都有明确的所有者和约束
    GenericParam {
        /// 参数名(如 "T", "U")
        name: String,
        /// 所有者 ID(定义该泛型的 Struct/Fn/Impl 的 Id)
        /// 用于区分不同作用域的同名泛型
        owner_id: Id,
        owner_name: String,
        /// Trait 约束(该泛型必须实现的 Trait)
        trait_bounds: Vec<Id>,
    },

    /// 元组类型(例如 (i32, String))
    Tuple(Vec<TypeNode>),

    /// 数组/切片的元素类型
    /// 注意:[T] 和 &[T] 的区别在 EdgeMode,这里只存 T
    Array(Box<TypeNode>),

    /// 函数指针类型(fn(A) -> B)
    #[allow(unused)]
    FnPointer {
        inputs: Vec<DataEdge>,
        output: Option<Box<DataEdge>>,
    },

    /// 单元类型 ()
    #[allow(unused)]
    Unit,

    /// Never 类型 !
    #[allow(unused)]
    Never,

    /// 关联类型/限定路径 <T as Trait>::Item
    QualifiedPath {
        parent: Box<TypeNode>,
        name: String,
        trait_id: Option<Id>,
    },

    /// 泛型实例化类型(如 Vec<u8>, HashMap<String, i32>)
    /// 保留完整的路径和泛型参数信息
    GenericInstance {
        /// 基础类型的 ID (如 Vec 的定义)
        base_id: Id,
        /// 完整路径 (如 "alloc::vec::Vec", "std::vec::Vec")
        path: String,
        /// 泛型参数列表
        type_args: Vec<TypeNode>,
    },

    /// 未知/不支持的类型
    Unknown,

    /// 不透明类型(外部类型,无法构建)
    #[allow(unused)]
    Opaque(String),
}

/// 边的模式:定义数据如何传递
///
/// 所有权信息存储在这里,而不是类型节点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeMode {
    /// 按值移动(所有权转移)
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

    #[allow(unused)]
    pub fn is_mutable(&self) -> bool {
        matches!(self, EdgeMode::MutRef | EdgeMode::MutRawPtr)
    }
}

/// 数据边:连接类型节点和操作节点
///
/// 包含类型信息和所有权模式
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataEdge {
    /// 指向的类型节点
    pub type_node: TypeNode,

    /// 数据传递方式
    pub mode: EdgeMode,

    /// 可选的名称(参数名)
    pub name: Option<String>,

    /// 参数位置索引（用于函数调用时排列参数）
    /// 对于输入边：表示这是第几个参数（从 0 开始）
    /// 对于输出边：None 或 Some(0) 表示主返回值
    pub param_index: Option<usize>,
}

impl DataEdge {
    /// 创建一个按值传递的边
    #[allow(dead_code)]
    pub fn move_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::Move,
            name,
            param_index: None,
        }
    }

    /// 创建一个共享引用边
    #[allow(dead_code)]
    pub fn ref_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::Ref,
            name,
            param_index: None,
        }
    }

    /// 创建一个可变引用边
    #[allow(dead_code)]
    pub fn mut_ref_edge(type_node: TypeNode, name: Option<String>) -> Self {
        Self {
            type_node,
            mode: EdgeMode::MutRef,
            name,
            param_index: None,
        }
    }
}

/// 操作节点类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpKind {
    /// 函数调用
    FnCall,

    /// 结构体构造器
    /// 输入:所有字段值 -> 输出:结构体实例
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
    /// 输入:结构体引用 &S 或 &mut S
    /// 输出:字段引用 &T 或 &mut T
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

    /// 关联函数(Associated function)
    AssocFn {
        /// 关联的类型
        assoc_type: Id,
    },

    /// Constant 别名操作
    /// 提供对 constant 的访问，返回其指向的类型
    ConstantAlias { const_id: Id, const_path: String },

    /// Static 别名操作
    /// 提供对 static 的访问，返回其指向的类型
    StaticAlias {
        static_id: Id,
        static_path: String,
        is_mutable: bool,
    },
}

/// 操作节点:表示一个可调用的转换
///
/// 统一处理函数、构造器等
#[derive(Debug, Clone)]
pub struct OpNode {
    /// 操作的唯一标识符(来自 rustdoc Item Id)
    pub id: Id,

    /// 操作名称
    pub name: String,

    /// 操作类型
    pub kind: OpKind,

    /// 输入边(参数)
    pub inputs: Vec<DataEdge>,

    /// 输出边(成功返回值)
    /// 对于 Result<T, E>: 这是 T
    /// 对于 Option<T>: 这是 Some(T)
    /// 对于其他类型: 就是返回值本身
    pub output: Option<DataEdge>,

    /// 错误输出边(仅用于 Result<T, E>)
    /// 当函数返回 Result<T, E> 时,这里存储错误类型 E
    /// 这样 Petri Net 中会有两条输出边,分别对应成功和失败路径
    pub error_output: Option<DataEdge>,

    /// 泛型参数约束
    /// 例如:fn foo<T: Display + Clone>(t: T)
    /// 这里存储 T -> [Display, Clone]
    pub generic_constraints: HashMap<String, Vec<Id>>,

    /// 文档注释 (从 rustdoc 提取)
    /// 用于在可视化时显示函数的说明文档
    pub docs: Option<String>,

    /// 是否是 unsafe 函数
    pub is_unsafe: bool,

    /// 是否是 const 函数
    #[allow(unused)]
    pub is_const: bool,

    /// 可见性
    #[allow(unused)]
    pub is_public: bool,

    /// 是否可能失败(返回 Result/Option)
    /// 如果为 true,则 error_output 可能有值
    pub is_fallible: bool,
}

impl OpNode {
    pub fn is_generic(&self) -> bool {
        !self.generic_constraints.is_empty()
    }

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

/// IR 图:整个程序的中间表示
#[derive(Debug)]
pub struct IrGraph {
    /// 所有类型节点
    pub type_nodes: HashSet<TypeNode>,

    /// 所有操作节点
    pub operations: Vec<OpNode>,

    /// 类型名称映射(用于调试和导出)
    pub type_names: HashMap<TypeNode, String>,
    
    /// 类型路径映射(用于代码生成)
    pub type_paths: HashMap<TypeNode, String>,

    /// Trait 实现映射(类型 -> 它实现的 Trait 列表)
    /// 用于解析泛型约束
    pub trait_impls: HashMap<Id, Vec<Id>>,

    /// Trait 实现详细信息列表
    pub trait_impl_details: Vec<TraitImpl>,

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
            type_paths: HashMap::new(),
            trait_impls: parsed_crate.trait_implementations.clone(),
            trait_impl_details: Vec::new(),
            parsed_crate,
        }
    }

    /// 添加 Trait 实现关系
    pub fn add_trait_impl(&mut self, trait_impl: TraitImpl) {
        log::debug!(
            "添加 Trait 实现: 类型 {:?} 实现 Trait {:?}, 关联类型: {:?}",
            trait_impl.for_type,
            trait_impl.trait_id,
            trait_impl.assoc_type_aliases
        );
        self.trait_impl_details.push(trait_impl);
    }

    /// 添加类型节点
    pub fn add_type(&mut self, node: TypeNode, name: String) {
        log::debug!("Creating TypeNode: {:?} (ID/Name: {})", node, name);
        self.type_nodes.insert(node.clone());
        self.type_names.insert(node, name);
    }
    
    /// 添加类型并指定完整路径
    pub fn add_type_with_path(&mut self, node: TypeNode, name: String, path: String) {
        log::debug!("Creating TypeNode: {:?} (Name: {}, Path: {})", node, name, path);
        self.type_nodes.insert(node.clone());
        self.type_names.insert(node.clone(), name);
        self.type_paths.insert(node, path);
    }
    
    /// 获取类型的完整路径
    pub fn get_type_path(&self, node: &TypeNode) -> Option<&str> {
        self.type_paths.get(node).map(|s| s.as_str())
    }

    /// 添加操作节点
    pub fn add_operation(&mut self, op: OpNode) {
        log::debug!(
            "Creating OpNode: {} (Kind: {:?}, ID: {:?})",
            op.name,
            op.kind,
            op.id
        );
        // 自动收集操作中涉及的所有类型(包括基本类型)
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

    /// 确保类型有名称(用于基本类型)
    ///
    /// 递归处理复合类型(Array, Tuple),确保所有嵌套的类型节点都被添加
    fn ensure_type_name(&mut self, node: &TypeNode) {
        if !self.type_names.contains_key(node) {
            let name = match node {
                TypeNode::Primitive(name) => name.clone(),
                TypeNode::Unit => "()".to_string(),
                TypeNode::Never => "!".to_string(),

                // 数组类型: [T] 或 [T; N]
                // 递归确保元素类型也被添加
                TypeNode::Array(elem_type) => {
                    // 先确保元素类型被处理
                    self.ensure_type_name(elem_type);

                    // 获取元素类型名称
                    let elem_name = self
                        .type_names
                        .get(elem_type.as_ref())
                        .map(|s| s.as_str())
                        .unwrap_or("unknown");

                    format!("[{}]", elem_name)
                }

                // 元组类型: (T, U, ...)
                // 递归确保所有元素类型都被添加
                TypeNode::Tuple(elements) => {
                    // 先确保所有元素类型被处理
                    for elem in elements {
                        self.ensure_type_name(elem);
                    }

                    // 构建元组名称
                    let elem_names: Vec<String> = elements
                        .iter()
                        .map(|elem| {
                            self.type_names
                                .get(elem)
                                .map(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        })
                        .collect();

                    format!("({})", elem_names.join(", "))
                }

                TypeNode::GenericParam { name, .. } => name.clone(),
                TypeNode::QualifiedPath {
                    parent: _, name, ..
                } => format!("::{}", name),

                // 泛型实例化: Vec<u8>, HashMap<String, i32> 等
                TypeNode::GenericInstance {
                    path, type_args, ..
                } => {
                    // 先确保所有泛型参数类型被处理
                    for arg in type_args {
                        self.ensure_type_name(arg);
                    }

                    // 提取基础类型名称(从完整路径中)
                    let base_name = path.split("::").last().unwrap_or(path);

                    // 构建泛型参数字符串
                    let arg_names: Vec<String> = type_args
                        .iter()
                        .map(|arg| {
                            self.type_names
                                .get(arg)
                                .map(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string()
                        })
                        .collect();

                    if arg_names.is_empty() {
                        base_name.to_string()
                    } else {
                        format!("{}<{}>", base_name, arg_names.join(", "))
                    }
                }

                TypeNode::Unknown => {
                    log::info!("遇到未知类型,标记为 Unknown: {:?}", node);
                    "unknown".to_string()
                }
                TypeNode::Opaque(name) => name.clone(),

                // Constant 和 Static 使用其路径作为名称
                TypeNode::Constant { path, .. } => path.clone(),
                TypeNode::Static { path, .. } => path.clone(),

                // 尝试解析 Struct/Enum/Union/TraitObject 的名称,即便是外部类型
                TypeNode::Struct(id)
                | TypeNode::Enum(id)
                | TypeNode::Union(id)
                | TypeNode::TraitObject(id) => {
                    // 优先从 type_index 获取
                    if id.is_none() {
                        "unknown".to_string();
                    }
                    if let Some(item) = self.parsed_crate.type_index.get(&id.unwrap()) {
                        if let Some(name) = &item.name {
                            name.clone()
                        } else {
                            // 尝试从 paths 获取(外部 crate 的类型)
                            self.parsed_crate
                                .crate_data
                                .paths
                                .get(&id.unwrap())
                                .and_then(|path_info| path_info.path.last().cloned())
                                .unwrap_or_else(|| format!("ExternalType_{:?}", id))
                        }
                    } else {
                        // 不在 type_index 中,尝试从 paths 获取(外部 crate 的类型)
                        self.parsed_crate
                            .crate_data
                            .paths
                            .get(&id.unwrap())
                            .and_then(|path_info| path_info.path.last().cloned())
                            .unwrap_or_else(|| format!("ExternalType_{:?}", id))
                    }
                }

                TypeNode::FnPointer { .. } => "fn".to_string(),
                TypeNode::MultiTrait(_) => {
                    todo!("MultiTrait 类型名称生成");
                }
            };

            // 插入类型名称
            self.type_names.insert(node.clone(), name);

            // 同时将该类型节点添加到 type_nodes 集合中
            self.type_nodes.insert(node.clone());
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
