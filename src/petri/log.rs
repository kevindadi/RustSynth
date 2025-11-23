use std::{fs::File, io::Write, path::PathBuf};

use rustdoc_types::{GenericParamDefKind, Id, ItemEnum};

use crate::petri::{PetriNetBuilder, utils::format_generics};

/// 分析泛型偏序关系
/// 生成一个报告,显示哪些泛型可以使用哪些泛型,以及约束之间的层级关系
pub fn analyze_generic_partial_order(
    builder: &PetriNetBuilder,
    log_path: &PathBuf,
) -> std::io::Result<()> {
    use std::collections::{HashMap, HashSet};

    let mut file = File::create(log_path)?;
    writeln!(file, "🔗 泛型偏序关系分析\n")?;

    // Rust 编译器默认实现的 trait
    let default_traits: HashSet<&str> = [
        "Send",
        "Sync",
        "Sized",
        "Copy",
        "Clone",
        "Debug",
        "Display",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "Hash",
        "Default",
        "Drop",
        "Unpin",
        "UnwindSafe",
        "RefUnwindSafe",
    ]
    .iter()
    .cloned()
    .collect();

    // 收集所有泛型参数及其约束
    #[derive(Debug, Clone)]
    struct GenericInfo {
        #[allow(unused)]
        owner_id: Id,
        owner_name: String,
        generic_name: String,
        constraints: Vec<String>, // trait 路径列表
    }

    let mut generic_infos: Vec<GenericInfo> = Vec::new();

    // 从类型定义中收集泛型
    for item in builder.crate_.index.values() {
        let owner_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
        let generics = match &item.inner {
            ItemEnum::Struct(s) => Some(&s.generics),
            ItemEnum::Enum(e) => Some(&e.generics),
            ItemEnum::Union(u) => Some(&u.generics),
            ItemEnum::Trait(t) => Some(&t.generics),
            _ => None,
        };

        if let Some(generics) = generics {
            for param in &generics.params {
                if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                    let constraints: Vec<String> = bounds
                        .iter()
                        .filter_map(|bound| {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                Some(trait_.path.clone())
                            } else {
                                None
                            }
                        })
                        .collect();

                    generic_infos.push(GenericInfo {
                        owner_id: item.id,
                        owner_name: owner_name.clone(),
                        generic_name: param.name.clone(),
                        constraints,
                    });
                }
            }
        }
    }

    // 从函数和 impl 中收集泛型
    for item in builder.crate_.index.values() {
        let owner_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
        let generics = match &item.inner {
            ItemEnum::Function(f) => Some(&f.generics),
            ItemEnum::Impl(i) => Some(&i.generics),
            _ => None,
        };

        if let Some(generics) = generics {
            for param in &generics.params {
                if let GenericParamDefKind::Type { bounds, .. } = &param.kind {
                    let constraints: Vec<String> = bounds
                        .iter()
                        .filter_map(|bound| {
                            if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                                Some(trait_.path.clone())
                            } else {
                                None
                            }
                        })
                        .collect();

                    generic_infos.push(GenericInfo {
                        owner_id: item.id,
                        owner_name: owner_name.clone(),
                        generic_name: param.name.clone(),
                        constraints,
                    });
                }
            }
        }
    }

    // 构建约束图:trait -> 满足该约束的泛型列表
    let mut trait_to_generics: HashMap<String, Vec<&GenericInfo>> = HashMap::new();
    for info in &generic_infos {
        for constraint in &info.constraints {
            trait_to_generics
                .entry(constraint.clone())
                .or_insert_with(Vec::new)
                .push(info);
        }
    }

    // 分析偏序关系
    writeln!(file, "📊 泛型约束统计:\n")?;
    writeln!(file, "  总泛型参数数量: {}\n", generic_infos.len())?;

    // 按约束分组显示
    writeln!(file, "🎯 按 Trait 约束分组:\n")?;
    let mut sorted_traits: Vec<_> = trait_to_generics.iter().collect();
    sorted_traits.sort_by_key(|(trait_name, _)| *trait_name);

    for (trait_name, generics) in &sorted_traits {
        let is_default = default_traits.contains(trait_name.as_str());
        let default_marker = if is_default { " (默认 trait)" } else { "" };
        writeln!(file, "  • {}:{}", trait_name, default_marker)?;
        for info in *generics {
            writeln!(file, "    └─ {}.{}", info.owner_name, info.generic_name)?;
        }
        writeln!(file)?;
    }

    // 分析哪些泛型可以用于哪些约束
    writeln!(file, "🔗 泛型可用性分析:\n")?;
    writeln!(
        file,
        "  说明: 如果泛型 T 满足约束 A + B,那么 T 可以用于需要 A 或 B 的地方\n"
    )?;

    for info in &generic_infos {
        if info.constraints.is_empty() {
            writeln!(
                file,
                "  • {}.{}: 无约束 (可用于任何地方)",
                info.owner_name, info.generic_name
            )?;
        } else {
            writeln!(
                file,
                "  • {}.{}: 满足 {}",
                info.owner_name,
                info.generic_name,
                info.constraints.join(" + ")
            )?;
            writeln!(file, "    可以用于需要以下任一约束的地方:")?;
            for constraint in &info.constraints {
                writeln!(file, "      - {}", constraint)?;
            }
        }
        writeln!(file)?;
    }

    // 分析约束层级关系(如果 T: A + B,且 A: C,那么 T 也满足 C)
    writeln!(file, "📈 约束层级关系:\n")?;
    writeln!(file, "  说明: 如果 T: A,且 A: B,那么 T 也满足 B (传递性)\n")?;

    // 查找 trait 之间的继承关系(通过 impl 块)
    let mut trait_supertraits: HashMap<String, Vec<String>> = HashMap::new();
    for item in builder.crate_.index.values() {
        if let ItemEnum::Trait(trait_def) = &item.inner {
            let trait_name = item.name.as_deref().unwrap_or("(匿名)").to_string();
            let supertraits: Vec<String> = trait_def
                .bounds
                .iter()
                .filter_map(|bound| {
                    if let rustdoc_types::GenericBound::TraitBound { trait_, .. } = bound {
                        Some(trait_.path.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if !supertraits.is_empty() {
                trait_supertraits.insert(trait_name, supertraits);
            }
        }
    }

    if !trait_supertraits.is_empty() {
        writeln!(file, "  Trait 继承关系:\n")?;
        for (trait_name, supertraits) in &trait_supertraits {
            writeln!(file, "    {}: {:?}", trait_name, supertraits)?;
        }
        writeln!(file)?;
    } else {
        writeln!(file, "  (未发现显式的 trait 继承关系)\n")?;
    }

    // 分析同级别约束(满足相同约束集合的泛型)
    writeln!(file, "⚖️  同级别约束分析:\n")?;
    writeln!(file, "  说明: 满足相同约束集合的泛型被视为同级别\n")?;

    let mut constraint_groups: HashMap<Vec<String>, Vec<&GenericInfo>> = HashMap::new();
    for info in &generic_infos {
        let mut constraints = info.constraints.clone();
        constraints.sort();
        constraint_groups
            .entry(constraints)
            .or_insert_with(Vec::new)
            .push(info);
    }

    let mut sorted_groups: Vec<_> = constraint_groups.iter().collect();
    sorted_groups.sort_by_key(|(constraints, _)| constraints.len());
    sorted_groups.reverse(); // 从约束最多的开始

    for (constraints, generics) in &sorted_groups {
        if generics.len() > 1 {
            writeln!(file, "  约束集合: {}", constraints.join(" + "))?;
            writeln!(file, "  满足该约束的泛型 (共 {} 个):", generics.len())?;
            for info in *generics {
                writeln!(file, "    • {}.{}", info.owner_name, info.generic_name)?;
            }
            writeln!(file)?;
        }
    }

    // 默认 trait 说明
    writeln!(file, "🔧 Rust 默认 Trait 说明:\n")?;
    writeln!(
        file,
        "  以下 trait 由 Rust 编译器自动实现(如果类型满足条件):\n"
    )?;
    for trait_name in &default_traits {
        writeln!(file, "    • {}", trait_name)?;
    }
    writeln!(file)?;
    writeln!(
        file,
        "  注意: 这些 trait 可能不会在约束中显式出现,但类型可能自动满足它们\n"
    )?;

    writeln!(
        file,
        "================================================================================\n"
    )?;

    Ok(())
}

/// 生成类型日志文件,记录所有类型及其泛型约束
pub fn generate_type_log(builder: &PetriNetBuilder, log_path: &PathBuf) -> std::io::Result<()> {
    let mut file = File::create(log_path)?;
    writeln!(file, "📋 Rust 类型清单\n")?;

    // 收集所有类型
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut unions = Vec::new();
    let mut traits = Vec::new();
    let mut variants = Vec::new();
    let mut generics = Vec::new();
    let mut primitives = Vec::new();
    let mut others = Vec::new();

    for item in builder.crate_.index.values() {
        let name = item.name.as_deref().unwrap_or("(匿名)");
        let id_str = format!("{:?}", item.id);

        match &item.inner {
            ItemEnum::Struct(s) => {
                let generics_info = format_generics(&s.generics);
                structs.push((name.to_string(), id_str, generics_info));
            }
            ItemEnum::Enum(e) => {
                let generics_info = format_generics(&e.generics);
                enums.push((name.to_string(), id_str, generics_info));
            }
            ItemEnum::Union(u) => {
                let generics_info = format_generics(&u.generics);
                unions.push((name.to_string(), id_str, generics_info));
            }
            ItemEnum::Trait(t) => {
                let generics_info = format_generics(&t.generics);
                traits.push((name.to_string(), id_str, generics_info));
            }
            ItemEnum::Variant(_v) => {
                // Variant 没有自己的 generics,它继承自 Enum
                variants.push((name.to_string(), id_str, String::new()));
            }
            ItemEnum::Primitive(_) => {
                primitives.push((name.to_string(), id_str, String::new()));
            }
            _ => {
                others.push((name.to_string(), id_str, String::new()));
            }
        }
    }

    // 收集泛型参数(从 generic_param_cache)
    for ((generic_name, constraint_trait_ids), _place_id) in &builder.generic_param_cache {
        // 格式化约束信息
        let constraints = if constraint_trait_ids.is_empty() {
            String::new()
        } else {
            let trait_names: Vec<String> = constraint_trait_ids
                .iter()
                .filter_map(|trait_id| {
                    if let Some(item) = builder.crate_.index.get(trait_id) {
                        item.name.clone()
                    } else {
                        None
                    }
                })
                .collect();
            if trait_names.is_empty() {
                format!(": {:?}", constraint_trait_ids)
            } else {
                format!(": {}", trait_names.join(" + "))
            }
        };

        generics.push((
            format!("{}{}", generic_name, constraints),
            format!("{:?}", constraint_trait_ids),
            constraints,
        ));
    }

    // 写入 Struct 类型
    if !structs.is_empty() {
        writeln!(file, "📦 Struct 类型 (共 {} 个):\n", structs.len())?;
        for (name, id, generics_info) in &structs {
            writeln!(file, "  • {} (ID: {})", name, id)?;
            if !generics_info.is_empty() {
                writeln!(file, "    {}", generics_info)?;
            }
        }
        writeln!(file)?;
    }

    // 写入 Enum 类型
    if !enums.is_empty() {
        writeln!(file, "🔢 Enum 类型 (共 {} 个):\n", enums.len())?;
        for (name, id, generics_info) in &enums {
            writeln!(file, "  • {} (ID: {})", name, id)?;
            if !generics_info.is_empty() {
                writeln!(file, "    {}", generics_info)?;
            }
        }
        writeln!(file)?;
    }

    // 写入 Union 类型
    if !unions.is_empty() {
        writeln!(file, "🔗 Union 类型 (共 {} 个):\n", unions.len())?;
        for (name, id, generics_info) in &unions {
            writeln!(file, "  • {} (ID: {})", name, id)?;
            if !generics_info.is_empty() {
                writeln!(file, "    {}", generics_info)?;
            }
        }
        writeln!(file)?;
    }

    // 写入 Trait 类型
    if !traits.is_empty() {
        writeln!(file, "🎯 Trait 类型 (共 {} 个):\n", traits.len())?;
        for (name, id, generics_info) in &traits {
            writeln!(file, "  • {} (ID: {})", name, id)?;
            if !generics_info.is_empty() {
                writeln!(file, "    {}", generics_info)?;
            }
        }
        writeln!(file)?;
    }

    // 写入 Variant 类型
    if !variants.is_empty() {
        writeln!(file, "🔀 Variant 类型 (共 {} 个):\n", variants.len())?;
        for (name, id, generics_info) in &variants {
            writeln!(file, "  • {} (ID: {})", name, id)?;
            if !generics_info.is_empty() {
                writeln!(file, "    {}", generics_info)?;
            }
        }
        writeln!(file)?;
    }

    // 写入泛型参数
    if !generics.is_empty() {
        writeln!(file, "🔧 泛型参数 (共 {} 个):\n", generics.len())?;
        for (name, id, constraints) in &generics {
            writeln!(file, "  • {} (Owner ID: {})", name, id)?;
            if !constraints.is_empty() {
                writeln!(file, "    约束: {}", constraints)?;
            } else {
                writeln!(file, "    约束: 无")?;
            }
        }
        writeln!(file)?;
    }

    // 写入 Primitive 类型
    if !primitives.is_empty() {
        writeln!(file, "⚡ Primitive 类型 (共 {} 个):\n", primitives.len())?;
        for (name, id, _) in &primitives {
            writeln!(file, "  • {} (ID: {})", name, id)?;
        }
        writeln!(file)?;
    }

    // 写入其他类型
    if !others.is_empty() {
        writeln!(file, "📝 其他类型 (共 {} 个):\n", others.len())?;
        for (name, id, _) in &others {
            writeln!(file, "  • {} (ID: {})", name, id)?;
        }
        writeln!(file)?;
    }

    writeln!(
        file,
        "================================================================================\n"
    )?;
    writeln!(file, "📊 统计信息:\n")?;
    writeln!(file, "  • Struct:  {} 个", structs.len())?;
    writeln!(file, "  • Enum:    {} 个", enums.len())?;
    writeln!(file, "  • Union:   {} 个", unions.len())?;
    writeln!(file, "  • Trait:   {} 个", traits.len())?;
    writeln!(file, "  • Variant: {} 个", variants.len())?;
    writeln!(file, "  • 泛型参数: {} 个", generics.len())?;
    writeln!(file, "  • Primitive: {} 个", primitives.len())?;
    writeln!(file, "  • 其他:     {} 个", others.len())?;
    writeln!(
        file,
        "  • 总计:    {} 个类型\n",
        structs.len()
            + enums.len()
            + unions.len()
            + traits.len()
            + variants.len()
            + generics.len()
            + primitives.len()
            + others.len()
    )?;
    writeln!(
        file,
        "================================================================================\n"
    )?;

    Ok(())
}
