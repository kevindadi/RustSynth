//! NodeInfo 系统：为 IrGraph 节点提供详细的类型信息
//!
//! 设计原则：
//! 1. 使用带数据的枚举（enum with associated data）区分不同节点类型
//! 2. 复用 EdgeMode 表示借用语义，避免重复定义
//! 3. 保存足够信息用于 Petri 网转换

use super::EdgeMode;
use petgraph::graph::NodeIndex;
use rustdoc_types::Id;
use serde::{Deserialize, Serialize};

/// 节点详细信息枚举
/// 每个变体对应一种节点类型，携带该类型特有的信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeInfo {
    /// 结构体
    Struct(StructInfo),
    /// 枚举
    Enum(EnumInfo),
    /// 联合体
    Union(UnionInfo),
    /// Trait 定义
    Trait(TraitInfo),
    /// 方法（包括 impl 方法和 trait 方法）
    Method(MethodInfo),
    /// 独立函数
    Function(FunctionInfo),
    /// 常量
    Constant(ConstantInfo),
    /// 静态变量
    Static(StaticInfo),
    /// 泛型参数
    Generic(GenericInfo),
    /// 类型别名
    TypeAlias(TypeAliasInfo),
    /// 基本类型
    Primitive(PrimitiveInfo),
    /// 元组类型
    Tuple(TupleInfo),
    /// 切片类型
    Slice(SliceInfo),
    /// 数组类型
    Array(ArrayInfo),
    /// 枚举变体
    Variant(VariantInfo),
    /// Result/Option 展开操作节点
    UnwrapOp(UnwrapOpInfo),
    /// 关联类型
    AssociatedType(AssociatedTypeInfo),
    /// dyn Trait 对象
    DynTrait(DynTraitInfo),
    /// 函数指针
    FunctionPointer(FunctionPointerInfo),
}

/// 完整路径信息（用于符号解析）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathInfo {
    /// 完整路径，如 "std::collections::HashMap"
    pub full_path: String,
    /// crate 名称
    pub crate_name: Option<String>,
    /// 模块路径
    pub module_path: Vec<String>,
    /// 项目名称
    pub name: String,
}

/// 参数信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    /// 参数名
    pub name: String,
    /// 参数类型的节点索引（如果已解析）
    pub type_node: Option<NodeIndex>,
    /// 借用模式（复用 EdgeMode）
    pub borrow_mode: EdgeMode,
    /// 是否是 self 参数
    pub is_self: bool,
    /// 原始类型字符串（用于调试）
    pub type_str: String,
}

/// 返回值信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnInfo {
    /// 返回类型的节点索引
    pub type_node: Option<NodeIndex>,
    /// 包装器类型（Result/Option 等）
    pub wrapper: Option<WrapperType>,
    /// 展开操作节点（用于 Result/Option 的分支处理）
    pub unwrap_node: Option<NodeIndex>,
    /// 原始类型字符串
    pub type_str: String,
}

/// 包装器类型（Result/Option 等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WrapperType {
    /// Result<T, E>
    Result {
        ok_type: Option<NodeIndex>,
        err_type: Option<NodeIndex>,
    },
    /// Option<T>
    Option { some_type: Option<NodeIndex> },
    /// Box<T>
    Box { inner_type: Option<NodeIndex> },
    /// Vec<T>
    Vec { elem_type: Option<NodeIndex> },
    /// 其他包装器
    Other { name: String },
}

/// Trait 实现信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitImplInfo {
    /// Trait 的节点索引
    pub trait_node: Option<NodeIndex>,
    /// Trait 的 Id（用于查找）
    pub trait_id: Option<Id>,
    /// Trait 名称
    pub trait_name: String,
    /// 是否是自动派生的（#[derive(...)]）
    pub is_derived: bool,
    /// 是否是默认实现（如 Default, Clone 等标准库 trait）
    pub is_default: bool,
}

/// 字段信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldInfo {
    /// 字段名（对于元组结构体是索引）
    pub name: String,
    /// 字段类型的节点索引
    pub type_node: Option<NodeIndex>,
    /// 字段类型字符串
    pub type_str: String,
    /// 可见性
    pub visibility: Visibility,
}

/// 可见性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Crate,
    Restricted,
}

