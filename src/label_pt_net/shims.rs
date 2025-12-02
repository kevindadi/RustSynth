use crate::ir_graph::{EdgeMode, IrGraph, NodeInfo, NodeType};
use crate::label_pt_net::net::LabeledPetriNet;
use crate::support_types::PRIMITIVE_DEFAULT_TRAITS;
use crate::support_types::primitives::PRIMITIVE_TYPES;
use std::collections::{HashMap, HashSet};

impl LabeledPetriNet {
    /// 添加基本类型 shims
    ///
    /// 为常见基本类型(i32, bool, f64, str 等)添加模拟 places,
    /// 并链接它们的默认 Trait 实现.
    ///
    /// # 功能
    /// 1. 为每个基本类型创建 shim place(如 "shim_i32")
    /// 2. 设置初始标记为 1(表示类型可用)
    /// 3. 链接默认 Trait(如 Copy, Clone, Debug 等)
    /// 4. 如果 ir 中存在对应的 Generic 节点,添加 Instance 弧
    pub fn add_primitive_shims(&mut self, ir: &IrGraph) {
        // 收集 ir 中已存在的 Primitive 节点名称
        let existing_primitives: HashSet<String> = ir
            .node_infos
            .iter()
            .filter_map(|(_, info)| {
                if let NodeInfo::Primitive(prim_info) = info {
                    Some(prim_info.name.clone())
                } else {
                    None
                }
            })
            .collect();

        // 收集 ir 中的 Trait 节点(名称 -> place 索引)
        let trait_places: HashMap<String, usize> = self
            .places
            .iter()
            .enumerate()
            .filter_map(|(idx, name)| {
                // 检查是否是 Trait place
                if let Some(&node_idx) = self
                    .place_to_node
                    .iter()
                    .find(|(place_idx, _)| **place_idx == idx)
                    .map(|(_, node_idx)| node_idx)
                {
                    if let Some(NodeType::Trait) = ir.node_types.get(&node_idx) {
                        // 提取 Trait 名称(可能是完整路径)
                        let trait_name = name.split("::").last().unwrap_or(name);
                        return Some((trait_name.to_string(), idx));
                    }
                }
                None
            })
            .collect();

        // 收集 ir 中的 Generic 节点(用于链接)
        let generic_places: Vec<(usize, Vec<String>)> = self
            .places
            .iter()
            .enumerate()
            .filter_map(|(idx, _)| {
                if let Some(&node_idx) = self
                    .place_to_node
                    .iter()
                    .find(|(place_idx, _)| **place_idx == idx)
                    .map(|(_, node_idx)| node_idx)
                {
                    if let Some(NodeInfo::Generic(gen_info)) = ir.node_infos.get(&node_idx) {
                        // 获取 bounds 的名称
                        let bound_names: Vec<String> = gen_info
                            .bounds
                            .iter()
                            .filter_map(|bound_idx| {
                                ir.node_infos
                                    .get(bound_idx)
                                    .map(|info| info.name().to_string())
                            })
                            .collect();
                        return Some((idx, bound_names));
                    }
                }
                None
            })
            .collect();

        // 为每个基本类型添加 shim
        for prim_name in PRIMITIVE_TYPES.iter() {
            let shim_name = format!("shim_{}", prim_name);

            // 检查是否已存在
            if self.places.contains(&shim_name) {
                continue;
            }

            // 如果 ir 中已有该 Primitive,跳过
            if existing_primitives.contains(*prim_name) {
                continue;
            }

            // 添加 shim place
            let shim_idx = self.add_place(shim_name.clone());
            self.set_initial_marking(shim_idx, 1); // 初始标记为 1

            // 获取该类型的默认 Trait
            if let Some(default_traits) = PRIMITIVE_DEFAULT_TRAITS.get(*prim_name) {
                for trait_name in default_traits.iter() {
                    // 查找对应的 Trait place
                    if let Some(&trait_place_idx) = trait_places.get(*trait_name) {
                        // 添加 Implements 弧:shim -> trait
                        self.add_output_arc(
                            shim_idx, // 这里我们创建一个虚拟 transition
                            trait_place_idx,
                            EdgeMode::Implements,
                            1,
                            Some(format!("{} implements {}", prim_name, trait_name)),
                        );
                    }
                }

                // 链接到满足 bounds 的 Generic places
                for (gen_place_idx, bounds) in &generic_places {
                    // 检查 shim 是否满足所有 bounds
                    let satisfies_all = bounds
                        .iter()
                        .all(|bound| default_traits.contains(&bound.as_str()));

                    if satisfies_all && !bounds.is_empty() {
                        // 创建虚拟 transition 连接 shim 到 generic
                        let virtual_trans_name = format!(
                            "{}_instance_{}",
                            prim_name,
                            self.places
                                .get(*gen_place_idx)
                                .map(|s| s.as_str())
                                .unwrap_or("?")
                        );
                        let trans_idx = self.add_transition(virtual_trans_name);

                        // shim -> transition (输入)
                        self.add_input_arc(
                            shim_idx,
                            trans_idx,
                            EdgeMode::Ref, // 使用 Ref 不消耗 token
                            1,
                            None,
                        );

                        // transition -> generic (输出)
                        self.add_output_arc(trans_idx, *gen_place_idx, EdgeMode::Instance, 1, None);
                    }
                }
            }
        }

        // 额外添加 String shim(不是 primitive 但常用)
        self.add_string_shim(&trait_places, &generic_places);
    }

    /// 添加 String 类型 shim
    fn add_string_shim(
        &mut self,
        trait_places: &HashMap<String, usize>,
        generic_places: &[(usize, Vec<String>)],
    ) {
        let shim_name = "shim_String".to_string();

        if self.places.contains(&shim_name) {
            return;
        }

        let shim_idx = self.add_place(shim_name);
        self.set_initial_marking(shim_idx, 1);

        // String 的默认 Trait
        let string_traits = [
            "Clone",
            "Debug",
            "Display",
            "PartialEq",
            "Eq",
            "PartialOrd",
            "Ord",
            "Hash",
            "Default",
            "Send",
            "Sync",
            "Sized",
            "From",
            "Into",
            "AsRef",
            "Deref",
            "Borrow",
        ];

        for trait_name in string_traits.iter() {
            if let Some(&trait_place_idx) = trait_places.get(*trait_name) {
                self.add_output_arc(
                    shim_idx,
                    trait_place_idx,
                    EdgeMode::Implements,
                    1,
                    Some(format!("String implements {}", trait_name)),
                );
            }
        }

        // 链接到 Generic places
        for (gen_place_idx, bounds) in generic_places {
            let satisfies_all = bounds
                .iter()
                .all(|bound| string_traits.contains(&bound.as_str()));

            if satisfies_all && !bounds.is_empty() {
                let virtual_trans_name = format!(
                    "String_instance_{}",
                    self.places
                        .get(*gen_place_idx)
                        .map(|s| s.as_str())
                        .unwrap_or("?")
                );
                let trans_idx = self.add_transition(virtual_trans_name);

                self.add_input_arc(shim_idx, trans_idx, EdgeMode::Ref, 1, None);
                self.add_output_arc(trans_idx, *gen_place_idx, EdgeMode::Instance, 1, None);
            }
        }
    }
}
