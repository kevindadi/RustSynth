//! Trait 偏序关系测试程序
//!
//! 构建 IR Graph 后，输出所有类型和泛型之间的偏序关系
//! 显示哪些类型能适配这个泛型参数，以及泛型参数来自哪个 API 或结构
//!
//! 运行: cargo run --bin test_trait_ordering -- <rustdoc_json_path>

// 注意: 需要在 main.rs 中将 ir_graph 和 parse 模块设为 pub

use std::env;
use std::collections::HashSet;
use petgraph::graph::NodeIndex;
use anyhow::Result;

// 使用库 crate 导出的模块
use rustdoc_petri_net_builder::ir_graph::builder::IrGraphBuilder;
use rustdoc_petri_net_builder::ir_graph::node_info::{GenericParamKind, NodeInfo};
use rustdoc_petri_net_builder::ir_graph::structure::{IrGraph, NodeType};
use rustdoc_petri_net_builder::ir_graph::trait_ordering::{TraitBound, TraitOrdering};
use rustdoc_petri_net_builder::parse::ParsedCrate;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // 获取命令行参数
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: {} <rustdoc_json_path>", args[0]);
        eprintln!("示例: {} target/doc/your_crate.json", args[0]);
        std::process::exit(1);
    }

    let json_path = &args[1];
    println!("正在加载 rustdoc JSON: {}", json_path);

    // 解析 JSON
    let parsed = ParsedCrate::from_json_file(json_path)
        .map_err(|e| anyhow::anyhow!("解析失败: {}", e))?;

    println!("✓ JSON 解析完成");
    // 注意: ParsedCrate 的字段结构可能不同，这里简化处理
    println!("  已加载 rustdoc JSON");

    // 构建 IR Graph
    println!("\n正在构建 IR Graph...");
    let graph = IrGraphBuilder::new(&parsed).build();
    println!("✓ IR Graph 构建完成");
    println!("  节点数: {}", graph.type_graph.node_count());
    println!("  边数: {}", graph.type_graph.edge_count());

    // 创建 Trait 偏序关系系统
    println!("\n正在构建 Trait 偏序关系系统...");
    let ordering = TraitOrdering::new(&graph);
    println!("✓ Trait 偏序关系系统构建完成");

    // 分析所有泛型参数
    println!("\n=== 开始分析类型和泛型之间的偏序关系 ===\n");

    let mut generic_count = 0;
    let mut total_satisfying_types = 0;

    // 遍历所有节点，找出泛型参数（只处理类型参数和常量参数，跳过生命周期参数）
    for (_, node_info) in &graph.node_infos {
        if let NodeInfo::Generic(generic_info) = node_info {
            // 过滤掉生命周期泛型参数，只保留类型参数和常量参数
            if generic_info.kind == GenericParamKind::Lifetime {
                continue;
            }
            
            generic_count += 1;

            // 获取泛型参数的来源信息
            let owner_info = get_owner_info(&graph, generic_info.owner);
            
            // 获取泛型参数的约束
            let bounds: Vec<TraitBound> = generic_info.bounds
                .iter()
                .filter_map(|&bound_node| {
                    get_trait_name(&graph, bound_node)
                        .map(|name| TraitBound::from(name))
                })
                .collect();

            // 查找所有满足约束的类型
            let satisfying_types = if bounds.is_empty() {
                // 无约束的泛型，返回所有类型节点
                graph.node_infos
                    .iter()
                    .filter(|(_, info)| is_type_node(info))
                    .map(|(idx, _)| *idx)
                    .collect()
            } else {
                // 有约束的泛型，找出满足所有约束的类型
                let mut candidates: Option<HashSet<petgraph::graph::NodeIndex>> = None;
                
                for bound in &bounds {
                    let types_for_bound = ordering.find_types_satisfying(bound.clone());
                    if let Some(ref mut cand) = candidates {
                        *cand = cand.intersection(&types_for_bound).cloned().collect();
                    } else {
                        candidates = Some(types_for_bound);
                    }
                }
                
                candidates.unwrap_or_default()
            };

            total_satisfying_types += satisfying_types.len();

            // 输出结果
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("泛型参数 #{}: {}", generic_count, generic_info.name);
            println!("  来源: {}", owner_info);
            
            if bounds.is_empty() {
                println!("  约束: 无约束（所有类型都可以）");
            } else {
                println!("  约束: {}", bounds.iter()
                    .map(|b| match b {
                        TraitBound::Name(name) => name.clone(),
                        TraitBound::Node(_) => "Trait".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "));
            }

            println!("  满足约束的类型数: {}", satisfying_types.len());
            
            if !satisfying_types.is_empty() {
                println!("  满足约束的类型列表:");
                let mut type_list: Vec<NodeIndex> = satisfying_types.iter().cloned().collect();
                type_list.sort();
                
                for type_idx in type_list.iter().take(20) {
                    let type_name = get_type_name(&graph, *type_idx);
                    let type_kind = get_type_kind(&graph, *type_idx);
                    println!("    - {} ({})", type_name, type_kind);
                }
                
                if satisfying_types.len() > 20 {
                    println!("    ... 还有 {} 个类型", satisfying_types.len() - 20);
                }
            } else {
                println!("  ⚠️  警告: 没有找到满足约束的类型");
            }
            
            println!();
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("\n=== 分析完成 ===");
    println!("总泛型参数数: {}", generic_count);
    println!("总满足约束的类型数: {}", total_satisfying_types);
    if generic_count > 0 {
        println!("平均每个泛型参数满足的类型数: {:.2}", 
                 total_satisfying_types as f64 / generic_count as f64);
    }

    Ok(())
}


/// 获取泛型参数所属的类型/方法信息
fn get_owner_info(graph: &IrGraph, owner: Option<NodeIndex>) -> String {
    if let Some(owner_idx) = owner {
        if let Some(node_info) = graph.node_infos.get(&owner_idx) {
            match node_info {
                NodeInfo::Struct(info) => {
                    format!("结构体: {}", info.path.full_path)
                }
                NodeInfo::Enum(info) => {
                    format!("枚举: {}", info.path.full_path)
                }
                NodeInfo::Union(info) => {
                    format!("联合体: {}", info.path.full_path)
                }
                NodeInfo::Trait(info) => {
                    format!("Trait: {}", info.path.full_path)
                }
                NodeInfo::Method(info) => {
                    if let Some(method_owner) = info.owner {
                        if let Some(owner_info) = graph.node_infos.get(&method_owner) {
                            let owner_name = owner_info.name();
                            format!("方法: {}::{}", owner_name, info.name)
                        } else {
                            format!("方法: {}", info.name)
                        }
                    } else {
                        format!("方法: {}", info.name)
                    }
                }
                NodeInfo::Function(info) => {
                    format!("函数: {}", info.path.full_path)
                }
                _ => {
                    format!("未知类型: {}", node_info.name())
                }
            }
        } else {
            "未知所有者".to_string()
        }
    } else {
        "全局作用域".to_string()
    }
}

/// 获取类型名称
fn get_type_name(graph: &IrGraph, node_idx: NodeIndex) -> String {
    if let Some(node_info) = graph.node_infos.get(&node_idx) {
        match node_info {
            NodeInfo::Struct(info) => info.path.full_path.clone(),
            NodeInfo::Enum(info) => info.path.full_path.clone(),
            NodeInfo::Union(info) => info.path.full_path.clone(),
            NodeInfo::Trait(info) => info.path.full_path.clone(),
            NodeInfo::Primitive(info) => info.name.clone(),
            NodeInfo::Generic(info) => info.name.clone(),
            NodeInfo::Method(info) => info.name.clone(),
            NodeInfo::Function(info) => info.path.full_path.clone(),
            _ => {
                // 尝试从图中获取节点名称
                graph.type_graph.node_weight(node_idx)
                    .cloned()
                    .unwrap_or_else(|| format!("Node({})", node_idx.index()))
            }
        }
    } else {
        graph.type_graph.node_weight(node_idx)
            .cloned()
            .unwrap_or_else(|| format!("Node({})", node_idx.index()))
    }
}

/// 获取类型种类
fn get_type_kind(graph: &IrGraph, node_idx: NodeIndex) -> String {
    if let Some(node_type) = graph.node_types.get(&node_idx) {
        match node_type {
            NodeType::Struct => "结构体",
            NodeType::Enum => "枚举",
            NodeType::Union => "联合体",
            NodeType::Trait => "Trait",
            NodeType::Primitive => "基本类型",
            NodeType::Generic => "泛型",
            NodeType::Function => "函数",
            NodeType::ImplMethod => "方法",
            NodeType::TraitMethod => "Trait 方法",
            _ => "其他",
        }.to_string()
    } else {
        "未知".to_string()
    }
}

/// 判断是否是类型节点（排除泛型、方法等）
fn is_type_node(node_info: &NodeInfo) -> bool {
    matches!(
        node_info,
        NodeInfo::Struct(_)
            | NodeInfo::Enum(_)
            | NodeInfo::Union(_)
            | NodeInfo::Trait(_)
            | NodeInfo::Primitive(_)
            | NodeInfo::Tuple(_)
            | NodeInfo::Slice(_)
            | NodeInfo::Array(_)
    )
}

/// 获取 Trait 名称
fn get_trait_name(graph: &IrGraph, trait_node: NodeIndex) -> Option<String> {
    if let Some(NodeInfo::Trait(trait_info)) = graph.node_infos.get(&trait_node) {
        Some(trait_info.path.name.clone())
    } else {
        // 尝试从图中获取节点名称
        graph.type_graph.node_weight(trait_node).cloned()
    }
}