/// 结构体信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructInfo {
    /// 路径信息
    pub path: PathInfo,
    /// 字段列表
    pub fields: Vec<FieldInfo>,
    /// 泛型参数节点
    pub generics: Vec<NodeIndex>,
    /// 实现的 Trait 列表
    pub trait_impls: Vec<TraitImplInfo>,
    /// 自带方法列表（impl Self 中的方法）
    pub methods: Vec<NodeIndex>,
    /// 是否是元组结构体
    pub is_tuple_struct: bool,
    /// 是否是单元结构体
    pub is_unit_struct: bool,
}

/// 枚举信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumInfo {
    pub path: PathInfo,
    /// 变体列表
    pub variants: Vec<NodeIndex>,
    pub generics: Vec<NodeIndex>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub methods: Vec<NodeIndex>,
}

/// 联合体信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnionInfo {
    pub path: PathInfo,
    pub fields: Vec<FieldInfo>,
    pub generics: Vec<NodeIndex>,
    pub trait_impls: Vec<TraitImplInfo>,
    pub methods: Vec<NodeIndex>,
}

/// 枚举变体信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantInfo {
    /// 变体名称
    pub name: String,
    /// 所属枚举的节点索引
    pub parent_enum: Option<NodeIndex>,
    /// 变体类型（包含字段信息）
    pub kind: VariantKind,
    /// 判别值（如果有）
    pub discriminant: Option<String>,
}

/// 变体类型
///
/// 所有变体类型都只存储字段的 NodeIndex，详细信息通过节点获取
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VariantKind {
    /// 单元变体：None
    Unit,
    /// 元组变体：Some(T) - 存储字段类型的 NodeIndex
    Tuple(Vec<NodeIndex>),
    /// 结构体变体：Struct { field: T } - 存储字段的 NodeIndex
    Struct(Vec<NodeIndex>),
}

/// Trait 信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitInfo {
    pub path: PathInfo,
    /// 关联类型
    pub associated_types: Vec<NodeIndex>,
    /// 关联常量
    pub associated_consts: Vec<NodeIndex>,
    /// 方法签名（trait 定义的方法）
    pub methods: Vec<NodeIndex>,
    /// 父 Trait（supertrait bounds）
    pub supertraits: Vec<NodeIndex>,
    /// 泛型参数
    pub generics: Vec<NodeIndex>,
    /// 是否是 auto trait
    pub is_auto: bool,
    /// 是否是 unsafe trait
    pub is_unsafe: bool,
}

/// 方法信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodInfo {
    /// 方法名
    pub name: String,
    /// 所属类型/Trait 的节点索引
    pub owner: Option<NodeIndex>,
    /// 参数列表（包括 self）
    pub params: Vec<ParamInfo>,
    /// 返回值信息
    pub return_info: ReturnInfo,
    /// 泛型参数
    pub generics: Vec<NodeIndex>,
    /// 是否是 const fn
    pub is_const: bool,
    /// 是否是 async fn
    pub is_async: bool,
    /// 是否是 unsafe fn
    pub is_unsafe: bool,
    /// 方法类型
    pub method_kind: MethodKind,
}

/// 方法类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodKind {
    /// impl Self 中的方法
    Inherent,
    /// impl Trait for Type 中的方法
    TraitImpl,
    /// trait 定义中的方法
    TraitDef,
}

/// 独立函数信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub path: PathInfo,
    pub params: Vec<ParamInfo>,
    pub return_info: ReturnInfo,
    pub generics: Vec<NodeIndex>,
    pub is_const: bool,
    pub is_async: bool,
    pub is_unsafe: bool,
}

/// 常量信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstantInfo {
    pub path: PathInfo,
    /// 类型节点
    pub type_node: Option<NodeIndex>,
    /// 初始值 token（字符串表示，用于 Petri 网初始标记）
    pub init_value: Option<String>,
    /// 类型字符串
    pub type_str: String,
}

/// 静态变量信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticInfo {
    pub path: PathInfo,
    pub type_node: Option<NodeIndex>,
    pub init_value: Option<String>,
    pub type_str: String,
    /// 是否可变
    pub is_mutable: bool,
}

