//! 库所(Place)创建相关的函数
//!
//! 这个模块包含 PetriNetBuilder 中与 Place 创建相关的方法实现

use log::{debug, info, warn};
use rustdoc_types::{GenericParamDefKind, Id, Impl, Item, ItemEnum, Path, StructKind, Type, VariantKind};

use crate::petri::structure::{BorrowKind, Flow, Transition, TransitionKind};
use crate::petri::utils::{format_type, is_std_library_type, type_has_generic};

use super::builder::PetriNetBuilder;
use super::net::PlaceId;
use super::structure::{Place, PlaceKind};

impl<'a> PetriNetBuilder<'a> {

    /// 构建类型成员之间的 holds 关系
    /// 1. 处理 Struct 的字段
    /// 2. 处理 Enum 的变体
    /// 3. 处理 Union 的字段
    /// 4. 处理 Variant 的字段(Tuple 和 Struct)
    pub(super) fn build_type_relationships(&mut self) {
        // 收集需要处理的类型关系
        let mut relationships = Vec::new();

        for (type_id, _place_id) in &self.type_place_map.clone() {
            if let Some(item) = self.crate_.index.get(type_id) {
                match &item.inner {
                    ItemEnum::Struct(struct_def) => {
                        match &struct_def.kind {
                            StructKind::Plain { fields, .. } => {
                                for field_id in fields {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            StructKind::Tuple(field_ids) => {
                                for field_id in field_ids.iter().flatten() {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            StructKind::Unit => {
                                // Unit struct 没有字段
                            }
                        }
                    }
                    ItemEnum::Enum(enum_def) => {
                        // Enum 到 Variant 的关系特殊处理
                        // Variant 已经在 Step 1 创建了 Place,直接建立 holds 关系
                        for variant_id in &enum_def.variants {
                            if let Some(&variant_place_id) = self.type_place_map.get(variant_id) {
                                if let Some(&enum_place_id) = self.type_place_map.get(type_id) {
                                    self.create_holds_transition(
                                        *type_id,
                                        *variant_id,
                                        enum_place_id,
                                        variant_place_id,
                                    );
                                }
                            }
                        }
                    }
                    ItemEnum::Union(union_def) => {
                        for field_id in &union_def.fields {
                            if let Some(field_item) = self.crate_.index.get(field_id) {
                                if let ItemEnum::StructField(field_type) = &field_item.inner {
                                    relationships.push((*type_id, *field_id, field_type.clone()));
                                }
                            }
                        }
                    }
                    ItemEnum::Variant(variant) => {
                        match &variant.kind {
                            VariantKind::Plain => {
                                // Plain variant 不需要特殊处理
                            }
                            VariantKind::Tuple(field_ids) => {
                                for field_id in field_ids.iter().flatten() {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                            VariantKind::Struct { fields, .. } => {
                                for field_id in fields {
                                    if let Some(field_item) = self.crate_.index.get(field_id) {
                                        if let ItemEnum::StructField(field_type) = &field_item.inner
                                        {
                                            relationships.push((
                                                *type_id,
                                                *field_id,
                                                field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // 处理所有关系
        for (owner_id, member_id, field_type) in relationships {
            // 为字段类型创建 Place(如果需要)
            // 传入 owner_id 作为上下文,用于解析泛型参数
            let member_place_id = self.create_or_get_type_place(&field_type, &member_id, owner_id);

            // 创建 holds transition 连接 owner 和 member
            if let (Some(owner_place_id), Some(member_place_id)) =
                (self.type_place_map.get(&owner_id), member_place_id)
            {
                self.create_holds_transition(owner_id, member_id, *owner_place_id, member_place_id);
            }
        }
    }

    /// 为类型创建或获取 Place,避免重复创建
    /// 返回 PlaceId,如果类型无需创建 Place 则返回 None
    ///
    /// # 参数
    /// - `ty`: 要创建 Place 的类型
    /// - `field_id`: 字段的 ID(用于生成唯一 ID)
    /// - `owner_type_id`: 拥有此字段的类型的 ID(用于解析泛型参数)
    pub(super) fn create_or_get_type_place(
        &mut self,
        ty: &Type,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        match ty {
            Type::Primitive(name) => {
                let type_key = format!("primitive:{}", name);
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                let place = Place::new(
                    *field_id,
                    name.clone(),
                    format!("primitive::{}", name),
                    PlaceKind::Primitive(name.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);
                Some(place_id)
            }
            Type::Tuple(types) => {
                let type_key = format!("tuple:{}", format_type(ty));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 递归为 tuple 中的每个类型创建 Place
                let mut inner_info = Vec::new();
                for inner_type in types.iter() {
                    let dummy_id = self.generate_temp_id();
                    if let Some(inner_place_id) =
                        self.create_or_get_type_place(inner_type, &dummy_id, owner_type_id)
                    {
                        inner_info.push((dummy_id, inner_place_id));
                    }
                }

                let place = Place::new(
                    *field_id,
                    format!("Tuple{}", types.len()),
                    format!("tuple::({})", format_type(ty)),
                    PlaceKind::Tuple(types.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key.clone(), place_id);

                // 创建 tuple 到其元素的 holds 关系
                for (dummy_id, inner_place_id) in inner_info {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Slice(inner_type) => {
                let type_key = format!("slice:{}", format_type(ty));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为 slice 的元素类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id =
                    self.create_or_get_type_place(inner_type, &dummy_id, owner_type_id);

                let place = Place::new(
                    *field_id,
                    format!("[{}]", format_type(inner_type)),
                    format!("slice::[{}]", format_type(inner_type)),
                    PlaceKind::Slice(inner_type.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 slice 到其元素的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Array { type_, len } => {
                let type_key = format!("array:{}:{}", format_type(type_), len);
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为 array 的元素类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

                let place = Place::new(
                    *field_id,
                    format!("[{}; {}]", format_type(type_), len),
                    format!("array::[{}; {}]", format_type(type_), len),
                    PlaceKind::Array(type_.clone(), len.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 array 到其元素的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::Infer => {
                let type_key = "infer:_".to_string();
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                let place = Place::new(
                    *field_id,
                    "_".to_string(),
                    "infer::_".to_string(),
                    PlaceKind::Infer,
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);
                Some(place_id)
            }
            Type::RawPointer { is_mutable, type_ } => {
                let mutability = if *is_mutable { "mut" } else { "const" };
                let type_key = format!("rawptr:{}:{}", mutability, format_type(type_));
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为指针的目标类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

                let place = Place::new(
                    *field_id,
                    format!("*{} {}", mutability, format_type(type_)),
                    format!("rawptr::*{} {}", mutability, format_type(type_)),
                    PlaceKind::RawPointer(type_.clone(), *is_mutable),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 rawptr 到其目标的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::BorrowedRef {
                is_mutable,
                type_,
                lifetime,
            } => {
                let mutability = if *is_mutable { " mut" } else { "" };
                let lifetime_str = lifetime.as_deref().unwrap_or("");
                let type_key = format!(
                    "borrowedref:{}{}:{}",
                    lifetime_str,
                    mutability,
                    format_type(type_)
                );
                if let Some(&place_id) = self.type_cache.get(&type_key) {
                    return Some(place_id);
                }

                // 为引用的目标类型创建 Place
                let dummy_id = self.generate_temp_id();
                let inner_place_id = self.create_or_get_type_place(type_, &dummy_id, owner_type_id);

                let place = Place::new(
                    *field_id,
                    format!("&{}{} {}", lifetime_str, mutability, format_type(type_)),
                    format!(
                        "borrowedref::&{}{} {}",
                        lifetime_str,
                        mutability,
                        format_type(type_)
                    ),
                    PlaceKind::BorrowedRef(type_.clone(), *is_mutable, lifetime.clone()),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_cache.insert(type_key, place_id);

                // 创建 borrowedref 到其目标的 holds 关系
                if let Some(inner_place_id) = inner_place_id {
                    self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
                }

                Some(place_id)
            }
            Type::ResolvedPath(path) => {
                // 检查是否是 Result<T, E> 或 Option<T>
                // 支持: "Result", "io::Result", "core::result::Result" 等
                if (path.path == "Result" || path.path.ends_with("::Result")) && path.args.is_some()
                {
                    return self.handle_result_type(path, field_id, owner_type_id);
                } else if (path.path == "Option" || path.path.ends_with("::Option"))
                    && path.args.is_some()
                {
                    return self.handle_option_type(path, field_id, owner_type_id);
                }

                // 如果是已知的类型,直接返回其 PlaceId
                if let Some(&place_id) = self.type_place_map.get(&path.id) {
                    return Some(place_id);
                }

                // 检查是否是标准库类型,如果是则自动创建
                if is_std_library_type(&path.path) {
                    return self.handle_std_library_type(path, field_id, owner_type_id);
                }

                None
            }
            Type::Generic(generic_name) => {
                // 查找对应的泛型参数占位符
                // 根据约束查找对应的库所
                let constraint_trait_ids =
                    self.find_generic_constraint_trait_ids(owner_type_id, generic_name);
                let cache_key = (generic_name.clone(), constraint_trait_ids);
                self.generic_param_cache.get(&cache_key).copied()
            }
            Type::QualifiedPath {
                name,
                self_type,
                trait_,
                ..
            } => {
                info!("处理 QualifiedPath: {:?}", ty);
                // 按照 Scheme B 规则处理 QualifiedPath:转换为 Projection-Type Place
                self.handle_qualified_path(name, self_type, trait_, field_id, owner_type_id)
            }
            _ => {
                // 其他类型暂不处理
                warn!("未处理的类型: {:?}", ty);
                None
            }
        }
    }

  /// 处理 Result<T, E> 类型
    /// 为 Result 创建 Place,提取 T 和 E,并创建 unwrap 变迁连接到 T 和 E
    fn handle_result_type(
        &mut self,
        path: &Path,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        use rustdoc_types::GenericArgs;

        // 提取泛型参数 T 和 E
        let args = path.args.as_ref()?;
        let generic_args = match args.as_ref() {
            GenericArgs::AngleBracketed { args, .. } => args,
            _ => return None,
        };

        // 处理 Result 类型的泛型参数
        // 标准 Result<T, E> 有 2 个参数,但类型别名可能只有 1 个(错误类型是固定的)
        let (ok_type, err_type) = if generic_args.len() >= 2 {
            // 标准 Result<T, E>
            let ok = match &generic_args[0] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            let err = match &generic_args[1] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            (ok, err)
        } else if generic_args.len() == 1 {
            // 类型别名 Result<T>(错误类型固定)
            // 例如 io::Result<T> = Result<T, io::Error>
            let ok = match &generic_args[0] {
                rustdoc_types::GenericArg::Type(ty) => ty,
                _ => return None,
            };
            // 为错误类型创建一个占位符(使用 Infer 类型)
            let err = &Type::Infer;
            (ok, err)
        } else {
            warn!(
                "Result 类型应该有 1-2 个泛型参数,但找到 {} 个",
                generic_args.len()
            );
            return None;
        };

        // 生成唯一的类型键
        let type_key = format!(
            "result:{}<{}, {}>",
            format_type(&Type::ResolvedPath(path.clone())),
            format_type(ok_type),
            format_type(err_type)
        );

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 为 T 和 E 创建 Place
        let ok_id = self.generate_temp_id();
        let err_id = self.generate_temp_id();
        let ok_place_id = self.create_or_get_type_place(ok_type, &ok_id, owner_type_id);
        let err_place_id = self.create_or_get_type_place(err_type, &err_id, owner_type_id);

        // 创建 Result<T, E> 的 Place
        let result_place = Place::new(
            *field_id,
            format!(
                "Result<{}, {}>",
                format_type(ok_type),
                format_type(err_type)
            ),
            format!(
                "result::Result<{}, {}>",
                format_type(ok_type),
                format_type(err_type)
            ),
            PlaceKind::Result(Box::new(ok_type.clone()), Box::new(err_type.clone())),
        );
        let result_place_id = self.net.add_place_and_get_id(result_place);
        self.type_cache.insert(type_key, result_place_id);

        // 创建 unwrap 变迁: Result<T, E> -> T 和 E
        let unwrap_transition_id = self.generate_temp_id();
        let unwrap_transition = Transition::new(
            unwrap_transition_id,
            format!("unwrap_result"),
            TransitionKind::Unwrap,
        );
        let unwrap_transition_id = self.net.add_transition_and_get_id(unwrap_transition);

        // Result -> unwrap
        self.net.add_flow(
            result_place_id,
            unwrap_transition_id,
            Flow {
                weight: 1,
                param_type: "result".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        // unwrap -> T (Ok)
        if let Some(ok_place_id) = ok_place_id {
            self.net.add_flow_from_transition(
                unwrap_transition_id,
                ok_place_id,
                Flow {
                    weight: 1,
                    param_type: "ok".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        } else {
            warn!(
                "Result 类型的 Ok 类型 '{}' 无法创建 Place,unwrap 变迁无法连接到 Ok 类型",
                format_type(ok_type)
            );
        }

        // unwrap -> E (Err)
        if let Some(err_place_id) = err_place_id {
            self.net.add_flow_from_transition(
                unwrap_transition_id,
                err_place_id,
                Flow {
                    weight: 1,
                    param_type: "err".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        } else {
            warn!(
                "Result 类型的 Err 类型 '{}' 无法创建 Place,unwrap 变迁无法连接到 Err 类型",
                format_type(err_type)
            );
        }

        debug!(
            "✨ 创建 Result<{}, {}> 及其 unwrap 变迁",
            format_type(ok_type),
            format_type(err_type)
        );

        Some(result_place_id)
    }

    /// 处理标准库类型
    /// 为标准库类型(String, Vec, bool 等)自动创建 Place
    pub(super) fn handle_std_library_type(
        &mut self,
        path: &Path,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        // 生成类型键(使用完整路径以区分 String 和 alloc::string::String)
        let type_key = format!("std_lib:{}", path.path);

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 处理泛型参数(如果有)
        let mut generic_info = Vec::new();
        if let Some(args) = &path.args {
            use rustdoc_types::GenericArgs;
            if let GenericArgs::AngleBracketed { args, .. } = args.as_ref() {
                for arg in args {
                    if let rustdoc_types::GenericArg::Type(ty) = arg {
                        let dummy_id = self.generate_temp_id();
                        if let Some(inner_place_id) =
                            self.create_or_get_type_place(ty, &dummy_id, owner_type_id)
                        {
                            generic_info.push((dummy_id, inner_place_id));
                        }
                    }
                }
            }
        }

        // 创建标准库类型的 Place(使用 Primitive 或 Struct)
        // 对于简单类型用 Primitive,对于复杂类型可以扩展
        let display_name = if generic_info.is_empty() {
            format!("{}", path.path)
        } else {
            format!("{}<...>", path.path)
        };

        let place = Place::new(
            *field_id,
            display_name.clone(),
            format!("std_lib::{}", path.path),
            PlaceKind::Primitive(path.path.clone()), // 使用 Primitive 存储标准库类型
        );
        let place_id = self.net.add_place_and_get_id(place);
        self.type_cache.insert(type_key.clone(), place_id);

        // 同时存入 type_place_map,使用 rustdoc 的 ID
        // 这样 impl 块和其他地方可以通过 rustdoc ID 找到这个 Place
        self.type_place_map.insert(path.id, place_id);

        // 如果有泛型参数,创建 holds 关系
        for (dummy_id, inner_place_id) in generic_info {
            self.create_holds_transition(*field_id, dummy_id, place_id, inner_place_id);
        }

        debug!("✨ 自动创建标准库类型 '{}' Place", path.path);

        Some(place_id)
    }


     /// 处理 Option<T> 类型
    /// 为 Option 创建 Place,提取 T,并创建 ok 变迁连接到 T
    pub(super) fn handle_option_type(
        &mut self,
        path: &Path,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        use rustdoc_types::GenericArgs;

        // 提取泛型参数 T
        let args = path.args.as_ref()?;
        let generic_args = match args.as_ref() {
            GenericArgs::AngleBracketed { args, .. } => args,
            _ => return None,
        };

        if generic_args.is_empty() {
            warn!("Option 类型应该有 1 个泛型参数,但没有找到");
            return None;
        }

        // 获取 T 的类型
        let some_type = match &generic_args[0] {
            rustdoc_types::GenericArg::Type(ty) => ty,
            _ => return None,
        };

        // 生成唯一的类型键
        let type_key = format!(
            "option:{}<{}>",
            format_type(&Type::ResolvedPath(path.clone())),
            format_type(some_type)
        );

        // 检查是否已创建
        if let Some(&place_id) = self.type_cache.get(&type_key) {
            return Some(place_id);
        }

        // 为 T 创建 Place
        let some_id = self.generate_temp_id();
        let some_place_id = self.create_or_get_type_place(some_type, &some_id, owner_type_id);

        // 创建 Option<T> 的 Place
        let option_place = Place::new(
            *field_id,
            format!("Option<{}>", format_type(some_type)),
            format!("option::Option<{}>", format_type(some_type)),
            PlaceKind::Option(Box::new(some_type.clone())),
        );
        let option_place_id = self.net.add_place_and_get_id(option_place);
        self.type_cache.insert(type_key, option_place_id);

        // 创建 ok/unwrap 变迁: Option<T> -> T
        let ok_transition_id = self.generate_temp_id();
        let ok_transition = Transition::new(
            ok_transition_id,
            format!("unwrap_option"),
            TransitionKind::Ok,
        );
        let ok_transition_id = self.net.add_transition_and_get_id(ok_transition);

        // Option -> ok
        self.net.add_flow(
            option_place_id,
            ok_transition_id,
            Flow {
                weight: 1,
                param_type: "option".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        // ok -> T (Some)
        if let Some(some_place_id) = some_place_id {
            self.net.add_flow_from_transition(
                ok_transition_id,
                some_place_id,
                Flow {
                    weight: 1,
                    param_type: "some".to_string(),
                    borrow_kind: BorrowKind::Owned,
                },
            );
        }

        debug!("✨ 创建 Option<{}> 及其 ok 变迁", format_type(some_type));

        Some(option_place_id)
    }
   /// 处理 QualifiedPath,将其转换为 Projection-Type Place
    ///
    /// 根据规则文档,每个 QualifiedPath 必须转换为一个 Projection-Type Place,
    /// 使用规范化的身份标识:Projection(self=<SelfTypeCanonicalID>, trait=<TraitCanonicalID>, assoc=<AssocName>)
    fn handle_qualified_path(
        &mut self,
        assoc_name: &str,
        self_type: &Type,
        trait_: &Option<Path>,
        field_id: &Id,
        owner_type_id: Id,
    ) -> Option<PlaceId> {
        // 1. 解析 self_type 到 canonical ID
        let self_type_id = match self_type {
            Type::ResolvedPath(path) => {
                // 如果是 ResolvedPath,使用 path.id 作为 canonical ID
                path.id
            }
            Type::Generic(gen_name) => {
                // 如果是 Generic("Self"),需要从上下文解析
                if gen_name == "Self" {
                    // 在 impl 块中,Self 就是 receiver_id
                    // 但这里我们可能没有 receiver_id,需要从 owner_type_id 推断
                    // 如果 owner_type_id 是一个类型,就使用它
                    owner_type_id
                } else {
                    // 其他泛型参数,查找对应的泛型参数 Place
                    let constraint_trait_ids =
                        self.find_generic_constraint_trait_ids(owner_type_id, gen_name);
                    let cache_key = (gen_name.clone(), constraint_trait_ids);
                    if self.generic_param_cache.contains_key(&cache_key) {
                        // 对于泛型参数,我们需要一个唯一的 ID 来表示它
                        // 我们可以使用泛型参数 Place 的 ID,但这不够准确
                        // 更好的方法是使用一个规范化的 ID 生成策略
                        // 这里我们使用 owner_type_id 和泛型名的组合作为代理 ID
                        // 但为了 Projection 的规范化,我们需要一个稳定的标识符
                        // 暂时使用 owner_type_id 作为 self_type_id,但这不是最优解
                        // TODO: 改进泛型参数的规范化 ID 生成策略
                        owner_type_id
                    } else {
                        // 泛型参数未找到,尝试在函数级别查找
                        // 这可能发生在函数泛型参数中
                        // 我们需要一个更好的上下文传递机制
                        warn!(
                            "QualifiedPath 中的 self_type 是未找到的泛型参数 {} (owner_type_id: {:?}),尝试使用 owner_type_id 作为代理",
                            gen_name, owner_type_id
                        );
                        // 使用 owner_type_id 作为代理,虽然不够准确,但至少可以创建 Projection
                        owner_type_id
                    }
                }
            }
            _ => {
                warn!("QualifiedPath 中的 self_type 类型不支持: {:?}", self_type);
                return None;
            }
        };

        // 2. 解析 trait 到 canonical ID
        let trait_id = match trait_ {
            Some(trait_path) => trait_path.id,
            None => {
                warn!("QualifiedPath 缺少 trait 信息");
                return None;
            }
        };

        // 3. 构建规范化的 key: (self_type_id, trait_id, assoc_name)
        let projection_key = (self_type_id, trait_id, assoc_name.to_string());

        // 4. 检查是否已存在,如果存在则重用
        if let Some(&projection_place_id) = self.projection_cache.get(&projection_key) {
            return Some(projection_place_id);
        }

        // 5. 获取 self_type 的 Place(如果不存在,尝试创建)
        let self_place_id = if let Some(&place_id) = self.type_place_map.get(&self_type_id) {
            place_id
        } else {
            // 如果 self_type 是泛型参数,尝试查找对应的泛型参数 Place
            if let Type::Generic(gen_name) = self_type {
                let constraint_trait_ids =
                    self.find_generic_constraint_trait_ids(owner_type_id, gen_name);
                let cache_key = (gen_name.clone(), constraint_trait_ids);
                if let Some(&generic_place_id) = self.generic_param_cache.get(&cache_key) {
                    // 使用泛型参数 Place 作为 self_place_id
                    generic_place_id
                } else {
                    // 泛型参数 Place 不存在,尝试创建
                    if let Some(place_id) =
                        self.create_or_get_type_place(self_type, field_id, owner_type_id)
                    {
                        place_id
                    } else {
                        // 如果还是无法创建,尝试使用 owner_type_id 的类型 Place 作为后备
                        if let Some(&owner_place_id) = self.type_place_map.get(&owner_type_id) {
                            warn!(
                                "无法为 QualifiedPath 的 self_type (泛型参数 '{}') 创建 Place,使用 owner_type_id {:?} 的类型 Place 作为后备",
                                gen_name, owner_type_id
                            );
                            owner_place_id
                        } else {
                            warn!(
                                "无法为 QualifiedPath 的 self_type 创建 Place: {:?}",
                                self_type
                            );
                            return None;
                        }
                    }
                }
            } else {
                // 尝试解析并创建 self_type 的 Place
                if let Some(place_id) =
                    self.create_or_get_type_place(self_type, field_id, owner_type_id)
                {
                    place_id
                } else {
                    warn!(
                        "无法为 QualifiedPath 的 self_type 创建 Place: {:?}",
                        self_type
                    );
                    return None;
                }
            }
        };

        // 6. 创建 Projection Place
        let projection_id = self.generate_temp_id();
        let projection_name = format!(
            "<{} as {}>::{}",
            format_type(self_type),
            self.get_type_name(&trait_id)
                .unwrap_or_else(|| format!("{:?}", trait_id)),
            assoc_name
        );
        let projection_path = format!(
            "projection::self={:?}::trait={:?}::assoc={}",
            self_type_id, trait_id, assoc_name
        );

        let projection_place = Place::new(
            projection_id,
            projection_name.clone(),
            projection_path,
            PlaceKind::Projection(self_type_id, trait_id, assoc_name.to_string()),
        );
        let projection_place_id = self.net.add_place_and_get_id(projection_place);

        // 7. 缓存 Projection Place
        self.projection_cache
            .insert(projection_key, projection_place_id);

        // 8. 创建 Projection transition: Place(SelfType) --Projection--> Place(Projection)
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            "projection".to_string(),
            TransitionKind::Projection(self_type_id, trait_id, assoc_name.to_string()),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:self_type -> transition -> projection
        self.net.add_flow(
            self_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "self_type".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            projection_place_id,
            Flow {
                weight: 1,
                param_type: "projection".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        debug!(
            "✨ 创建 Projection: <{:?} as {:?}>::{}",
            self_type_id, trait_id, assoc_name
        );

        Some(projection_place_id)
    }

    /// 为 Struct/Enum/Union/Variant 类型创建 Place
    pub(super) fn create_type_place(&mut self, item: &Item) {
        let place_kind = match &item.inner {
            ItemEnum::Struct(s) => PlaceKind::Struct(s.clone()),
            ItemEnum::Enum(e) => PlaceKind::Enum(e.clone()),
            ItemEnum::Union(u) => PlaceKind::Union(u.clone()),
            ItemEnum::Trait(t) => PlaceKind::Trait(t.clone()),
            ItemEnum::Variant(v) => PlaceKind::Variant(v.clone()),
            _ => return,
        };

        let name = item
            .name
            .clone()
            .unwrap_or_else(|| format!("anonymous_{:?}", item.id));
        let path = item.name.clone().unwrap_or_default();

        let place = Place::new(item.id, name, path, place_kind);
        let place_id = self.net.add_place_and_get_id(place);

        self.type_place_map.insert(item.id, place_id);

        self.create_generic_param_places(item.id, &item.inner, place_id);

        // 如果是 Trait,还要处理 items（关联类型和函数签名）
        if let ItemEnum::Trait(_) = &item.inner {
            self.create_trait_items_places(item.id, &item.inner, place_id);
        }
    }

     /// 查找泛型参数的约束 trait IDs
     pub(super) fn find_generic_constraint_trait_ids(&self, owner_id: Id, generic_name: &str) -> Vec<Id> {
        // 查找 owner 的 generics 定义
        if let Some(item) = self.crate_.index.get(&owner_id) {
            let generics = match &item.inner {
                ItemEnum::Struct(s) => &s.generics,
                ItemEnum::Enum(e) => &e.generics,
                ItemEnum::Union(u) => &u.generics,
                ItemEnum::Trait(t) => &t.generics,
                ItemEnum::Function(f) => &f.generics,
                ItemEnum::Impl(i) => &i.generics,
                // Variant 没有自己的 generics,它继承自 Enum
                _ => return Vec::new(),
            };

            // 查找匹配的泛型参数
            for param in &generics.params {
                if param.name == generic_name {
                    if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                        let mut trait_ids = Vec::new();
                        for bound in bounds {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                trait_ids.push(trait_.id);
                            }
                        }
                        trait_ids.sort();
                        return trait_ids;
                    }
                }
            }
        }

        Vec::new()
    }
    /// 为有 impl 的类型创建 Place
    ///
    /// 这个方法用于自动发现并创建那些有 impl 块但还没有创建 Place 的类型
    /// 主要用于处理标准库类型(如 String、Vec 等)和类型别名
    ///
    /// 返回是否成功创建了 Place
    pub(super) fn create_type_place_for_impl(&mut self, item: &Item, impl_block: &Impl) -> bool {
        // 如果已经存在,不重复创建
        if self.type_place_map.contains_key(&item.id) {
            return false;
        }

        let place_kind = match &item.inner {
            ItemEnum::Struct(s) => PlaceKind::Struct(s.clone()),
            ItemEnum::Enum(e) => PlaceKind::Enum(e.clone()),
            ItemEnum::Union(u) => PlaceKind::Union(u.clone()),
            ItemEnum::Variant(v) => PlaceKind::Variant(v.clone()),
            // 对于 Primitive 和 TypeAlias,我们也可以创建 Place
            ItemEnum::Primitive(_) => {
                let name = item
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("primitive_{:?}", item.id));
                let place = Place::new(
                    item.id,
                    name.clone(),
                    format!("primitive::{}", name),
                    PlaceKind::Primitive(name),
                );
                let place_id = self.net.add_place_and_get_id(place);
                self.type_place_map.insert(item.id, place_id);
                return true;
            }
            // 其他类型暂不支持自动创建
            _ => {
                return false;
            }
        };

        let name = item
            .name
            .clone()
            .unwrap_or_else(|| format!("anonymous_{:?}", item.id));
        let path = item.name.clone().unwrap_or_default();

        let place = Place::new(item.id, name, path, place_kind);
        let place_id = self.net.add_place_and_get_id(place);

        self.type_place_map.insert(item.id, place_id);

        // 为类型的泛型参数创建占位符 Place
        // 使用 impl 块的泛型信息
        self.create_generic_param_places_from_impl(item.id, impl_block, place_id);

        true
    }

    /// 从 impl 块的泛型信息创建泛型参数占位符
    pub(super) fn create_generic_param_places_from_impl(
        &mut self,
        _type_id: Id,
        impl_block: &Impl,
        _type_place_id: PlaceId,
    ) {
        // 为 impl 块的泛型参数创建 Place
        // 使用与 create_generic_param_places 相同的逻辑，基于约束创建
        for param_def in &impl_block.generics.params {
            if let GenericParamDefKind::Type { bounds, .. } = &param_def.kind {
                let generic_name = param_def.name.clone();

                // 提取约束的 trait IDs
                let mut constraint_trait_ids = Vec::new();
                for bound in bounds {
                    if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                        constraint_trait_ids.push(trait_.id);
                    }
                }
                constraint_trait_ids.sort();

                // 使用 (generic_name, constraint_trait_ids) 作为 key
                let cache_key = (generic_name.clone(), constraint_trait_ids.clone());

                // 如果已存在则重用，否则创建（在 create_generic_param_places 中会创建）
                // 这里只是确保 impl 块的泛型参数也被考虑
                if !self.generic_param_cache.contains_key(&cache_key) {
                    // 如果不存在，会在其他地方创建，这里不做处理
                    debug!("Impl 块的泛型参数 '{}' 将在需要时创建", generic_name);
                }
            }
        }
    }

    /// 为类型的泛型参数创建占位符 Place,并建立 holds 关系
    pub(super) fn create_generic_param_places(
        &mut self,
        type_id: Id,
        item_inner: &ItemEnum,
        type_place_id: PlaceId,
    ) {
        let generics = match item_inner {
            ItemEnum::Struct(s) => &s.generics,
            ItemEnum::Enum(e) => &e.generics,
            ItemEnum::Union(u) => &u.generics,
            ItemEnum::Trait(t) => &t.generics,
            _ => return,
        };

        for param_def in &generics.params {
            // 只处理类型参数(Type),不处理生命周期(Lifetime)和常量(Const)
            if let GenericParamDefKind::Type { bounds, .. } = &param_def.kind {
                let generic_name = param_def.name.clone();

                // 提取约束的 trait IDs
                let mut constraint_trait_ids = Vec::new();
                for bound in bounds {
                    if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                        constraint_trait_ids.push(trait_.id);
                    }
                }
                // 排序以保证相同约束集合使用相同的 key
                constraint_trait_ids.sort();

                // 使用 (generic_name, constraint_trait_ids) 作为 key
                let cache_key = (generic_name.clone(), constraint_trait_ids.clone());

                // 检查是否已经创建过这个约束集合的泛型参数 Place
                let generic_place_id = if let Some(&place_id) =
                    self.generic_param_cache.get(&cache_key)
                {
                    // 已存在，重用
                    place_id
                } else {
                    // 创建新的泛型参数占位符 Place
                    let generic_id = self.generate_temp_id();
                    let constraint_str = if constraint_trait_ids.is_empty() {
                        "".to_string()
                    } else {
                        let trait_names: Vec<String> = constraint_trait_ids
                            .iter()
                            .filter_map(|trait_id| self.get_type_name(trait_id))
                            .collect();
                        if trait_names.is_empty() {
                            format!(": {:?}", constraint_trait_ids)
                        } else {
                            format!(": {}", trait_names.join(" + "))
                        }
                    };
                    let generic_place = Place::new(
                        generic_id,
                        format!("{}{}", generic_name, constraint_str),
                        format!(
                            "generic_param::{}::{:?}",
                            generic_name, constraint_trait_ids
                        ),
                        PlaceKind::GenericParam(generic_name.clone(), constraint_trait_ids.clone()),
                    );
                    let place_id = self.net.add_place_and_get_id(generic_place);

                    // 缓存泛型参数 Place
                    self.generic_param_cache.insert(cache_key, place_id);

                    // 为有约束的泛型参数创建 impls 变迁链接到 trait
                    for trait_id in &constraint_trait_ids {
                        if let Some(&trait_place_id) = self.type_place_map.get(trait_id) {
                            self.create_impls_transition(
                                generic_id,
                                *trait_id,
                                place_id,
                                trait_place_id,
                            );
                            debug!("✨ 泛型参数 '{}' 约束于 trait {:?}", generic_name, trait_id);
                        }
                    }

                    place_id
                };

                // 创建 holds 关系:类型 -> 泛型参数
                let dummy_member_id = self.generate_temp_id();
                self.create_holds_transition(
                    type_id,
                    dummy_member_id,
                    type_place_id,
                    generic_place_id,
                );
            }
        }
    }

    /// 为 Trait 的 items 创建 holds 关系（关联类型和函数签名）
    /// items 只和 trait 库所链接，不创建独立的 Place 或 Transition
    pub(super) fn create_trait_items_places(
        &mut self,
        trait_id: Id,
        item_inner: &ItemEnum,
        trait_place_id: PlaceId,
    ) {
        let trait_def = match item_inner {
            ItemEnum::Trait(t) => t,
            _ => return,
        };

        // 遍历 trait 的所有 items
        for item_id in &trait_def.items {
            if let Some(trait_item) = self.crate_.index.get(item_id) {
                match &trait_item.inner {
                    ItemEnum::AssocType {
                        generics: _,
                        bounds,
                        type_: _,
                    } => {
                        // 关联类型：创建 holds 关系，但不创建独立的 Place
                        // 关联类型的信息已经保存在 Trait Place 中
                        let assoc_type_name =
                            trait_item.name.as_deref().unwrap_or("UnnamedAssocType");

                        // 为关联类型创建一个轻量级的 Place（仅用于 holds 关系）
                        // 这个 Place 不包含详细的类型信息，只表示"这是 trait 的一个关联类型"
                        let assoc_place = Place::new(
                            *item_id,
                            format!("{}::{}", self.get_trait_name(trait_id), assoc_type_name),
                            format!(
                                "trait_item::assoc_type::{}::{}",
                                trait_id.0, assoc_type_name
                            ),
                            PlaceKind::AssocType(
                                trait_id,
                                assoc_type_name.to_string(),
                                bounds
                                    .iter()
                                    .filter_map(|bound| {
                                        if let rustdoc_types::GenericBound::TraitBound {
                                            trait_,
                                            ..
                                        } = bound
                                        {
                                            Some(trait_.path.clone())
                                        } else {
                                            None
                                        }
                                    })
                                    .collect(),
                            ),
                        );
                        let assoc_place_id = self.net.add_place_and_get_id(assoc_place);

                        // 将关联类型存入 type_place_map,以便后续查找（例如 QualifiedPath）
                        self.type_place_map.insert(*item_id, assoc_place_id);

                        // 创建 holds 关系:trait -> 关联类型
                        self.create_holds_transition(
                            trait_id,
                            *item_id,
                            trait_place_id,
                            assoc_place_id,
                        );

                        debug!(
                            "✨ 创建 trait 关联类型 '{}::{}' (ID: {:?}) 的 holds 关系",
                            self.get_trait_name(trait_id),
                            assoc_type_name,
                            item_id
                        );
                    }
                    ItemEnum::Function(_func) => {
                        // 函数签名：创建 holds 关系，但不创建独立的 Transition
                        // 函数签名的信息已经保存在 Trait Place 中
                        let func_name = trait_item.name.as_deref().unwrap_or("(匿名)");

                        // 为函数签名创建一个轻量级的 Place（仅用于 holds 关系）
                        // 这个 Place 不包含输入输出的详细链接，只表示"这是 trait 的一个方法签名"
                        let func_signature_place = Place::new(
                            *item_id,
                            format!("{}::{}", self.get_trait_name(trait_id), func_name),
                            format!("trait_item::function::{}::{}", trait_id.0, func_name),
                            PlaceKind::StructField(Type::Infer), // 使用 StructField 作为占位符，表示这是 trait 的一个方法
                        );
                        let func_signature_place_id =
                            self.net.add_place_and_get_id(func_signature_place);

                        // 创建 holds 关系:trait -> 函数签名
                        self.create_holds_transition(
                            trait_id,
                            *item_id,
                            trait_place_id,
                            func_signature_place_id,
                        );

                        // 标记这个函数是 trait 定义中的方法，后续不需要为它创建 Transition
                        self.impl_function_ids.insert(*item_id);

                        debug!(
                            "✨ 创建 trait 方法签名 '{}::{}' (ID: {:?}) 的 holds 关系",
                            self.get_trait_name(trait_id),
                            func_name,
                            item_id
                        );
                    }
                    _ => {
                        // 其他类型的 items 暂不处理
                        debug!(
                            "⚠️  Trait '{}' 的 item {:?} 类型暂不支持",
                            self.get_trait_name(trait_id),
                            item_id
                        );
                    }
                }
            }
        }
    }

     /// 查找类型对应的 Place
    ///
    /// 支持以下类型:
    /// - ResolvedPath: 查找 type_place_map
    /// - Generic: 查找 generic_param_cache
    /// - Primitive, Tuple, Slice, Array 等: 查找 type_cache
    pub(super) fn find_type_place(&self, ty: &Type, owner_type_id: Id) -> Option<PlaceId> {
        match ty {
            Type::ResolvedPath(path) => {
                // 查找结构体、枚举、联合体等
                self.type_place_map.get(&path.id).copied()
            }
            Type::Generic(generic_name) => {
                // 查找泛型参数占位符
                // 根据约束查找对应的库所
                let constraint_trait_ids =
                    self.find_generic_constraint_trait_ids(owner_type_id, generic_name);
                let cache_key = (generic_name.clone(), constraint_trait_ids);
                self.generic_param_cache.get(&cache_key).copied()
            }
            Type::Primitive(name) => {
                // 查找基本类型
                let type_key = format!("primitive:{}", name);
                self.type_cache.get(&type_key).copied()
            }
            Type::Tuple(_) => {
                // 查找元组类型 
                let type_key = format!("tuple:{}", format_type(ty));
                self.type_cache.get(&type_key).copied()
            }
            Type::Slice(_) => {
                // 查找切片类型
                let type_key = format!("slice:{}", format_type(ty));
                self.type_cache.get(&type_key).copied()
            }
            Type::Array { type_, len } => {
                // 查找数组类型
                let type_key = format!("array:{}:{}", format_type(type_), len);
                self.type_cache.get(&type_key).copied()
            }
            Type::RawPointer { is_mutable, type_ } => {
                // 查找原始指针类型
                let mutability = if *is_mutable { "mut" } else { "const" };
                let type_key = format!("rawptr:{}:{}", mutability, format_type(type_));
                self.type_cache.get(&type_key).copied()
            }
            Type::Infer => {
                // 查找推断类型
                let type_key = "infer:_".to_string();
                self.type_cache.get(&type_key).copied()
            }
            _ => {
                // 其他类型暂不支持
                None
            }
        }
    }

     /// 在函数上下文中查找类型对应的 Place
    ///
    /// 与 find_type_place 的区别:
    /// - 支持 Self 类型解析(通过 receiver_id)
    /// - 优先查找函数级别的泛型参数,然后查找类型级别的泛型参数
    ///
    /// # 参数
    /// - `ty`: 要查找的类型
    /// - `function_id`: 函数的 ID(用于查找函数级别的泛型)
    /// - `receiver_id`: impl 块的接收者 ID(用于解析 Self 和类型级别的泛型)
    pub(super) fn find_type_place_in_function(
        &mut self,
        ty: &Type,
        function_id: Id,
        receiver_id: Option<Id>,
    ) -> Option<PlaceId> {
        match ty {
            Type::Generic(generic_name) => {
                // 特殊处理 Self
                if generic_name == "Self" {
                    if let Some(receiver_id) = receiver_id {
                        if let Some(&place_id) = self.type_place_map.get(&receiver_id) {
                            return Some(place_id);
                        } else {
                            warn!(
                                "❌ 错误:Self 类型在函数 {:?} 中使用,但接收者类型 {:?} 的 Place 不存在",
                                function_id, receiver_id
                            );
                            return None;
                        }
                    }
                    // Self 但没有 receiver_id,这是无约束函数
                    if let Some(item) = self.crate_.index.get(&function_id) {
                        let func_name = item.name.as_deref().unwrap_or("(匿名)");
                        warn!(
                            "⚠️  无约束函数 '{}' (ID: {:?}) 的返回值包含 Self 类型,但无法确定 Self 的具体类型(函数不在 impl 块中)",
                            func_name, function_id
                        );
                    } else {
                        warn!(
                            "⚠️  无约束函数 (ID: {:?}) 的返回值包含 Self 类型,但无法确定 Self 的具体类型(函数不在 impl 块中)",
                            function_id
                        );
                    }
                    return None;
                }

                // 特殊处理:标准 trait 的泛型参数 T/U 映射到 Self
                // 例如 Borrow<T> 的 T, BorrowMut<T> 的 T, ToOwned 的 T
                if (generic_name == "T" || generic_name == "U") && receiver_id.is_some() {
                    let receiver_id_val = receiver_id.unwrap();
                    // 先检查是否真的有这个泛型参数
                    let func_constraints =
                        self.find_generic_constraint_trait_ids(function_id, generic_name);
                    let type_constraints =
                        self.find_generic_constraint_trait_ids(receiver_id_val, generic_name);

                    let func_cache_key = (generic_name.clone(), func_constraints);
                    let type_cache_key = (generic_name.clone(), type_constraints);

                    let has_func_generic = self.generic_param_cache.contains_key(&func_cache_key);
                    let has_type_generic = self.generic_param_cache.contains_key(&type_cache_key);

                    // 如果既没有函数级泛型,也没有类型级泛型,可能是标准 trait
                    if !has_func_generic && !has_type_generic {
                        // 尝试映射到 Self
                        if let Some(&place_id) = self.type_place_map.get(&receiver_id_val) {
                            return Some(place_id);
                        }
                    }
                }

                // 先查找函数级别的泛型参数
                let func_constraints =
                    self.find_generic_constraint_trait_ids(function_id, generic_name);
                let func_cache_key = (generic_name.clone(), func_constraints);
                if let Some(&place_id) = self.generic_param_cache.get(&func_cache_key) {
                    return Some(place_id);
                }

                // 再查找类型级别的泛型参数(如果在 impl 块中)
                if let Some(receiver_id) = receiver_id {
                    let type_constraints =
                        self.find_generic_constraint_trait_ids(receiver_id, generic_name);
                    let type_cache_key = (generic_name.clone(), type_constraints);
                    if let Some(&place_id) = self.generic_param_cache.get(&type_cache_key) {
                        return Some(place_id);
                    }

                    // 如果在 impl 块中但找不到泛型参数,检查类型是否有这个泛型
                    if let Some(type_item) = self.crate_.index.get(&receiver_id) {
                        let has_generic = type_has_generic(&type_item.inner, generic_name);
                        if !has_generic {
                            // 不报告错误,可能是标准 trait 的泛型(已经映射到 Self)
                            debug!(
                                "泛型参数 '{}' 在函数 {:?} 中使用,但类型 '{}' (ID: {:?}) 不包含此泛型参数(可能是标准 trait)",
                                generic_name,
                                function_id,
                                type_item.name.as_deref().unwrap_or("?"),
                                receiver_id
                            );
                        } else {
                            // 类型有这个泛型参数,但未创建 Place
                            // 尝试链接到 receiver_id 的类型 Place 作为后备
                            if let Some(&receiver_place_id) = self.type_place_map.get(&receiver_id)
                            {
                                warn!(
                                    "⚠️  泛型参数 '{}' 在函数 {:?} 中使用,类型 '{}' (ID: {:?}) 有此泛型但未创建 Place.将链接到类型 Place 作为后备.",
                                    generic_name,
                                    function_id,
                                    type_item.name.as_deref().unwrap_or("?"),
                                    receiver_id
                                );
                                return Some(receiver_place_id);
                            } else {
                                warn!(
                                    "❌ 错误:泛型参数 '{}' 在函数 {:?} 中使用,类型 '{}' (ID: {:?}) 有此泛型但未创建 Place,且类型 Place 也不存在",
                                    generic_name,
                                    function_id,
                                    type_item.name.as_deref().unwrap_or("?"),
                                    receiver_id
                                );
                            }
                        }
                    }
                } else {
                    // 自由函数中的泛型参数
                    if let Some(item) = self.crate_.index.get(&function_id) {
                        let func_name = item.name.as_deref().unwrap_or("(匿名)");
                        warn!(
                            "⚠️  无约束函数 '{}' (ID: {:?}) 的返回值包含泛型参数 '{}',但无法确定其具体类型(函数不在 impl 块中,且泛型参数未创建 Place)",
                            func_name, function_id, generic_name
                        );
                    } else {
                        warn!(
                            "⚠️  无约束函数 (ID: {:?}) 的返回值包含泛型参数 '{}',但无法确定其具体类型(函数不在 impl 块中,且泛型参数未创建 Place)",
                            function_id, generic_name
                        );
                    }
                }

                None
            }
            _ => {
                // 其他类型:先尝试查找,如果不存在则创建
                // 特别处理 Result 和 Option
                let owner_type_id = receiver_id.unwrap_or(function_id);

                // 先尝试查找
                if let Some(place_id) = self.find_type_place(ty, owner_type_id) {
                    return Some(place_id);
                }

                // 如果不存在,尝试创建(特别是 Result/Option/Primitive 等)
                // 为此类型生成一个临时 ID
                let temp_id = self.generate_temp_id();
                self.create_or_get_type_place(ty, &temp_id, owner_type_id)
            }
        }
    }
}
