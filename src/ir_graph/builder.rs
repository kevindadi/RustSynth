/// IR Graph 构建器
///
/// 负责将 ParsedCrate 转换为 IR Graph
/// 核心：将 BorrowedRef 和 RawPointer 映射到 EdgeMode
use rustdoc_types::{GenericBound, GenericParamDefKind, Id, Type};
use std::collections::HashMap;

use super::generic_scope::GenericScope;
use super::structure::{DataEdge, EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use crate::parse::{FunctionInfo, ParsedCrate, TypeKind, TypeRef};
use log::warn;

pub struct IrGraphBuilder {
    graph: IrGraph,
    /// 泛型作用域管理器
    generic_scope: GenericScope,
}

impl IrGraphBuilder {
    pub fn new(parsed_crate: ParsedCrate) -> Self {
        Self {
            graph: IrGraph::new(parsed_crate),
            generic_scope: GenericScope::new(),
        }
    }

    /// 构建 IR 图
    pub fn build(mut self) -> IrGraph {
        // 步骤 1: 添加所有类型节点
        self.build_type_nodes();

        // 步骤 2: 构建操作节点（函数，带泛型作用域）
        self.build_function_operations();

        // 步骤 3: 构建构造器操作
        self.build_constructor_operations();

        // 步骤 4: 处理 impl 块中的方法
        self.build_impl_methods();

        self.graph
    }

    /// 构建类型节点
    fn build_type_nodes(&mut self) {
        // 克隆类型列表以避免借用冲突
        let types = self.graph.parsed_crate().types.clone();

        for type_info in &types {
            let node = match type_info.kind {
                TypeKind::Struct => TypeNode::Struct(type_info.id),
                TypeKind::Enum => TypeNode::Enum(type_info.id),
                TypeKind::Union => TypeNode::Union(type_info.id),
                TypeKind::Trait => TypeNode::TraitObject(type_info.id),
                TypeKind::TypeAlias => {
                    // Type alias: 可能需要解析实际类型
                    // 暂时作为 Unknown 处理
                    warn!("遇到 TypeAlias，暂时标记为 Unknown: {:?}", type_info);
                    continue;
                }
            };

            self.graph.add_type(node, type_info.name.clone());
        }

        // 注意：不预先添加基本类型（i32, u64, str 等）
        // 它们会在实际使用时（通过 add_operation）自动添加到 type_nodes
    }

    /// 构建函数操作
    fn build_function_operations(&mut self) {
        let functions = self.graph.parsed_crate().functions.clone();

        for func_info in functions {
            if let Some(op) = self.build_operation_from_function(&func_info) {
                self.graph.add_operation(op);
            }
        }
    }

    /// 从函数信息构建操作节点
    fn build_operation_from_function(&mut self, func: &FunctionInfo) -> Option<OpNode> {
        // 步骤 1: 创建泛型作用域
        // 从 rustdoc 获取函数的泛型参数定义
        let func_item = self.graph.parsed_crate().type_index.get(&func.id)?;
        let generics = if let rustdoc_types::ItemEnum::Function(f) = &func_item.inner {
            &f.generics
        } else {
            return None;
        };

        // 创建泛型节点并推入作用域
        let generic_nodes = self.create_generic_nodes(func.id, generics);
        self.generic_scope.push_scope(func.id, generic_nodes);

        // 步骤 2: 解析输入参数（在泛型作用域中）
        let inputs: Vec<DataEdge> = func
            .inputs
            .iter()
            .enumerate()
            .filter_map(|(i, type_ref)| {
                let name = Some(format!("arg{}", i));
                self.type_ref_to_data_edge(type_ref, name)
            })
            .collect();

        // 步骤 3: 解析输出（在泛型作用域中）
        let output = func
            .output
            .as_ref()
            .and_then(|type_ref| self.type_ref_to_data_edge(type_ref, None));

        // 步骤 4: 构建泛型约束映射
        let mut generic_constraints: HashMap<String, Vec<Id>> = HashMap::new();
        for constraint in &func.generic_constraints {
            generic_constraints
                .entry(constraint.param_name.clone())
                .or_insert_with(Vec::new)
                .push(constraint.required_trait);
        }

        // 步骤 5: 弹出作用域
        self.generic_scope.pop_scope();

        Some(OpNode {
            id: func.id,
            name: func.name.clone(),
            kind: OpKind::FnCall,
            inputs,
            output,
            generic_constraints,
            is_unsafe: false, // TODO: 从 rustdoc 获取
            is_const: false,  // TODO: 从 rustdoc 获取
            is_public: true,  // TODO: 从 rustdoc 获取
        })
    }

    /// 从 rustdoc Generics 创建泛型节点
    fn create_generic_nodes(
        &self,
        owner_id: Id,
        generics: &rustdoc_types::Generics,
    ) -> HashMap<String, TypeNode> {
        let mut nodes = HashMap::new();

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                // 提取 trait bounds
                let trait_bounds: Vec<Id> = bounds
                    .iter()
                    .filter_map(|bound| {
                        if let GenericBound::TraitBound { trait_, .. } = bound {
                            Some(trait_.id)
                        } else {
                            None
                        }
                    })
                    .collect();

                let node = TypeNode::GenericParam {
                    name: param.name.clone(),
                    owner_id,
                    trait_bounds,
                };

                nodes.insert(param.name.clone(), node);
            }
        }

        nodes
    }

    /// 构建构造器和字段访问器操作
    fn build_constructor_operations(&mut self) {
        let types = self.graph.parsed_crate().types.clone();

        for type_info in types {
            match type_info.kind {
                TypeKind::Struct => {
                    self.build_struct_operations(&type_info);
                }
                TypeKind::Enum => {
                    self.build_enum_operations(&type_info);
                }
                TypeKind::Union => {
                    self.build_union_operations(&type_info);
                }
                _ => {}
            }
        }
    }

    /// 为结构体构建操作：构造器 + 字段访问器
    ///
    /// 示例：struct Config { pub port: u16 }
    /// 生成：
    /// 1. StructCtor: u16(Move) -> Config(Move)
    /// 2. FieldAccessor(port): Config(Ref) -> u16(Ref)
    /// 3. FieldAccessor(port_mut): Config(MutRef) -> u16(MutRef)
    fn build_struct_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let struct_type_node = TypeNode::Struct(type_info.id);

        // 步骤 1: 创建构造器
        // 只为有公开字段的结构体创建构造器
        let public_fields: Vec<_> = type_info.fields.iter().filter(|f| f.is_public).collect();

        if !public_fields.is_empty() {
            let inputs: Vec<DataEdge> = public_fields
                .iter()
                .filter_map(|field| {
                    let data_edge =
                        self.type_ref_to_data_edge(&field.field_type, Some(field.name.clone()))?;
                    Some(data_edge)
                })
                .collect();

            let output = Some(DataEdge {
                type_node: struct_type_node.clone(),
                mode: EdgeMode::Move,
                name: None,
            });

            let ctor_op = OpNode {
                id: type_info.id,
                name: format!("{}::new", type_info.name),
                kind: OpKind::StructCtor,
                inputs,
                output,
                generic_constraints: HashMap::new(),
                is_unsafe: false,
                is_const: false,
                is_public: true,
            };

            self.graph.add_operation(ctor_op);
        }

        // 步骤 2: 为每个公开字段创建访问器（Ref 和 MutRef）
        for field in &public_fields {
            // 解析字段类型
            if let Some(field_edge) = self.type_ref_to_data_edge(&field.field_type, None) {
                // 2a. 不可变访问器: &S -> &T
                let accessor_ref = OpNode {
                    id: type_info.id, // 暂时使用相同的 Id
                    name: format!("{}.{}", type_info.name, field.name),
                    kind: OpKind::FieldAccessor {
                        field_name: field.name.clone(),
                        struct_type: type_info.id,
                    },
                    inputs: vec![DataEdge {
                        type_node: struct_type_node.clone(),
                        mode: EdgeMode::Ref,
                        name: Some("self".to_string()),
                    }],
                    output: Some(DataEdge {
                        type_node: field_edge.type_node.clone(),
                        mode: EdgeMode::Ref,
                        name: Some(field.name.clone()),
                    }),
                    generic_constraints: HashMap::new(),
                    is_unsafe: false,
                    is_const: false,
                    is_public: true,
                };

                self.graph.add_operation(accessor_ref);

                // 2b. 可变访问器: &mut S -> &mut T
                let accessor_mut = OpNode {
                    id: type_info.id,
                    name: format!("{}.{}_mut", type_info.name, field.name),
                    kind: OpKind::FieldAccessor {
                        field_name: field.name.clone(),
                        struct_type: type_info.id,
                    },
                    inputs: vec![DataEdge {
                        type_node: struct_type_node.clone(),
                        mode: EdgeMode::MutRef,
                        name: Some("self".to_string()),
                    }],
                    output: Some(DataEdge {
                        type_node: field_edge.type_node.clone(),
                        mode: EdgeMode::MutRef,
                        name: Some(field.name.clone()),
                    }),
                    generic_constraints: HashMap::new(),
                    is_unsafe: false,
                    is_const: false,
                    is_public: true,
                };

                self.graph.add_operation(accessor_mut);
            }
        }
    }

    /// 处理 impl 块中的方法
    ///
    /// 策略：扁平化 impl 块
    /// - 不为 Trait 创建节点
    /// - impl 块中的方法直接关联到 Self 类型
    /// - 解析 Self 为具体类型
    fn build_impl_methods(&mut self) {
        let impl_blocks = self.graph.parsed_crate().impl_blocks.clone();

        for impl_block in impl_blocks {
            // 设置 Self 类型上下文
            let self_type_id = impl_block.for_type;

            // 处理 impl 的泛型参数
            let generic_nodes =
                if let Some(item) = self.graph.parsed_crate().type_index.get(&impl_block.id) {
                    if let rustdoc_types::ItemEnum::Impl(impl_item) = &item.inner {
                        self.create_generic_nodes(impl_block.id, &impl_item.generics)
                    } else {
                        HashMap::new()
                    }
                } else {
                    HashMap::new()
                };

            self.generic_scope
                .push_scope_with_self(impl_block.id, generic_nodes, self_type_id);

            // 遍历 impl 块中的所有 item
            for &item_id in &impl_block.items {
                if let Some(item) = self.graph.parsed_crate().type_index.get(&item_id) {
                    // 处理方法
                    if let rustdoc_types::ItemEnum::Function(func) = &item.inner {
                        if let Some(op) =
                            self.build_method_from_impl(item_id, &item.name, func, self_type_id)
                        {
                            self.graph.add_operation(op);
                        }
                    }
                }
            }

            // 退出作用域
            self.generic_scope.pop_scope();
        }
    }

    /// 从 impl 块中的方法构建操作节点
    fn build_method_from_impl(
        &self,
        method_id: Id,
        method_name: &Option<String>,
        func: &rustdoc_types::Function,
        self_type_id: Id,
    ) -> Option<OpNode> {
        let name = method_name.as_deref().unwrap_or("anonymous").to_string();

        // 解析输入参数（注意 Self 的解析）
        let inputs: Vec<DataEdge> = func
            .sig
            .inputs
            .iter()
            .filter_map(|(param_name, ty)| {
                self.extract_data_edge_from_type(ty, Some(param_name.clone()))
            })
            .collect();

        // 解析输出
        let output = func
            .sig
            .output
            .as_ref()
            .and_then(|ty| self.extract_data_edge_from_type(ty, None));

        // 创建泛型约束
        let generic_constraints = self.create_generic_constraints_from_generics(&func.generics);

        let kind = if inputs.first().map(|e| e.name.as_deref()) == Some(Some("self")) {
            // 如果第一个参数是 self，则是方法调用
            OpKind::MethodCall {
                self_type: TypeNode::Struct(self_type_id),
            }
        } else {
            // 否则是关联函数
            OpKind::AssocFn {
                assoc_type: self_type_id,
            }
        };

        Some(OpNode {
            id: method_id,
            name,
            kind,
            inputs,
            output,
            generic_constraints,
            is_unsafe: func.header.is_unsafe,
            is_const: func.header.is_const,
            is_public: true,
        })
    }

    /// 从 rustdoc Type 提取 DataEdge（处理 Self 解析）
    fn extract_data_edge_from_type(&self, ty: &Type, name: Option<String>) -> Option<DataEdge> {
        let (type_node, mode) = Self::extract_type_and_mode_with_self(ty, &self.generic_scope)?;

        Some(DataEdge {
            type_node,
            mode,
            name,
        })
    }

    /// 从 Generics 创建泛型约束映射
    fn create_generic_constraints_from_generics(
        &self,
        generics: &rustdoc_types::Generics,
    ) -> HashMap<String, Vec<Id>> {
        let mut constraints = HashMap::new();

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                let trait_ids: Vec<Id> = bounds
                    .iter()
                    .filter_map(|bound| {
                        if let GenericBound::TraitBound { trait_, .. } = bound {
                            Some(trait_.id)
                        } else {
                            None
                        }
                    })
                    .collect();

                if !trait_ids.is_empty() {
                    constraints.insert(param.name.clone(), trait_ids);
                }
            }
        }

        constraints
    }

    /// 为枚举构建操作：为每个变体创建构造器
    ///
    /// 示例：enum Option<T> { None, Some(T) }
    /// 生成：
    /// 1. None 构造器: () -> Option<T>
    /// 2. Some 构造器: T(Move) -> Option<T>
    fn build_enum_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let enum_type_node = TypeNode::Enum(type_info.id);

        for variant in &type_info.variants {
            // 构建变体构造器的输入
            let inputs: Vec<DataEdge> = variant
                .fields
                .iter()
                .filter(|f| f.is_public)
                .filter_map(|field| {
                    self.type_ref_to_data_edge(&field.field_type, Some(field.name.clone()))
                })
                .collect();

            let output = Some(DataEdge {
                type_node: enum_type_node.clone(),
                mode: EdgeMode::Move,
                name: None,
            });

            let variant_ctor = OpNode {
                id: variant.id,
                name: format!("{}::{}", type_info.name, variant.name),
                kind: OpKind::VariantCtor {
                    enum_id: type_info.id,
                    variant_name: variant.name.clone(),
                },
                inputs,
                output,
                generic_constraints: HashMap::new(),
                is_unsafe: false,
                is_const: false,
                is_public: true,
            };

            self.graph.add_operation(variant_ctor);
        }
    }

    /// 为联合体构建操作
    ///
    /// Union 类似于 struct，但所有字段共享内存
    fn build_union_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let union_type_node = TypeNode::Union(type_info.id);
        let public_fields: Vec<_> = type_info.fields.iter().filter(|f| f.is_public).collect();

        if !public_fields.is_empty() {
            let inputs: Vec<DataEdge> = public_fields
                .iter()
                .filter_map(|field| {
                    self.type_ref_to_data_edge(&field.field_type, Some(field.name.clone()))
                })
                .collect();

            let output = Some(DataEdge {
                type_node: union_type_node.clone(),
                mode: EdgeMode::Move,
                name: None,
            });

            let union_ctor = OpNode {
                id: type_info.id,
                name: format!("{}::new", type_info.name),
                kind: OpKind::UnionCtor,
                inputs,
                output,
                generic_constraints: HashMap::new(),
                is_unsafe: true, // Union 构造通常是 unsafe
                is_const: false,
                is_public: true,
            };

            self.graph.add_operation(union_ctor);
        }

        // Union 字段访问器（与 struct 类似）
        for field in &public_fields {
            if let Some(field_edge) = self.type_ref_to_data_edge(&field.field_type, None) {
                // 不可变访问器
                let accessor_ref = OpNode {
                    id: field.id,
                    name: format!("{}.{}", type_info.name, field.name),
                    kind: OpKind::FieldAccessor {
                        field_name: field.name.clone(),
                        struct_type: type_info.id,
                    },
                    inputs: vec![DataEdge {
                        type_node: union_type_node.clone(),
                        mode: EdgeMode::Ref,
                        name: Some("self".to_string()),
                    }],
                    output: Some(DataEdge {
                        type_node: field_edge.type_node.clone(),
                        mode: EdgeMode::Ref,
                        name: Some(field.name.clone()),
                    }),
                    generic_constraints: HashMap::new(),
                    is_unsafe: true, // Union 字段访问通常是 unsafe
                    is_const: false,
                    is_public: true,
                };

                self.graph.add_operation(accessor_ref);

                // 可变访问器
                let accessor_mut = OpNode {
                    id: field.id,
                    name: format!("{}.{}_mut", type_info.name, field.name),
                    kind: OpKind::FieldAccessor {
                        field_name: field.name.clone(),
                        struct_type: type_info.id,
                    },
                    inputs: vec![DataEdge {
                        type_node: union_type_node.clone(),
                        mode: EdgeMode::MutRef,
                        name: Some("self".to_string()),
                    }],
                    output: Some(DataEdge {
                        type_node: field_edge.type_node,
                        mode: EdgeMode::MutRef,
                        name: Some(field.name.clone()),
                    }),
                    generic_constraints: HashMap::new(),
                    is_unsafe: true,
                    is_const: false,
                    is_public: true,
                };

                self.graph.add_operation(accessor_mut);
            }
        }
    }

    /// 将 TypeRef 转换为 DataEdge
    ///
    /// **核心解析逻辑**：
    /// - TypeRef::Resolved(id) -> DataEdge { TypeNode::from_id(id), Move }
    /// - TypeRef::Generic(name) -> 从作用域解析
    /// - 如果是引用，需要在更高层处理（因为 TypeRef 可能不包含引用信息）
    fn type_ref_to_data_edge(&self, type_ref: &TypeRef, name: Option<String>) -> Option<DataEdge> {
        match type_ref {
            TypeRef::Resolved(id) => Some(DataEdge {
                type_node: TypeNode::Struct(*id), // 简化：假设都是 struct
                mode: EdgeMode::Move,
                name,
            }),

            TypeRef::Primitive(prim_name) => Some(DataEdge {
                type_node: TypeNode::Primitive(prim_name.clone()),
                mode: EdgeMode::Move,
                name,
            }),

            TypeRef::Generic(param_name) => {
                // 从作用域解析泛型参数
                let type_node = self.generic_scope.resolve(param_name)?;

                Some(DataEdge {
                    type_node,
                    mode: EdgeMode::Move,
                    name,
                })
            }

            TypeRef::ImplTrait(trait_ids) => {
                // impl Trait: 作为特殊的泛型参数处理
                // owner_id 使用 0 表示匿名 impl Trait
                Some(DataEdge {
                    type_node: TypeNode::GenericParam {
                        name: "impl_trait".to_string(),
                        owner_id: Id(0), // 使用 Id(0) 表示匿名 impl Trait
                        trait_bounds: trait_ids.clone(),
                    },
                    mode: EdgeMode::Move,
                    name,
                })
            }

            TypeRef::Composite(_) => {
                // 复合类型：元组、数组等
                // 暂时跳过复杂处理
                None
            }
        }
    }

    /// **关键函数**：从 rustdoc_types::Type 提取 TypeNode 和 EdgeMode（带 Self 解析）
    ///
    /// 这是设计的核心：
    /// 1. 处理 BorrowedRef 和 RawPointer
    /// 2. 解析 Self 为具体类型
    /// 3. 处理 QualifiedPath (<T as Trait>::Item)
    pub fn extract_type_and_mode_with_self(
        ty: &Type,
        scope: &GenericScope,
    ) -> Option<(TypeNode, EdgeMode)> {
        match ty {
            // 处理 Self 类型 (T)
            Type::Generic(name) if name == "Self" => {
                let self_id = scope.resolve_self()?;
                // 尝试获取具体类型节点（例如 Struct/Enum）
                // 这里简化为 Struct，实际应该根据 ID 查询 TypeIndex
                Some((TypeNode::Struct(self_id), EdgeMode::Move))
            }

            // 处理引用 (&T, &mut T)
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                // 递归调用 with_self
                let (inner_type, inner_mode) = Self::extract_type_and_mode_with_self(type_, scope)?;

                if inner_mode.is_reference() || inner_mode.is_raw_pointer() {
                    return Some((inner_type, inner_mode));
                }

                let mode = if *is_mutable {
                    EdgeMode::MutRef
                } else {
                    EdgeMode::Ref
                };
                Some((inner_type, mode))
            }

            // 处理裸指针 (*const T, *mut T)
            Type::RawPointer { is_mutable, type_ } => {
                // 递归调用 with_self
                let (inner_type, inner_mode) = Self::extract_type_and_mode_with_self(type_, scope)?;

                if inner_mode.is_raw_pointer() {
                    return Some((inner_type, inner_mode));
                }

                let mode = if *is_mutable {
                    EdgeMode::MutRawPtr
                } else {
                    EdgeMode::RawPtr
                };
                Some((inner_type, mode))
            }

            // 处理 Slice ([T])
            Type::Slice(elem_type) => {
                let (inner_type, _) = Self::extract_type_and_mode_with_self(elem_type, scope)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 处理 Array ([T; N])
            Type::Array { type_, .. } => {
                let (inner_type, _) = Self::extract_type_and_mode_with_self(type_, scope)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 处理 Tuple ((T, U))
            Type::Tuple(elements) => {
                let nodes: Option<Vec<TypeNode>> = elements
                    .iter()
                    .map(|ty| {
                        Self::extract_type_and_mode_with_self(ty, scope).map(|(node, _)| node)
                    })
                    .collect();
                nodes.map(|ns| (TypeNode::Tuple(ns), EdgeMode::Move))
            }

            // QualifiedPath: <T as Trait>::Item 或 Self::Item
            Type::QualifiedPath {
                self_type,
                trait_,
                name,
                ..
            } => {
                // 尝试解析 self_type
                let (inner_type, _) = Self::extract_type_and_mode_with_self(self_type, scope)?;
                let trait_id = trait_.as_ref().map(|path| path.id);

                Some((
                    TypeNode::QualifiedPath {
                        parent: Box::new(inner_type),
                        name: name.clone(),
                        trait_id,
                    },
                    EdgeMode::Move,
                ))
            }

            // 其他类型：不涉及递归的，可以安全委托给 extract_type_and_mode
            // (Primitive, ResolvedPath, Generic(非Self), ImplTrait, etc.)
            _ => Self::extract_type_and_mode(ty),
        }
    }

    /// **原有函数**：从 rustdoc_types::Type 提取 TypeNode 和 EdgeMode
    ///
    /// 这是设计的核心：如何处理 BorrowedRef 和 RawPointer
    ///
    /// ```ignore
    /// // 示例 rustdoc Type:
    /// Type::BorrowedRef {
    ///     lifetime: None,
    ///     is_mutable: false,
    ///     type_: Box<Type::Primitive("u32")>
    /// }
    /// ```
    ///
    /// 解析为:
    /// ```ignore
    /// DataEdge {
    ///     type_node: TypeNode::Primitive("u32"),  // 规范类型
    ///     mode: EdgeMode::Ref,                     // 引用信息在这里
    /// }
    /// ```
    pub fn extract_type_and_mode(ty: &Type) -> Option<(TypeNode, EdgeMode)> {
        match ty {
            // 原始类型：按值传递
            Type::Primitive(name) => Some((TypeNode::Primitive(name.clone()), EdgeMode::Move)),

            // 已解析的路径：Struct/Enum 等
            Type::ResolvedPath(path) => {
                // 简化：假设都是 Struct，实际需要查询类型
                Some((TypeNode::Struct(path.id), EdgeMode::Move))
            }

            // 1. &T 或 &mut T -> 提取内部类型 T，EdgeMode 记录引用信息
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                // 递归提取内部类型
                let (inner_type, inner_mode) = Self::extract_type_and_mode(type_)?;

                // 如果内部已经是引用，保持原样（避免 &&T）
                if inner_mode.is_reference() || inner_mode.is_raw_pointer() {
                    return Some((inner_type, inner_mode));
                }

                // 设置引用模式
                let mode = if *is_mutable {
                    EdgeMode::MutRef
                } else {
                    EdgeMode::Ref
                };

                Some((inner_type, mode))
            }

            // 2. *const T 或 *mut T -> 提取内部类型 T
            Type::RawPointer { is_mutable, type_ } => {
                let (inner_type, inner_mode) = Self::extract_type_and_mode(type_)?;

                // 如果内部已经是指针，保持原样
                if inner_mode.is_raw_pointer() {
                    return Some((inner_type, inner_mode));
                }

                let mode = if *is_mutable {
                    EdgeMode::MutRawPtr
                } else {
                    EdgeMode::RawPtr
                };

                Some((inner_type, mode))
            }

            // 3. 泛型参数
            // 注意：这里不应该创建新的泛型节点，应该从作用域解析
            // 这个函数主要用于处理 rustdoc Type，而不是 TypeRef
            Type::Generic(name) => Some((
                TypeNode::GenericParam {
                    name: name.clone(),
                    owner_id: Id(0), // TODO: 需要从作用域上下文获取 owner_id
                    trait_bounds: Vec::new(),
                },
                EdgeMode::Move,
            )),

            // 4. Slice: [T] -> 提取元素类型
            Type::Slice(elem_type) => {
                let (inner_type, _) = Self::extract_type_and_mode(elem_type)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 5. Array: [T; N] -> 提取元素类型
            Type::Array { type_, .. } => {
                let (inner_type, _) = Self::extract_type_and_mode(type_)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 6. 元组
            Type::Tuple(elements) => {
                let nodes: Option<Vec<TypeNode>> = elements
                    .iter()
                    .map(|ty| Self::extract_type_and_mode(ty).map(|(node, _)| node))
                    .collect();
                nodes.map(|ns| (TypeNode::Tuple(ns), EdgeMode::Move))
            }

            // 7. impl Trait
            Type::ImplTrait(bounds) => {
                let trait_bounds: Vec<Id> = bounds
                    .iter()
                    .filter_map(|bound| {
                        if let GenericBound::TraitBound { trait_, .. } = bound {
                            Some(trait_.id)
                        } else {
                            None
                        }
                    })
                    .collect();

                Some((
                    TypeNode::GenericParam {
                        name: "impl_trait".to_string(),
                        owner_id: Id(0), // 匿名 impl Trait（使用 Id(0) 作为特殊标识）
                        trait_bounds,
                    },
                    EdgeMode::Move,
                ))
            }

            // 8. 函数指针
            Type::FunctionPointer(_) => {
                // TODO: 完整处理函数指针
                warn!("遇到 FunctionPointer，暂时标记为 Unknown");
                Some((TypeNode::Unknown, EdgeMode::Move))
            }

            // 9. 其他类型
            _ => {
                warn!("遇到未知类型，标记为 Unknown: {:?}", ty);
                Some((TypeNode::Unknown, EdgeMode::Move))
            }
        }
    }
}

/// 便捷函数：从 ParsedCrate 构建 IR Graph
pub fn build_ir_graph(parsed_crate: ParsedCrate) -> IrGraph {
    IrGraphBuilder::new(parsed_crate).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_borrowed_ref() {
        // 测试 &u32 的解析
        let ty = Type::BorrowedRef {
            lifetime: None,
            is_mutable: false,
            type_: Box::new(Type::Primitive("u32".to_string())),
        };

        let (type_node, mode) = IrGraphBuilder::extract_type_and_mode(&ty).unwrap();

        assert_eq!(type_node, TypeNode::Primitive("u32".to_string()));
        assert_eq!(mode, EdgeMode::Ref);
    }

    #[test]
    fn test_extract_mut_ref() {
        // 测试 &mut u32 的解析
        let ty = Type::BorrowedRef {
            lifetime: None,
            is_mutable: true,
            type_: Box::new(Type::Primitive("u32".to_string())),
        };

        let (type_node, mode) = IrGraphBuilder::extract_type_and_mode(&ty).unwrap();

        assert_eq!(type_node, TypeNode::Primitive("u32".to_string()));
        assert_eq!(mode, EdgeMode::MutRef);
    }

    #[test]
    fn test_extract_raw_ptr() {
        // 测试 *const u32 的解析
        let ty = Type::RawPointer {
            is_mutable: false,
            type_: Box::new(Type::Primitive("u32".to_string())),
        };

        let (type_node, mode) = IrGraphBuilder::extract_type_and_mode(&ty).unwrap();

        assert_eq!(type_node, TypeNode::Primitive("u32".to_string()));
        assert_eq!(mode, EdgeMode::RawPtr);
    }
}
