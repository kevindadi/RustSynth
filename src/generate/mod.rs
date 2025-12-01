use crate::pt_net::{EdgeKind, NodePayload, PetriNet, PlaceData, TransitionData};
use petgraph::Direction;
use petgraph::visit::EdgeRef;
use std::collections::HashMap;

/// Fuzz Target 生成器
pub struct FuzzTargetGenerator {
    /// 目标 Crate 名称 (用于 use 语句)
    target_crate: String,
}

impl FuzzTargetGenerator {
    pub fn new(target_crate: String) -> Self {
        Self { target_crate }
    }

    /// 生成完整的 fuzz target 代码
    pub fn generate(&self, net: &PetriNet) -> String {
        let mut code = String::new();

        // 1. 生成头部导入
        code.push_str(&self.generate_imports());

        // 2. 收集所有 Place (用于生成 FuzzState)
        // 过滤掉 Source 类型 (它们直接作为 Action 参数,不需要存储在 State 中)
        let complex_places: Vec<&PlaceData> = net
            .graph
            .node_weights()
            .filter_map(|node| node.as_place())
            .filter(|p| !p.is_source)
            .collect();

        // 去重 (可能有多个 Place 对应同一个类型,但我们按 ID 或类型名去重？)
        // 要求: "Pool for std::vec::Vec<u8>".
        // 实际上 PetriNet 中不同 Place 节点代表不同的逻辑位置,但类型可能相同.
        // State 应该按类型聚合吗？
        // Prompt 示例: "pool_vec_u8: Vec<std::vec::Vec<u8>>".
        // 这意味着同一个类型的所有实例共享一个池.
        // 所以我们需要按 type_name 聚合.

        let mut type_to_pool_name: HashMap<String, String> = HashMap::new();
        // type_name -> (resolved_path, sanitized_name)
        let mut unique_types: HashMap<String, (Option<String>, String)> = HashMap::new();

        for place in &complex_places {
            if !unique_types.contains_key(&place.type_name) {
                let sanitized = self.sanitize_type_name(&place.type_name);
                unique_types.insert(
                    place.type_name.clone(),
                    (place.resolved_path.clone(), sanitized.clone()),
                );
                type_to_pool_name.insert(place.type_name.clone(), format!("pool_{}", sanitized));
            }
        }

        // 3. 生成 FuzzState 结构体
        code.push_str(&self.generate_fuzz_state(&unique_types));

        // 4. 生成 Action 枚举
        // 收集所有 Transition
        let transitions: Vec<&TransitionData> = net
            .graph
            .node_weights()
            .filter_map(|node| node.as_transition())
            .collect();

        code.push_str(&self.generate_action_enum(net, &transitions, &unique_types));

        // 5. 生成 Harness
        code.push_str(&self.generate_harness(net, &transitions, &unique_types));

        code
    }

    fn generate_imports(&self) -> String {
        // 注意:现在我们使用完整路径,不再需要简单的 use crate_name 导入
        // 但为了兼容性,保留 prelude 导入
        format!(
            r#"#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

"#
        )
    }

    fn sanitize_type_name(&self, type_name: &str) -> String {
        let mut result = type_name.to_string();

        // 处理元组类型 (T1, T2, ...) -> tuple_T1_T2_...
        if result.starts_with('(') && result.ends_with(')') {
            result = result
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_string();
            result = format!("tuple_{}", result);
        }

        // 处理切片类型 [T] -> array_T
        if result.starts_with('[') && result.ends_with(']') && !result.contains(';') {
            result = result
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            result = format!("array_{}", result);
        }

        // 处理固定大小数组 [T; N] -> array_T_N
        if result.starts_with('[') && result.contains(';') && result.ends_with(']') {
            result = result
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            result = format!("array_{}", result);
        }

        // 通用的符号替换
        result
            .replace("::", "_")
            .replace("<", "_")
            .replace(">", "")
            .replace(" ", "")
            .replace(",", "_")
            .replace("&", "")
            .replace("mut", "")
            .replace("[", "")
            .replace("]", "")
            .replace("(", "")
            .replace(")", "")
            .replace(";", "_")
    }

    fn sanitize_func_name(&self, func_name: &str) -> String {
        // 将 std::fs::File::open 转换为 StdFsFileOpen (PascalCase)
        // 简单的实现:按 :: 分割,首字母大写
        func_name
            .split("::")
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect()
    }

