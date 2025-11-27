/// IR Graph 构建器
///
/// 负责将 ParsedCrate 转换为 IR Graph
/// 将 BorrowedRef 和 RawPointer 映射到 EdgeMode
use rustdoc_types::{GenericBound, GenericParamDefKind, Id, Type, Visibility};
use std::collections::HashMap;

use super::generic_scope::GenericScope;
use super::structure::{DataEdge, EdgeMode, IrGraph, OpKind, OpNode, TypeNode};
use crate::parse::{FunctionInfo, ParsedCrate, TypeKind, TypeRef};
use log::warn;

pub struct IrGraphBuilder {
    graph: IrGraph,
    /// 泛型作用域管理器, 链接到类型定义的泛型
    generic_scope: GenericScope,
    /// 实例化类型的 ID 生成器, 用于 Vec<u8> 等已经实例化类型
    #[allow(dead_code)]
    next_synthetic_id: u32,
}

impl IrGraphBuilder {
    pub fn new(parsed_crate: ParsedCrate) -> Self {
        Self {
            graph: IrGraph::new(parsed_crate),
            generic_scope: GenericScope::new(),
            next_synthetic_id: u32::MAX,
        }
    }

    fn is_blacklisted_method(&self, name: &str) -> bool {
        crate::support_types::is_blacklisted_method(name)
    }

    /// 根据 Id 创建正确的 TypeNode
    ///
    /// 查询 parsed_crate.types 来确定实际的类型种类 (Struct/Enum/Union/Trait)
    /// 自动解析重导出(pub use)到规范定义
    fn create_type_node_from_id(&self, id: Id) -> TypeNode {
        let canonical_id = self.graph.parsed_crate().resolve_root_id(id);

        for type_info in &self.graph.parsed_crate().types {
            if type_info.id == canonical_id {
                return match type_info.kind {
                    TypeKind::Struct => TypeNode::Struct(Some(canonical_id)),
                    TypeKind::Enum => TypeNode::Enum(Some(canonical_id)),
                    TypeKind::Union => TypeNode::Union(Some(canonical_id)),
                    TypeKind::Trait => TypeNode::TraitObject(Some(canonical_id)),
                    TypeKind::TypeAlias => {
                        warn!("TypeAlias 暂时映射为 Struct: {:?}", type_info.name);
                        TypeNode::Struct(Some(canonical_id))
                    }
                };
            }
        }

        // 如果在 types 中找不到,尝试在 type_index 中查找
        if let Some(item) = self.graph.parsed_crate().type_index.get(&canonical_id) {
            match &item.inner {
                rustdoc_types::ItemEnum::Struct(_) => return TypeNode::Struct(Some(canonical_id)),
                rustdoc_types::ItemEnum::Enum(_) => return TypeNode::Enum(Some(canonical_id)),
                rustdoc_types::ItemEnum::Union(_) => return TypeNode::Union(Some(canonical_id)),
                rustdoc_types::ItemEnum::Trait(_) => {
                    return TypeNode::TraitObject(Some(canonical_id));
                }

                // 关联类型:需要解析到实际类型
                rustdoc_types::ItemEnum::AssocType { type_, bounds, .. } => {
                    // 如果关联类型有具体的 type,使用它
                    if let Some(actual_type) = type_ {
                        if let Some((type_node, _)) = self.extract_type_and_mode(actual_type) {
                            log::debug!("关联类型 {:?} 解析到实际类型: {:?}", item.name, type_node);
                            return type_node;
                        }
                    }

                    // 如果没有具体类型,查看 bounds
                    // 取第一个 trait bound 作为类型约束
                    for bound in bounds {
                        if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                            log::debug!(
                                "关联类型 {:?} 通过 trait bound 解析: Id({:?})",
                                item.name,
                                trait_.id
                            );
                            return TypeNode::TraitObject(Some(trait_.id));
                        }
                    }

                    warn!(
                        "关联类型无法解析,默认为 Struct: Id={:?}, name={:?}",
                        canonical_id, item.name
                    );
                }

                // Constant: 提取它指向的类型
                rustdoc_types::ItemEnum::Constant { type_, .. } => {
                    log::debug!("处理 constant {:?}, 提取其类型", item.name);
                    if let Some((type_node, _)) = self.extract_type_and_mode(type_) {
                        return type_node;
                    }
                    warn!(
                        "无法提取 constant 的类型: Id={:?}, name={:?}",
                        canonical_id, item.name
                    );
                }

                // Static 包含 type_ 字段
                rustdoc_types::ItemEnum::Static(static_data) => {
                    log::debug!("处理 static {:?}, 提取其类型", item.name);
                    if let Some((type_node, _)) = self.extract_type_and_mode(&static_data.type_) {
                        return type_node;
                    }
                    warn!(
                        "无法提取 static 的类型: Id={:?}, name={:?}",
                        canonical_id, item.name
                    );
                }

                _ => {
                    warn!("无法确定类型种类,默认为 Struct: Id={:?}", canonical_id);
                }
            }
        }