/// 泛型参数信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericInfo {
    /// 泛型名称（如 T, U）
    pub name: String,
    /// 所属的类型/方法节点
    pub owner: Option<NodeIndex>,
    /// Trait bounds（需要满足的 Trait）
    pub bounds: Vec<NodeIndex>,
    /// 默认类型（如果有）
    pub default_type: Option<NodeIndex>,
}

/// 类型别名信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAliasInfo {
    pub path: PathInfo,
    /// 别名指向的类型节点
    pub aliased_type: Option<NodeIndex>,
    pub generics: Vec<NodeIndex>,
}

/// 基本类型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveInfo {
    /// 类型名称（如 u8, i32, bool, str）
    pub name: String,
}

/// 元组类型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TupleInfo {
    /// 元素类型节点列表
    pub elements: Vec<NodeIndex>,
}

/// 切片类型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceInfo {
    /// 元素类型节点
    pub element_type: Option<NodeIndex>,
    /// 元素类型字符串
    pub element_type_str: String,
}

/// 数组类型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrayInfo {
    /// 元素类型节点
    pub element_type: Option<NodeIndex>,
    /// 数组长度
    pub length: String,
    /// 元素类型字符串
    pub element_type_str: String,
}

/// 关联类型信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssociatedTypeInfo {
    /// 关联类型名称
    pub name: String,
    /// 所属 Trait 节点
    pub owner_trait: Option<NodeIndex>,
    /// 具体类型（如果已绑定）
    pub concrete_type: Option<NodeIndex>,
    /// Trait bounds
    pub bounds: Vec<NodeIndex>,
}

/// dyn Trait 对象信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynTraitInfo {
    /// 主 Trait 节点
    pub main_trait: Option<NodeIndex>,
    /// 额外的 Trait bounds
    pub additional_bounds: Vec<NodeIndex>,
    /// 生命周期（如果有）
    pub lifetime: Option<String>,
}

/// 函数指针信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionPointerInfo {
    /// 参数类型节点列表
    pub param_types: Vec<NodeIndex>,
    /// 返回类型节点
    pub return_type: Option<NodeIndex>,
    /// 是否 unsafe
    pub is_unsafe: bool,
    /// ABI（如 "C", "Rust"）
    pub abi: Option<String>,
}

/// Result/Option 展开操作节点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnwrapOpInfo {
    /// 操作类型
    pub op_kind: UnwrapOpKind,
    /// 输入类型节点（Result<T,E> 或 Option<T>）
    pub input_type: Option<NodeIndex>,
    /// 成功分支目标节点（T 或 Some(T)）
    pub success_branch: Option<NodeIndex>,
    /// 失败分支目标节点（E 或 None）
    pub failure_branch: Option<NodeIndex>,
    /// 所属方法节点
    pub owner_method: Option<NodeIndex>,
}

/// 展开操作类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnwrapOpKind {
    /// Result::unwrap / Option::unwrap
    Unwrap,
    /// Result::expect / Option::expect
    Expect,
    /// Result::unwrap_or / Option::unwrap_or
    UnwrapOr,
    /// Result::unwrap_or_default / Option::unwrap_or_default
    UnwrapOrDefault,
    /// Result::ok / Option::ok_or
    Ok,
    /// Result::err
    Err,
    /// Option::is_some
    IsSome,
    /// Option::is_none
    IsNone,
    /// Result::is_ok
    IsOk,
    /// Result::is_err
    IsErr,
    /// ? 操作符
    QuestionMark,
    /// match 分支
    Match,
}

impl NodeInfo {
    /// 获取节点的路径信息（如果有）
    pub fn path(&self) -> Option<&PathInfo> {
        match self {
            NodeInfo::Struct(info) => Some(&info.path),
            NodeInfo::Enum(info) => Some(&info.path),
            NodeInfo::Union(info) => Some(&info.path),
            NodeInfo::Trait(info) => Some(&info.path),
            NodeInfo::Function(info) => Some(&info.path),
            NodeInfo::Constant(info) => Some(&info.path),
            NodeInfo::Static(info) => Some(&info.path),
            NodeInfo::TypeAlias(info) => Some(&info.path),
            _ => None,
        }
    }

