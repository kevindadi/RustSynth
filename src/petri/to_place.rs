//! 库所(Place)创建相关的函数
//! 
//! 这个模块包含 PetriNetBuilder 中与 Place 创建相关的方法实现

use log::debug;
use rustdoc_types::{GenericParamDefKind, Id, Impl, Item, ItemEnum, Type};

use super::net::PlaceId;
use super::structure::{Place, PlaceKind};
use super::builder::PetriNetBuilder;

impl<'a> PetriNetBuilder<'a> {
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
                let generic_place_id = if let Some(&place_id) = self.generic_param_cache.get(&cache_key) {
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
                        format!("generic_param::{}::{:?}", generic_name, constraint_trait_ids),
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
                            debug!(
                                "✨ 泛型参数 '{}' 约束于 trait {:?}",
                                generic_name, trait_id
                            );
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
                        let assoc_type_name = trait_item.name.as_deref().unwrap_or("UnnamedAssocType");
                        
                        // 为关联类型创建一个轻量级的 Place（仅用于 holds 关系）
                        // 这个 Place 不包含详细的类型信息，只表示"这是 trait 的一个关联类型"
                        let assoc_place = Place::new(
                            *item_id,
                            format!("{}::{}", self.get_trait_name(trait_id), assoc_type_name),
                            format!("trait_item::assoc_type::{}::{}", trait_id.0, assoc_type_name),
                            PlaceKind::AssocType(
                                trait_id,
                                assoc_type_name.to_string(),
                                bounds
                                    .iter()
                                    .filter_map(|bound| {
                                        if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
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
                        let func_signature_place_id = self.net.add_place_and_get_id(func_signature_place);

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
}