    fn generate_fuzz_state(
        &self,
        unique_types: &HashMap<String, (Option<String>, String)>,
    ) -> String {
        let mut code = String::from("#[derive(Default)]\nstruct FuzzState {\n");

        // 按名称排序以保持生成代码的稳定性
        let mut sorted_types: Vec<_> = unique_types.iter().collect();
        sorted_types.sort_by_key(|(k, _)| *k);

        for (type_name, (resolved_path, sanitized)) in sorted_types {
            // 优先使用完整路径,否则使用简单类型名
            let type_path = resolved_path.as_ref().unwrap_or(type_name);
            // 生成: pool_XXX: Vec<完整路径>
            code.push_str(&format!("    pool_{}: Vec<{}>,\n", sanitized, type_path));
        }

        code.push_str("}\n\n");
        code
    }

    fn generate_action_enum(
        &self,
        net: &PetriNet,
        transitions: &[&TransitionData],
        _unique_types: &HashMap<String, (Option<String>, String)>,
    ) -> String {
        let mut code = String::from("#[derive(Arbitrary, Debug)]\nenum Action {\n");

        for trans in transitions {
            let variant_name = self.sanitize_func_name(&trans.func_name);
            code.push_str(&format!("    {} {{\n", variant_name));

            // 查找输入边
            let trans_idx = net.transition_map[&trans.id];
            let mut inputs: Vec<_> = net
                .graph
                .edges_directed(trans_idx, Direction::Incoming)
                .collect();
            // 按 index 排序
            inputs.sort_by_key(|e| e.weight().index);

            for edge in inputs {
                let source_node = net.graph.node_weight(edge.source()).unwrap();
                if let NodePayload::Place(place) = source_node {
                    let field_name = format!("arg{}", edge.weight().index);

                    if place.is_source {
                        // 直接值: arg0: u8
                        code.push_str(&format!("        {}: {},\n", field_name, place.type_name));
                    } else {
                        // 复杂类型索引: arg0_idx: usize
                        code.push_str(&format!("        {}_idx: usize,\n", field_name));
                    }
                }
            }

            code.push_str("    },\n");
        }

        code.push_str("}\n\n");
        code
    }