    /// 获取节点名称
    pub fn name(&self) -> &str {
        match self {
            NodeInfo::Struct(info) => &info.path.name,
            NodeInfo::Enum(info) => &info.path.name,
            NodeInfo::Union(info) => &info.path.name,
            NodeInfo::Trait(info) => &info.path.name,
            NodeInfo::Method(info) => &info.name,
            NodeInfo::Function(info) => &info.path.name,
            NodeInfo::Constant(info) => &info.path.name,
            NodeInfo::Static(info) => &info.path.name,
            NodeInfo::Generic(info) => &info.name,
            NodeInfo::TypeAlias(info) => &info.path.name,
            NodeInfo::Primitive(info) => &info.name,
            NodeInfo::Variant(info) => &info.name,
            NodeInfo::AssociatedType(info) => &info.name,
            NodeInfo::Tuple(_) => "(tuple)",
            NodeInfo::Slice(_) => "[slice]",
            NodeInfo::Array(_) => "[array]",
            NodeInfo::UnwrapOp(_) => "unwrap_op",
            NodeInfo::DynTrait(_) => "dyn",
            NodeInfo::FunctionPointer(_) => "fn_ptr",
        }
    }

    /// 获取泛型参数列表（如果有）
    pub fn generics(&self) -> &[NodeIndex] {
        match self {
            NodeInfo::Struct(info) => &info.generics,
            NodeInfo::Enum(info) => &info.generics,
            NodeInfo::Union(info) => &info.generics,
            NodeInfo::Trait(info) => &info.generics,
            NodeInfo::Method(info) => &info.generics,
            NodeInfo::Function(info) => &info.generics,
            NodeInfo::TypeAlias(info) => &info.generics,
            _ => &[],
        }
    }

    /// 获取方法列表（如果有）
    pub fn methods(&self) -> &[NodeIndex] {
        match self {
            NodeInfo::Struct(info) => &info.methods,
            NodeInfo::Enum(info) => &info.methods,
            NodeInfo::Union(info) => &info.methods,
            NodeInfo::Trait(info) => &info.methods,
            _ => &[],
        }
    }

    /// 获取 Trait 实现列表（如果有）
    pub fn trait_impls(&self) -> &[TraitImplInfo] {
        match self {
            NodeInfo::Struct(info) => &info.trait_impls,
            NodeInfo::Enum(info) => &info.trait_impls,
            NodeInfo::Union(info) => &info.trait_impls,
            _ => &[],
        }
    }

    /// 判断是否是类型定义节点
    pub fn is_type_def(&self) -> bool {
        matches!(
            self,
            NodeInfo::Struct(_)
                | NodeInfo::Enum(_)
                | NodeInfo::Union(_)
                | NodeInfo::TypeAlias(_)
        )
    }

    /// 判断是否是可调用节点
    pub fn is_callable(&self) -> bool {
        matches!(
            self,
            NodeInfo::Method(_) | NodeInfo::Function(_) | NodeInfo::FunctionPointer(_)
        )
    }

    /// 判断是否有初始 token（用于 Petri 网）
    pub fn has_initial_token(&self) -> bool {
        match self {
            NodeInfo::Constant(info) => info.init_value.is_some(),
            NodeInfo::Static(info) => info.init_value.is_some(),
            _ => false,
        }
    }

    /// 获取初始 token 值（用于 Petri 网）
    pub fn initial_token(&self) -> Option<&str> {
        match self {
            NodeInfo::Constant(info) => info.init_value.as_deref(),
            NodeInfo::Static(info) => info.init_value.as_deref(),
            _ => None,
        }
    }
}

impl PathInfo {
    pub fn new(full_path: &str, name: &str) -> Self {
        let parts: Vec<&str> = full_path.split("::").collect();
        let crate_name = parts.first().map(|s| s.to_string());
        let module_path = if parts.len() > 2 {
            parts[1..parts.len() - 1]
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        Self {
            full_path: full_path.to_string(),
            crate_name,
            module_path,
            name: name.to_string(),
        }
    }
}

impl ParamInfo {
    /// 从借用模式判断是否需要可变访问
    pub fn requires_mut(&self) -> bool {
        matches!(self.borrow_mode, EdgeMode::MutRef | EdgeMode::MutPtr)
    }