        TypeNode::Struct(Some(canonical_id))
    }

    /// 构建 IR 图
    pub fn build(mut self) -> IrGraph {
        // Step 1: 添加所有类型节点
        self.build_type_nodes();

        // Step 2: 构建操作节点(函数,带泛型作用域)
        self.build_function_operations();

        // Step 3: 构建构造器操作, pub 字段默认可以构造一个复合类型
        self.build_constructor_operations();

        // Step 4: 处理 impl 块中的方法
        self.build_impl_methods();

        self.graph
    }

    /// 构建类型节点
    fn build_type_nodes(&mut self) {
        let types = self.graph.parsed_crate().types.clone();

        for type_info in &types {
            let node = match type_info.kind {
                TypeKind::Struct => TypeNode::Struct(Some(type_info.id)),
                TypeKind::Enum => TypeNode::Enum(Some(type_info.id)),
                TypeKind::Union => TypeNode::Union(Some(type_info.id)),
                TypeKind::Trait => TypeNode::TraitObject(Some(type_info.id)),
                TypeKind::TypeAlias => {
                    warn!("遇到 TypeAlias,暂时标记为 Unknown: {:?}", type_info);
                    continue;
                }
            };

            self.graph.add_type(node, type_info.name.clone());

            // 为 Struct/Enum/Union 的泛型参数创建节点
            if matches!(
                type_info.kind,
                TypeKind::Struct | TypeKind::Enum | TypeKind::Union
            ) {
                if let Some(item) = self.graph.parsed_crate().type_index.get(&type_info.id) {
                    let generics = match &item.inner {
                        rustdoc_types::ItemEnum::Struct(s) => Some(&s.generics),
                        rustdoc_types::ItemEnum::Enum(e) => Some(&e.generics),
                        rustdoc_types::ItemEnum::Union(u) => Some(&u.generics),
                        _ => None,
                    };

                    if let Some(generics) = generics {
                        let generic_nodes =
                            self.create_generic_nodes(type_info.id, &type_info.name, generics);
                        for (param_name, generic_node) in generic_nodes {
                            self.graph.add_type(generic_node, param_name);
                        }
                    }
                }
            }
        }

        // 注意: ItemEnum::Use 在 extract_types 阶段已被过滤掉
        // 因为它们不会生成 TypeInfo,只有实际定义才会

        // 注意:不预先添加基本类型(i32, u64, str 等)
        // 它们会在实际使用时(通过 add_operation)自动添加到 type_nodes
    }

    /// 构建函数操作
    fn build_function_operations(&mut self) {
        let functions = self.graph.parsed_crate().functions.clone();

        // 收集所有在 impl 块中的函数 ID
        // 这些函数将在 build_impl_methods 中处理,避免重复
        let impl_method_ids: std::collections::HashSet<Id> = self
            .graph
            .parsed_crate()
            .impl_blocks
            .iter()
            .flat_map(|impl_block| impl_block.items.iter().copied())
            .collect();

        for func_info in functions {
            // 跳过 Trait 定义中的抽象方法
            if self.graph.parsed_crate().is_trait_method(&func_info.id) {
                log::debug!(
                    "跳过 Trait 抽象方法: {} (ID: {:?})",
                    func_info.name,
                    func_info.id
                );
                continue;
            }

            // 跳过 impl 块中的方法,它们会在 build_impl_methods 中处理
            if impl_method_ids.contains(&func_info.id) {
                log::debug!(
                    "跳过 impl 块方法(将在 impl 处理阶段创建): {} (ID: {:?})",
                    func_info.name,
                    func_info.id
                );
                continue;
            }

            if let Some(op) = self.build_operation_from_function(&func_info) {
                self.graph.add_operation(op);
            }
        }
    }

    /// 从函数信息构建操作节点
    fn build_operation_from_function(&mut self, func: &FunctionInfo) -> Option<OpNode> {
        // Step 1: 创建泛型作用域
        // 从 rustdoc 获取函数的泛型参数定义
        let func_item = self.graph.parsed_crate().type_index.get(&func.id)?;
        let rustdoc_func = if let rustdoc_types::ItemEnum::Function(f) = &func_item.inner {
            f
        } else {
            return None;
        };
        let generics = &rustdoc_func.generics;

        // 对于独立函数,将泛型参数解析为它们约束的 Trait(不创建 GenericParam 节点)
        let generic_nodes = self.create_generic_nodes(func.id, &func.name, generics);
        self.generic_scope.push_scope(func.id, generic_nodes);

        // Step 2: 解析输入参数(在泛型作用域中)
        // 直接从 rustdoc Type 解析,保留完整的引用和所有权信息
        let inputs: Vec<DataEdge> = rustdoc_func
            .sig
            .inputs
            .iter()
            .enumerate()
            .filter_map(|(i, (param_name, ty))| {
                let name = if param_name == "self" {
                    Some("self".to_string())
                } else if !param_name.is_empty() {
                    Some(param_name.clone())
                } else {
                    Some(format!("arg{}", i))
                };
                self.extract_data_edge_from_type(ty, name)
            })
            .collect();

        // Step 3: 解析输出(在泛型作用域中)- 提取 Result 的成功和错误分支
        let (output, error_output, is_fallible) = if let Some(ty) = &rustdoc_func.sig.output {
            let (success_ty, error_ty) = self.extract_result_branches(ty);
            let output = self.extract_data_edge_from_type(success_ty, None);
            let error_output = error_ty.and_then(|e| self.extract_data_edge_from_type(e, None));
            let is_fallible = error_ty.is_some();
            (output, error_output, is_fallible)
        } else {
            (None, None, false)
        };

        // Step 4: 构建泛型约束映射
        let mut generic_constraints: HashMap<String, Vec<Id>> = HashMap::new();
        for constraint in &func.generic_constraints {
            generic_constraints
                .entry(constraint.param_name.clone())
                .or_insert_with(Vec::new)
                .push(constraint.required_trait);
        }

        // Step 5: 弹出作用域
        self.generic_scope.pop_scope();

        // 提取文档注释: 如果链接到 llm 给它用的
        let docs = func_item.docs.clone();

        // 从 rustdoc 获取函数属性
        let is_unsafe = rustdoc_func.header.is_unsafe;
        let is_const = rustdoc_func.header.is_const;
        let is_public = matches!(func_item.visibility, Visibility::Public);

        Some(OpNode {
            id: func.id,
            name: func.name.clone(),
            kind: OpKind::FnCall,
            inputs,
            output,
            error_output,
            generic_constraints,
            docs,
            is_unsafe,
            is_const,
            is_public,
            is_fallible,
        })
    }

    /// 从 rustdoc Generics 创建泛型节点(用于类型定义:Struct/Enum/Union)
    fn create_generic_nodes(
        &self,
        owner_id: Id,
        owner_name: &str,
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
                    owner_name: owner_name.to_string(),
                    trait_bounds,
                };

                nodes.insert(param.name.clone(), node);
            }
        }

        nodes
    }

    /// 获取类型定义的泛型节点
    ///
    /// 用于 impl 块:impl 块应该使用类型定义的泛型参数,而不是创建新的
    /// 例如:impl<E, R> DecoderReader<E, R> 的 E 和 R 应该链接到 DecoderReader::E 和 DecoderReader::R
    fn get_type_generic_nodes(&self, type_id: Id) -> HashMap<String, TypeNode> {
        let mut nodes = HashMap::new();

        // 查找类型定义
        if let Some(item) = self.graph.parsed_crate().type_index.get(&type_id) {
            let type_name = item.name.as_deref().unwrap_or("unknown");

            // 获取类型的泛型参数
            let generics = match &item.inner {
                rustdoc_types::ItemEnum::Struct(s) => Some(&s.generics),
                rustdoc_types::ItemEnum::Enum(e) => Some(&e.generics),
                rustdoc_types::ItemEnum::Union(u) => Some(&u.generics),
                _ => None,
            };

            if let Some(generics) = generics {
                // 重建泛型节点(与类型定义时创建的一致)
                for param in &generics.params {
                    if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
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
                            owner_id: type_id, // 使用类型的 ID,不是 impl 的 ID
                            owner_name: type_name.to_string(),
                            trait_bounds,
                        };

                        nodes.insert(param.name.clone(), node);
                    }
                }
            }
        }

        nodes
    }

    /// 为独立函数创建泛型作用域(将泛型参数解析为 Trait,避免创建 GenericParam 节点)
    /// 所有指向该泛型 Trait 的类型都会调用此方法
    #[allow(dead_code)]
    fn create_generic_nodes_as_traits(
        &self,
        generics: &rustdoc_types::Generics,
    ) -> HashMap<String, TypeNode> {
        let mut nodes = HashMap::new();

        for param in &generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                // 提取第一个 trait bound 作为该泛型的类型
                let trait_node = bounds.iter().find_map(|bound| {
                    if let GenericBound::TraitBound { trait_, .. } = bound {
                        Some(TypeNode::TraitObject(Some(trait_.id)))
                    } else {
                        None
                    }
                });

                if let Some(node) = trait_node {
                    log::debug!("独立函数泛型参数 {} 解析为 Trait: {:?}", param.name, node);
                    nodes.insert(param.name.clone(), node);
                } else {
                    // 如果没有 trait bound, 则是最大泛型约束, 忽略
                    log::warn!("独立函数泛型参数 {} 没有 trait 约束", param.name);
                    nodes.insert(param.name.clone(), TypeNode::Unknown);
                }
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

    /// 为结构体构建操作:构造器 + 字段访问器
    ///
    /// 示例:struct Config { pub port: u16 }
    /// 生成:
    /// 1. StructCtor: u16(Move) -> Config(Move)
    /// 2. FieldAccessor(port): Config(Ref) -> u16(Ref)
    /// 3. FieldAccessor(port_mut): Config(MutRef) -> u16(MutRef)
    fn build_struct_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let struct_type_node = TypeNode::Struct(Some(type_info.id));

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
                error_output: None,
                generic_constraints: HashMap::new(),
                docs: None, // 构造器无文档注释
                is_unsafe: false,
                is_const: false,
                is_public: true,
                is_fallible: false,
            };

            self.graph.add_operation(ctor_op);
        }

        // 步骤 2: 为每个公开字段创建访问器(Ref 和 MutRef)
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
                    error_output: None,
                    generic_constraints: HashMap::new(),
                    docs: None, // 字段访问器无文档注释
                    is_unsafe: false,
                    is_const: false,
                    is_public: true,
                    is_fallible: false,
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
                    error_output: None,
                    generic_constraints: HashMap::new(),
                    docs: None, // 字段访问器无文档注释
                    is_unsafe: false,
                    is_const: false,
                    is_public: true,
                    is_fallible: false,
                };

                self.graph.add_operation(accessor_mut);
            }
        }
    }

    /// 处理 impl 块中的方法
    ///
    /// 策略:扁平化 impl 块
    /// - 不为 Trait 创建节点
    /// - impl 块中的方法直接关联到 Self 类型
    /// - 解析 Self 为具体类型
    /// - impl 块的泛型参数应该链接到类型定义的泛型参数
    fn build_impl_methods(&mut self) {
        let impl_blocks = self.graph.parsed_crate().impl_blocks.clone();

        for impl_block in impl_blocks {
            // 设置 Self 类型上下文
            let self_type_id = impl_block.for_type;

            // 从被实现的类型获取泛型参数
            // impl<E, R> DecoderReader<E, R> 中的 E 和 R 应该指向 DecoderReader::E 和 DecoderReader::R
            let generic_nodes = self.get_type_generic_nodes(self_type_id);

            log::debug!(
                "impl 块 {:?} 的泛型作用域: {} 个泛型参数",
                impl_block.id,
                generic_nodes.len()
            );
            for (param_name, node) in &generic_nodes {
                log::debug!("  - {}: {:?}", param_name, node);
            }

            self.generic_scope
                .push_scope_with_self(impl_block.id, generic_nodes, self_type_id);

            // 遍历 impl 块中的所有 item
            for &item_id in &impl_block.items {
                if let Some(item) = self.graph.parsed_crate().type_index.get(&item_id) {
                    // 处理方法
                    if let rustdoc_types::ItemEnum::Function(func) = &item.inner {
                        // 检查黑名单
                        let method_name = item.name.as_deref().unwrap_or("anonymous");
                        if self.is_blacklisted_method(method_name) {
                            log::debug!("跳过黑名单方法: {} (ID: {:?})", method_name, item_id);
                            continue;
                        }

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

        // 跳过 Trait 定义中的抽象方法
        // 注意:这是额外的安全检查,因为 trait 定义的方法不应出现在具体类型的 impl 块中
        if self.graph.parsed_crate().is_trait_method(&method_id) {
            log::debug!(
                "在 impl 块中跳过 Trait 抽象方法: {} (ID: {:?})",
                name,
                method_id
            );
            return None;
        }

        // 获取方法的文档注释
        let docs = self
            .graph
            .parsed_crate()
            .type_index
            .get(&method_id)
            .and_then(|item| item.docs.clone());

        // 解析输入参数(注意 Self 的解析)
        let inputs: Vec<DataEdge> = func
            .sig
            .inputs
            .iter()
            .filter_map(|(param_name, ty)| {
                self.extract_data_edge_from_type(ty, Some(param_name.clone()))
            })
            .collect();

        // 解析输出 - 提取 Result 的成功和错误分支
        let (output, error_output, is_fallible) = if let Some(ty) = &func.sig.output {
            let (success_ty, error_ty) = self.extract_result_branches(ty);
            let output = self.extract_data_edge_from_type(success_ty, None);
            let error_output = error_ty.and_then(|e| self.extract_data_edge_from_type(e, None));
            let is_fallible = error_ty.is_some();
            (output, error_output, is_fallible)
        } else {
            (None, None, false)
        };

        // 创建泛型约束
        let generic_constraints = self.create_generic_constraints_from_generics(&func.generics);

        let kind = if inputs.first().map(|e| e.name.as_deref()) == Some(Some("self")) {
            // 如果第一个参数是 self,则是方法调用
            OpKind::MethodCall {
                self_type: self.create_type_node_from_id(self_type_id),
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
            error_output,
            generic_constraints,
            docs,
            is_unsafe: func.header.is_unsafe,
            is_const: func.header.is_const,
            is_public: true,
            is_fallible,
        })
    }

    /// 从 Result<T, E> 中提取成功和错误分支
    ///
    /// # 返回值
    /// - `(&Type, Option<&Type>)`: (成功类型, 错误类型)
    ///
    /// # 行为
    /// - `Result<T, E>` -> (T, Some(E))
    /// - `Option<T>` -> (T, None)  // Option 没有错误类型
    /// - 其他类型 -> (原类型, None)
    ///
    /// # 示例
    /// ```ignore
    /// Result<Vec<u8>, DecodeError> -> (Vec<u8>, Some(DecodeError))
    /// Option<String> -> (String, None)
    /// Vec<u8> -> (Vec<u8>, None)
    /// ```
    fn extract_result_branches<'a>(&self, ty: &'a Type) -> (&'a Type, Option<&'a Type>) {
        match ty {
            Type::ResolvedPath(path) => {
                let name = &path.path;

                // 检测是否是 Result 类型
                let is_result = name == "Result"
                    || name.ends_with("::Result")
                    || name == "std::result::Result"
                    || name == "core::result::Result";

                // 检测是否是 Option 类型
                let is_option = name == "Option"
                    || name.ends_with("::Option")
                    || name == "std::option::Option"
                    || name == "core::option::Option";

                if is_result {
                    // Result<T, E> -> 提取 T 和 E
                    if let Some(args) = &path.args {
                        if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } =
                            args.as_ref()
                        {
                            // 完整的 Result<T, E>
                            if args.len() >= 2 {
                                if let (
                                    rustdoc_types::GenericArg::Type(success_ty),
                                    rustdoc_types::GenericArg::Type(error_ty),
                                ) = (&args[0], &args[1])
                                {
                                    log::debug!(
                                        "提取 Result<T, E> 分支: 成功={:?}, 错误={:?}",
                                        success_ty,
                                        error_ty
                                    );
                                    return (success_ty, Some(error_ty));
                                }
                            }
                            // 类型别名如 io::Result<T> = Result<T, io::Error>
                            // 只显示一个泛型参数,错误类型被别名隐藏
                            else if args.len() == 1 {
                                if let rustdoc_types::GenericArg::Type(success_ty) = &args[0] {
                                    log::debug!(
                                        "提取 Result 类型别名: 成功={:?}, 错误类型被别名隐藏",
                                        success_ty
                                    );
                                    // 返回成功类型,错误类型为 None
                                    return (success_ty, None);
                                }
                            }
                        }
                    }

                    // 没有泛型参数的 Result(极少见)
                    log::debug!("Result 类型缺少泛型参数: {:?}", path);
                } else if is_option {
                    // Option<T> -> 提取 T, 没有错误类型
                    if let Some(args) = &path.args {
                        if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } =
                            args.as_ref()
                        {
                            if let Some(rustdoc_types::GenericArg::Type(inner_ty)) = args.first() {
                                log::debug!("提取 Option<T> 值: {:?}", inner_ty);
                                return (inner_ty, None);
                            }
                        }
                    }

                    // 没有泛型参数的 Option(极少见)
                    log::debug!("Option 类型缺少泛型参数: {:?}", path);
                }

                // 非包装类型,返回原样
                (ty, None)
            }
            // 其他类型不是包装类型
            _ => (ty, None),
        }
    }

    /// (已废弃) 从 Result/Option 中提取成功类型
    ///
    /// 返回 (内部类型, 是否进行了提取)
    /// 递归处理嵌套 (e.g. Result<Option<T>, E> -> T)
    ///
    /// # 支持的包装类型
    /// - `Result<T, E>` -> 提取 T, 忽略 E
    /// - `Option<T>` -> 提取 T
    /// - `std::result::Result<T, E>` -> 提取 T
    /// - `core::result::Result<T, E>` -> 提取 T
    /// - `std::option::Option<T>` -> 提取 T
    /// - `core::option::Option<T>` -> 提取 T
    ///
    /// # 示例
    /// ```ignore
    /// Result<u32, String> -> (u32, true)
    /// Option<String> -> (String, true)
    /// Result<Option<u32>, Error> -> (u32, true)  // 递归解包
    /// Vec<String> -> (Vec<String>, false)  // 非包装类型,不变
    /// ```
    #[allow(dead_code)]
    fn extract_success_type<'a>(&self, ty: &'a Type) -> (&'a Type, bool) {
        match ty {
            Type::ResolvedPath(path) => {
                let name = &path.path;

                // 检测是否是 Result 类型
                let is_result = name == "Result"
                    || name.ends_with("::Result")
                    || name == "std::result::Result"
                    || name == "core::result::Result";

                // 检测是否是 Option 类型
                let is_option = name == "Option"
                    || name.ends_with("::Option")
                    || name == "std::option::Option"
                    || name == "core::option::Option";

                // 如果是包装类型,提取内部类型
                if is_result || is_option {
                    if let Some(args) = &path.args {
                        if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } =
                            args.as_ref()
                        {
                            // 获取第一个泛型参数(Success 类型 或 Option 的 Inner 类型)
                            if let Some(first_arg) = args.first() {
                                if let rustdoc_types::GenericArg::Type(inner_ty) = first_arg {
                                    // 递归处理嵌套 (e.g., Result<Option<T>, E> -> T)
                                    let (deep_inner, _) = self.extract_success_type(inner_ty);

                                    // 只要外层是 Result/Option,就标记为 fallible
                                    log::debug!(
                                        "解包 {} 类型: {:?} -> {:?}",
                                        if is_result { "Result" } else { "Option" },
                                        path.path,
                                        deep_inner
                                    );

                                    return (deep_inner, true);
                                }
                            }
                        }
                    }

                    // 如果无法提取泛型参数,警告并返回原类型
                    log::warn!(
                        "无法从 {} 类型中提取泛型参数: {:?}",
                        if is_result { "Result" } else { "Option" },
                        path
                    );
                }

                // 非包装类型,返回原样
                (ty, false)
            }
            // 其他类型(Primitive, Generic, etc.)不是包装类型
            _ => (ty, false),
        }
    }

    /// 从 rustdoc Type 提取 DataEdge(处理 Self 解析)
    fn extract_data_edge_from_type(&self, ty: &Type, name: Option<String>) -> Option<DataEdge> {
        let (type_node, mode) = self.extract_type_and_mode_with_self(ty, &self.generic_scope)?;

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

    /// 为枚举构建操作:为每个变体创建构造器
    ///
    /// 示例:enum Option<T> { None, Some(T) }
    /// 生成:
    /// 1. None 构造器: () -> Option<T>
    /// 2. Some 构造器: T(Move) -> Option<T>
    fn build_enum_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let enum_type_node = TypeNode::Enum(Some(type_info.id));

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
                error_output: None,
                generic_constraints: HashMap::new(),
                docs: None, // 变体构造器无文档注释
                is_unsafe: false,
                is_const: false,
                is_public: true,
                is_fallible: false,
            };

            self.graph.add_operation(variant_ctor);
        }
    }

    /// 为联合体构建操作
    ///
    /// Union 类似于 struct,但所有字段共享内存
    fn build_union_operations(&mut self, type_info: &crate::parse::TypeInfo) {
        let union_type_node = TypeNode::Union(Some(type_info.id));
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
                error_output: None,
                generic_constraints: HashMap::new(),
                docs: None,      // 联合体构造器无文档注释
                is_unsafe: true, // Union 构造通常是 unsafe
                is_const: false,
                is_public: true,
                is_fallible: false,
            };

            self.graph.add_operation(union_ctor);
        }

        // Union 字段访问器(与 struct 类似)
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
                    error_output: None,
                    generic_constraints: HashMap::new(),
                    docs: None,      // 联合体字段访问器无文档注释
                    is_unsafe: true, // Union 字段访问通常是 unsafe
                    is_const: false,
                    is_public: true,
                    is_fallible: false,
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
                    error_output: None,
                    generic_constraints: HashMap::new(),
                    docs: None, // 联合体字段访问器无文档注释
                    is_unsafe: true,
                    is_const: false,
                    is_public: true,
                    is_fallible: false,
                };

                self.graph.add_operation(accessor_mut);
            }
        }
    }

    /// 将 TypeRef 转换为 DataEdge
    ///
    /// **核心解析逻辑**:
    /// - TypeRef::Resolved(id) -> DataEdge { TypeNode::from_id(id), Move }
    /// - TypeRef::Generic(name) -> 从作用域解析
    /// - 如果是引用,需要在更高层处理(因为 TypeRef 可能不包含引用信息)
    /// - 自动解析重导出(pub use)到规范定义
    fn type_ref_to_data_edge(&self, type_ref: &TypeRef, name: Option<String>) -> Option<DataEdge> {
        match type_ref {
            TypeRef::Resolved(id) => {
                // create_type_node_from_id 内部会调用 resolve_root_id
                Some(DataEdge {
                    type_node: self.create_type_node_from_id(*id),
                    mode: EdgeMode::Move,
                    name,
                })
            }

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
                        owner_name: "anonymous".to_string(),
                        trait_bounds: trait_ids.clone(),
                    },
                    mode: EdgeMode::Move,
                    name,
                })
            }

            TypeRef::Composite(inner_types) => {
                // 复合类型:数组、切片等
                // 对于切片 [T],inner_types 只有一个元素
                if inner_types.len() == 1 {
                    // 递归解析内部类型
                    let inner_edge = self.type_ref_to_data_edge(&inner_types[0], None)?;
                    Some(DataEdge {
                        type_node: TypeNode::Array(Box::new(inner_edge.type_node)),
                        mode: EdgeMode::Move, // 注意:引用信息在 TypeRef 中已丢失
                        name,
                    })
                } else if inner_types.is_empty() {
                    // 空复合类型(不太可能出现)
                    None
                } else {
                    // 元组类型
                    let tuple_nodes: Option<Vec<TypeNode>> = inner_types
                        .iter()
                        .map(|type_ref| {
                            self.type_ref_to_data_edge(type_ref, None)
                                .map(|edge| edge.type_node)
                        })
                        .collect();

                    tuple_nodes.map(|nodes| DataEdge {
                        type_node: TypeNode::Tuple(nodes),
                        mode: EdgeMode::Move,
                        name,
                    })
                }
            }
        }
    }

    /// **关键函数**:从 rustdoc_types::Type 提取 TypeNode 和 EdgeMode(带 Self 解析)
    ///
    /// 这是设计的核心:
    /// 1. 处理 BorrowedRef 和 RawPointer
    /// 2. 解析 Self 为具体类型
    /// 3. 处理 QualifiedPath (<T as Trait>::Item)
    fn extract_type_and_mode_with_self(
        &self,
        ty: &Type,
        scope: &GenericScope,
    ) -> Option<(TypeNode, EdgeMode)> {
        match ty {
            // 处理泛型参数(包括 Self 和其他泛型参数)
            Type::Generic(name) => {
                if name == "Self" {
                    // Self 类型:从作用域解析 Self 的 ID
                    let self_id = scope.resolve_self()?;
                    Some((self.create_type_node_from_id(self_id), EdgeMode::Move))
                } else {
                    // 其他泛型参数(E, R, T 等):从作用域解析
                    let type_node = scope.resolve(name)?;
                    Some((type_node, EdgeMode::Move))
                }
            }

            // 处理引用 (&T, &mut T)
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                // 递归调用 with_self
                let (inner_type, inner_mode) =
                    self.extract_type_and_mode_with_self(type_, scope)?;

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
                let (inner_type, inner_mode) =
                    self.extract_type_and_mode_with_self(type_, scope)?;

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
                let (inner_type, _) = self.extract_type_and_mode_with_self(elem_type, scope)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 处理 Array ([T; N])
            Type::Array { type_, .. } => {
                let (inner_type, _) = self.extract_type_and_mode_with_self(type_, scope)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 处理 Tuple ((T, U))
            Type::Tuple(elements) => {
                let nodes: Option<Vec<TypeNode>> = elements
                    .iter()
                    .map(|ty| {
                        self.extract_type_and_mode_with_self(ty, scope)
                            .map(|(node, _)| node)
                    })
                    .collect();
                nodes.map(|ns| (TypeNode::Tuple(ns), EdgeMode::Move))
            }

            // QualifiedPath: <T as Trait>::Item 或 Self::Item
            // 关联类型应该被解析到实际定义
            Type::QualifiedPath {
                self_type,
                trait_,
                name,
                args,
                ..
            } => {
                // 优先尝试从 args 中解析出实际类型 ID
                if let Some(args_box) = args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } =
                        args_box.as_ref()
                    {
                        // 如果有泛型参数,尝试使用第一个(通常是实际类型)
                        if let Some(rustdoc_types::GenericArg::Type(actual_ty)) = args.first() {
                            log::debug!(
                                "QualifiedPath 通过 args 解析: {} -> {:?}",
                                name,
                                actual_ty
                            );
                            return self.extract_type_and_mode_with_self(actual_ty, scope);
                        }
                    }
                }

                // 尝试在类型索引中查找关联类型的实际定义
                // 通过 trait + 名称查找
                if let Some(trait_path) = trait_ {
                    // 尝试查找 Trait 的关联类型定义
                    if let Some(trait_item) =
                        self.graph.parsed_crate().type_index.get(&trait_path.id)
                    {
                        if let rustdoc_types::ItemEnum::Trait(trait_def) = &trait_item.inner {
                            // 在 trait 的 items 中查找同名的关联类型
                            for &item_id in &trait_def.items {
                                if let Some(assoc_item) =
                                    self.graph.parsed_crate().type_index.get(&item_id)
                                {
                                    if assoc_item.name.as_deref() == Some(name) {
                                        log::debug!(
                                            "QualifiedPath 通过 Trait 定义解析: {} -> Id({:?})",
                                            name,
                                            item_id
                                        );
                                        // 找到关联类型定义,使用其 ID
                                        return Some((
                                            self.create_type_node_from_id(item_id),
                                            EdgeMode::Move,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                // 如果无法解析,尝试解析 self_type
                log::debug!("QualifiedPath 无法完全解析,使用路径表示: {}", name);
                let (inner_type, _) = self.extract_type_and_mode_with_self(self_type, scope)?;
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

            // 其他类型:不涉及递归的,可以安全委托给 extract_type_and_mode
            // (Primitive, ResolvedPath, Generic(非Self), ImplTrait, etc.)
            _ => self.extract_type_and_mode(ty),
        }
    }

    /// **原有函数**:从 rustdoc_types::Type 提取 TypeNode 和 EdgeMode
    ///
    /// 这是设计的核心:如何处理 BorrowedRef 和 RawPointer
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
    ///
    ///  ```
    fn extract_type_and_mode(&self, ty: &Type) -> Option<(TypeNode, EdgeMode)> {
        match ty {
            // 原始类型:按值传递
            Type::Primitive(name) => Some((TypeNode::Primitive(name.clone()), EdgeMode::Move)),

            // 已解析的路径:Struct/Enum 等
            Type::ResolvedPath(path) => {
                let name = &path.path;

                // 检测是否是包装类型 (Result/Option)
                let is_result = name == "Result"
                    || name.ends_with("::Result")
                    || name == "std::result::Result"
                    || name == "core::result::Result";

                let is_option = name == "Option"
                    || name.ends_with("::Option")
                    || name == "std::option::Option"
                    || name == "core::option::Option";

                if is_result || is_option {
                    // 包装类型:提取第一个泛型参数(成功类型)
                    // 注意:这里忽略了错误类型,因为这个函数只能返回单个类型
                    // 错误类型应该在 extract_result_branches 中处理
                    if let Some(args) = &path.args {
                        if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } =
                            args.as_ref()
                        {
                            if let Some(rustdoc_types::GenericArg::Type(inner_ty)) = args.first() {
                                log::debug!(
                                    "穿透包装类型 {}: {:?} -> {:?}",
                                    if is_result { "Result" } else { "Option" },
                                    path.path,
                                    inner_ty
                                );
                                // 递归处理内部类型
                                return self.extract_type_and_mode(inner_ty);
                            }
                        }
                    }

                    log::warn!("无法从包装类型 {} 中提取内部类型,标记为 Unknown", path.path);
                    return Some((TypeNode::Unknown, EdgeMode::Move));
                }

                // 非包装类型:检查是否有泛型参数
                // 只有当泛型参数是具体类型(不是泛型参数占位符)时才创建 GenericInstance
                // 例如:Vec<u8> 是 GenericInstance,但 EncoderWriter<E, W> 不是
                if let Some(args) = &path.args {
                    if let rustdoc_types::GenericArgs::AngleBracketed { args, .. } = args.as_ref() {
                        // 提取类型参数
                        let type_args: Vec<TypeNode> = args
                            .iter()
                            .filter_map(|arg| {
                                if let rustdoc_types::GenericArg::Type(ty) = arg {
                                    self.extract_type_and_mode(ty).map(|(node, _)| node)
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if !type_args.is_empty() {
                            // 检查是否所有类型参数都是具体类型(不是 GenericParam)
                            let has_concrete_types = type_args
                                .iter()
                                .any(|node| !matches!(node, TypeNode::GenericParam { .. }));

                            if has_concrete_types {
                                // 至少有一个具体类型,创建泛型实例化节点
                                log::debug!(
                                    "创建泛型实例化: {} with {} type args (有具体类型)",
                                    path.path,
                                    type_args.len()
                                );
                                return Some((
                                    TypeNode::GenericInstance {
                                        base_id: path.id,
                                        path: path.path.clone(),
                                        type_args,
                                    },
                                    EdgeMode::Move,
                                ));
                            } else {
                                // 全是泛型参数占位符,使用类型定义本身
                                log::debug!("跳过泛型实例化: {} (全是泛型参数占位符)", path.path);
                            }
                        }
                    }
                }

                // 没有泛型参数,使用普通类型节点
                Some((self.create_type_node_from_id(path.id), EdgeMode::Move))
            }

            // 1. &T 或 &mut T -> 提取内部类型 T,EdgeMode 记录引用信息
            Type::BorrowedRef {
                is_mutable, type_, ..
            } => {
                // 递归提取内部类型
                let (inner_type, inner_mode) = self.extract_type_and_mode(type_)?;

                // 如果内部已经是引用,保持原样(避免 &&T)
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
                let (inner_type, inner_mode) = self.extract_type_and_mode(type_)?;

                // 如果内部已经是指针,保持原样
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
            // 需要从作用域解析,而不是创建新节点
            // 注意:这个函数不接受作用域参数,所以委托给 extract_type_and_mode_with_self
            Type::Generic(_name) => {
                // 这里不应该直接处理,应该在 extract_type_and_mode_with_self 中处理
                // 临时创建一个匿名节点,但这不应该被使用
                log::warn!(
                    "extract_type_and_mode 遇到泛型参数,应该使用 extract_type_and_mode_with_self: {}",
                    _name
                );
                Some((
                    TypeNode::GenericParam {
                        name: _name.clone(),
                        owner_id: Id(0),
                        owner_name: "anonymous".to_string(),
                        trait_bounds: Vec::new(),
                    },
                    EdgeMode::Move,
                ))
            }

            // 4. Slice: [T] -> 提取元素类型
            Type::Slice(elem_type) => {
                let (inner_type, _) = self.extract_type_and_mode(elem_type)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 5. Array: [T; N] -> 提取元素类型
            Type::Array { type_, .. } => {
                let (inner_type, _) = self.extract_type_and_mode(type_)?;
                Some((TypeNode::Array(Box::new(inner_type)), EdgeMode::Move))
            }

            // 6. 元组
            Type::Tuple(elements) => {
                let nodes: Option<Vec<TypeNode>> = elements
                    .iter()
                    .map(|ty| self.extract_type_and_mode(ty).map(|(node, _)| node))
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
                        owner_id: Id(0), // 匿名 impl Trait(使用 Id(0) 作为特殊标识)
                        owner_name: "anonymous".to_string(),
                        trait_bounds,
                    },
                    EdgeMode::Move,
                ))
            }

            // 8. 函数指针
            Type::FunctionPointer(_) => {
                // TODO: 完整处理函数指针
                warn!("遇到 FunctionPointer,暂时标记为 Unknown");
                Some((TypeNode::Unknown, EdgeMode::Move))
            }

            // 9. 其他类型
            _ => {
                warn!("遇到未知类型,标记为 Unknown: {:?}", ty);
                Some((TypeNode::Unknown, EdgeMode::Move))
            }
        }
    }
}

/// 从 ParsedCrate 构建 IR Graph
pub fn build_ir_graph(parsed_crate: ParsedCrate) -> IrGraph {
    IrGraphBuilder::new(parsed_crate).build()
}