    fn generate_harness(
        &self,
        net: &PetriNet,
        transitions: &[&TransitionData],
        unique_types: &HashMap<String, (Option<String>, String)>,
    ) -> String {
        let mut code = String::from(
            r#"fuzz_target!(|actions: Vec<Action>| {
    let mut state = FuzzState::default();
    for action in actions {
        match action {
"#,
        );

        for trans in transitions {
            let variant_name = self.sanitize_func_name(&trans.func_name);
            code.push_str(&format!("            Action::{} {{ ", variant_name));

            // 收集输入变量名,用于解构和后续调用
            let trans_idx = net.transition_map[&trans.id];
            let mut inputs: Vec<_> = net
                .graph
                .edges_directed(trans_idx, Direction::Incoming)
                .collect();
            inputs.sort_by_key(|e| e.weight().index);

            let mut input_vars = Vec::new();

            for edge in inputs.iter() {
                let source_node = net.graph.node_weight(edge.source()).unwrap();
                if let NodePayload::Place(place) = source_node {
                    let field_name = format!("arg{}", edge.weight().index);
                    if place.is_source {
                        code.push_str(&format!("{}, ", field_name));
                        input_vars.push((field_name, place, edge.weight()));
                    } else {
                        let idx_name = format!("{}_idx", field_name);
                        code.push_str(&format!("{}, ", idx_name));
                        input_vars.push((idx_name, place, edge.weight()));
                    }
                }
            }
            code.push_str("} => {\n");

            // 生成函数调用准备代码
            // 检查复杂类型的索引是否有效
            let mut check_code = String::new();
            let mut args_code = Vec::new();

            for (var_name, place, edge_data) in &input_vars {
                if place.is_source {
                    // 直接使用
                    args_code.push(var_name.clone());
                } else {
                    // 从 pool 获取
                    let sanitized_type = self.sanitize_type_name(&place.type_name);
                    let pool_name = format!("state.pool_{}", sanitized_type);

                    // 边界检查
                    check_code.push_str(&format!(
                        "                if {} >= {}.len() {{ continue; }}\n",
                        var_name, pool_name
                    ));

                    // 获取对象
                    match edge_data.kind {
                        EdgeKind::Move => {
                            // Move: remove(idx)
                            // 注意: remove 会改变后续元素的索引,这对 fuzzing 来说通常是可以接受的随机性,
                            // 但如果同一个 action 中有多个相同类型的参数,可能会有问题.
                            // 简单的 fuzzing 策略通常容忍这一点.
                            // 更好的策略是 swap_remove,效率更高.
                            let access = format!("{}.remove({})", pool_name, var_name);
                            args_code.push(access);
                        }
                        EdgeKind::Ref => {
                            let access = format!("&{}[{}]", pool_name, var_name);
                            args_code.push(access);
                        }
                        EdgeKind::MutRef => {
                            let access = format!("&mut {}[{}]", pool_name, var_name);
                            args_code.push(access);
                        }
                    }
                }
            }

            code.push_str(&check_code);

            // 处理 raw pointer cast
            // 注意: args_code 现在包含了 获取对象的表达式
            // 我们需要在这些表达式后面追加 .as_ptr() 或 cast
            // 但如果表达式是 remove() 的结果 (owned value),转指针需要 careful.
            // 通常 RawPtr 对应 Ref/MutRef + Cast.
            // 在 builder 中,RawPtr 被映射为 Ref/MutRef + is_raw_ptr=true.
            // 所以 args_code 中的表达式应该是 &... 或 &mut ...

            let mut final_args = Vec::new();
            for (i, expr) in args_code.iter().enumerate() {
                let (_, _, edge_data) = &input_vars[i];
                if edge_data.is_raw_ptr {
                    // Cast
                    let cast_type = match edge_data.kind {
                        EdgeKind::MutRef => "*mut _",
                        _ => "*const _",
                    };
                    final_args.push(format!("({} as {})", expr, cast_type));
                } else {
                    final_args.push(expr.clone());
                }
            }

            // 生成函数调用
            // 考虑泛型 turbo fish? "generic_map in Transition"
            // 目前简化处理,假设 rustc 能推断,或者需要在 format func_name 时处理.
            // TransitionData 有 generic_map,但 PetriNetBuilder 中我们暂时留空了.
            // 如果有,应该在这里拼装 ::<...>

            let call_expr = format!("{}({})", trans.func_name, final_args.join(", "));

            // 处理返回值
            // 查找输出边
            // 注意: 可能有正常输出 (index 0) 和 错误输出 (index 1)
            let mut outputs: Vec<_> = net
                .graph
                .edges_directed(trans_idx, Direction::Outgoing)
                .collect();
            outputs.sort_by_key(|e| e.weight().index); // 0: Ok/Return, 1: Err

            if outputs.is_empty() {
                code.push_str(&format!("                {};\n", call_expr));
            } else {
                // 检查是否是 Result (如果有 index 1 的边,或者 output type 是 Result)
                // 仅凭边无法确定是否是 Result,但如果存在 index=1 的边,说明 builder 解析出了 Result.
                // 如果只有 index=0,可能是 Result 的 T,也可能是普通返回值.
                // 实际上 builder 中: Result<T,E> -> (T, 0), (E, 1).
                // 非 Result -> (Ret, 0).

                let has_error_output = outputs.iter().any(|e| e.weight().index == 1);

                if has_error_output {
                    code.push_str(&format!("                match {} {{\n", call_expr));

                    // 处理 Ok
                    if let Some(ok_edge) = outputs.iter().find(|e| e.weight().index == 0) {
                        let target_node = net.graph.node_weight(ok_edge.target()).unwrap();
                        if let NodePayload::Place(place) = target_node {
                            if !place.is_source {
                                // 只存储复杂类型
                                let (_, sanitized) = unique_types.get(&place.type_name).unwrap(); // 应该存在
                                code.push_str(&format!(
                                    "                    Ok(res) => state.pool_{}.push(res),\n",
                                    sanitized
                                ));
                            } else {
                                code.push_str("                    Ok(_) => {},\n");
                            }
                        }
                    } else {
                        code.push_str("                    Ok(_) => {},\n");
                    }

                    // 处理 Err
                    if let Some(err_edge) = outputs.iter().find(|e| e.weight().index == 1) {
                        let target_node = net.graph.node_weight(err_edge.target()).unwrap();
                        if let NodePayload::Place(place) = target_node {
                            if !place.is_source {
                                let (_, sanitized) = unique_types.get(&place.type_name).unwrap();
                                code.push_str(&format!(
                                    "                    Err(err) => state.pool_{}.push(err),\n",
                                    sanitized
                                ));
                            } else {
                                code.push_str("                    Err(_) => {},\n");
                            }
                        }
                    } else {
                        code.push_str("                    Err(_) => {},\n");
                    }

                    code.push_str("                }\n");
                } else {
                    // 单一返回值
                    let edge = outputs.first().unwrap();
                    let target_node = net.graph.node_weight(edge.target()).unwrap();
                    if let NodePayload::Place(place) = target_node {
                        if !place.is_source {
                            let (_, sanitized) = unique_types.get(&place.type_name).unwrap();
                            code.push_str(&format!("                let res = {};\n", call_expr));
                            code.push_str(&format!(
                                "                state.pool_{}.push(res);\n",
                                sanitized
                            ));
                        } else {
                            code.push_str(&format!("                {};\n", call_expr));
                        }
                    }
                }
            }

            code.push_str("            }\n");
        }

        code.push_str(
            r#"        }
    }
});
"#,
        );
        code
    }
}