    /// 从借用模式判断是否是引用
    pub fn is_reference(&self) -> bool {
        matches!(
            self.borrow_mode,
            EdgeMode::Ref | EdgeMode::MutRef | EdgeMode::Ptr | EdgeMode::MutPtr
        )
    }
}

impl Default for StructInfo {
    fn default() -> Self {
        Self {
            path: PathInfo::default(),
            fields: Vec::new(),
            generics: Vec::new(),
            trait_impls: Vec::new(),
            methods: Vec::new(),
            is_tuple_struct: false,
            is_unit_struct: false,
        }
    }
}

impl Default for EnumInfo {
    fn default() -> Self {
        Self {
            path: PathInfo::default(),
            variants: Vec::new(),
            generics: Vec::new(),
            trait_impls: Vec::new(),
            methods: Vec::new(),
        }
    }
}

impl Default for MethodInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            owner: None,
            params: Vec::new(),
            return_info: ReturnInfo {
                type_node: None,
                wrapper: None,
                unwrap_node: None,
                type_str: String::new(),
            },
            generics: Vec::new(),
            is_const: false,
            is_async: false,
            is_unsafe: false,
            method_kind: MethodKind::Inherent,
        }
    }
}

/// 示例：从 rustdoc Item 创建 NodeInfo 并插入到 IrGraph
pub mod insert_example {
    use super::*;
    use crate::ir_graph::{IrGraph, EdgeMode};
    use rustdoc_types::{Item, Type};

    /// 从 rustdoc Item 插入节点到 IrGraph
    ///
    /// # 返回值
    /// - 新创建的 NodeIndex
    /// - 对应的 NodeInfo（调用者需要存入 node_infos HashMap）
    pub fn insert_struct_node(
        ir_graph: &mut IrGraph,
        item: &Item,
        struct_data: &rustdoc_types::Struct,
    ) -> (NodeIndex, NodeInfo) {
        // 1. 创建节点
        let name = item.name.as_deref().unwrap_or("anonymous");
        let node_idx = ir_graph.add_type_node(name);

        // 2. 构建路径信息
        let path = PathInfo::new(
            &format!("crate::{}", name), // 简化的路径
            name,
        );

        // 3. 构建 StructInfo
        let struct_info = StructInfo {
            path,
            fields: Vec::new(),      // 后续填充
            generics: Vec::new(),    // 后续填充
            trait_impls: Vec::new(), // 后续填充
            methods: Vec::new(),     // 后续填充
            is_tuple_struct: matches!(struct_data.kind, rustdoc_types::StructKind::Tuple(_)),
            is_unit_struct: matches!(struct_data.kind, rustdoc_types::StructKind::Unit),
        };

        (node_idx, NodeInfo::Struct(struct_info))
    }

    /// 插入方法节点并建立边关系
    pub fn insert_method_node(
        ir_graph: &mut IrGraph,
        method_name: &str,
        owner_node: NodeIndex,
        params: &[(String, Type)],
        return_type: Option<&Type>,
        is_const: bool,
    ) -> (NodeIndex, NodeInfo, Vec<(NodeIndex, NodeIndex, EdgeMode)>) {
        // 1. 创建方法节点
        let node_idx = ir_graph.add_type_node(method_name);

        // 2. 构建参数信息
        let mut param_infos = Vec::new();
        let mut edges_to_add = Vec::new();

        for (param_name, param_type) in params {
            // 确定借用模式
            let (borrow_mode, is_self) = match param_type {
                Type::BorrowedRef { is_mutable, .. } => {
                    let mode = if *is_mutable {
                        EdgeMode::MutRef
                    } else {
                        EdgeMode::Ref
                    };
                    (mode, param_name == "self")
                }
                Type::Generic(name) if name == "Self" => (EdgeMode::Move, true),
                _ => (EdgeMode::Move, false),
            };

            param_infos.push(ParamInfo {
                name: param_name.clone(),
                type_node: None, // 后续解析时填充
                borrow_mode,
                is_self,
                type_str: format!("{:?}", param_type),
            });

            // 如果是 self 参数，添加从 owner 到方法的边
            if is_self {
                edges_to_add.push((owner_node, node_idx, borrow_mode));
            }
        }

        // 3. 构建返回值信息
        let return_info = ReturnInfo {
            type_node: None,
            wrapper: detect_wrapper_type(return_type),
            unwrap_node: None,
            type_str: return_type.map(|t| format!("{:?}", t)).unwrap_or_default(),
        };

        // 4. 构建 MethodInfo
        let method_info = MethodInfo {
            name: method_name.to_string(),
            owner: Some(owner_node),
            params: param_infos,
            return_info,
            generics: Vec::new(),
            is_const,
            is_async: false,
            is_unsafe: false,
            method_kind: MethodKind::Inherent,
        };

        (node_idx, NodeInfo::Method(method_info), edges_to_add)
    }

    /// 插入常量节点（带初始 token）
    pub fn insert_constant_node(
        ir_graph: &mut IrGraph,
        item: &Item,
        const_data: &rustdoc_types::Constant,
    ) -> (NodeIndex, NodeInfo) {
        let name = item.name.as_deref().unwrap_or("CONST");
        let node_idx = ir_graph.add_type_node(name);

        let constant_info = ConstantInfo {
            path: PathInfo::new(&format!("crate::{}", name), name),
            type_node: None,
            init_value: const_data.value.clone(), // 初始 token
            type_str: const_data.expr.clone(),    // 使用表达式字符串
        };

        (node_idx, NodeInfo::Constant(constant_info))
    }

    /// 插入 UnwrapOp 节点（用于 Result/Option 展开）
    pub fn insert_unwrap_op_node(
        ir_graph: &mut IrGraph,
        op_kind: UnwrapOpKind,
        input_node: NodeIndex,
        success_node: Option<NodeIndex>,
        failure_node: Option<NodeIndex>,
    ) -> (NodeIndex, NodeInfo, Vec<(NodeIndex, NodeIndex, EdgeMode)>) {
        let op_name = match op_kind {
            UnwrapOpKind::Unwrap => "unwrap",
            UnwrapOpKind::QuestionMark => "?",
            UnwrapOpKind::Match => "match",
            UnwrapOpKind::IsSome => "is_some",
            UnwrapOpKind::IsNone => "is_none",
            UnwrapOpKind::IsOk => "is_ok",
            UnwrapOpKind::IsErr => "is_err",
            _ => "unwrap_op",
        };

        let node_idx = ir_graph.add_type_node(op_name);

        let unwrap_info = UnwrapOpInfo {
            op_kind,
            input_type: Some(input_node),
            success_branch: success_node,
            failure_branch: failure_node,
            owner_method: None,
        };

        // 建立边关系
        let mut edges = vec![
            // 输入到 unwrap 操作
            (input_node, node_idx, EdgeMode::Move),
        ];

        // 成功分支
        if let Some(success) = success_node {
            edges.push((node_idx, success, EdgeMode::Move));
        }

        // 失败分支
        if let Some(failure) = failure_node {
            edges.push((node_idx, failure, EdgeMode::Move));
        }

        (node_idx, NodeInfo::UnwrapOp(unwrap_info), edges)
    }

    /// 检测返回类型的包装器
    fn detect_wrapper_type(ty: Option<&Type>) -> Option<WrapperType> {
        let ty = ty?;

        if let Type::ResolvedPath(path) = ty {
            let name = path.path.split("::").last().unwrap_or(&path.path);

            match name {
                "Result" => Some(WrapperType::Result {
                    ok_type: None,
                    err_type: None,
                }),
                "Option" => Some(WrapperType::Option { some_type: None }),
                "Box" => Some(WrapperType::Box { inner_type: None }),
                "Vec" => Some(WrapperType::Vec { elem_type: None }),
                _ => None,
            }
        } else {
            None
        }
    }

    /// 添加 Trait 实现边
    pub fn add_trait_impl_edge(
        ir_graph: &mut IrGraph,
        type_node: NodeIndex,
        trait_node: NodeIndex,
        is_derived: bool,
    ) {
        // 添加 Implements 边
        ir_graph.add_type_relation(
            type_node,
            trait_node,
            EdgeMode::Implements,
            if is_derived {
                Some("derived".to_string())
            } else {
                None
            },
        );
    }

    /// 添加泛型约束边
    pub fn add_generic_bound_edge(
        ir_graph: &mut IrGraph,
        generic_node: NodeIndex,
        trait_node: NodeIndex,
    ) {
        ir_graph.add_type_relation(
            generic_node,
            trait_node,
            EdgeMode::Require,
            None,
        );
    }
}
