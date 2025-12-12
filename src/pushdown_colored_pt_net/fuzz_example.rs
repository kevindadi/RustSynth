//! 模糊测试入口点使用示例
//!
//! 展示如何在下推着色 Petri 网中使用模糊测试入口点

use crate::pushdown_colored_pt_net::net::{PushdownColoredPetriNet, TokenColor};
use crate::pushdown_colored_pt_net::analysis::PcpnAnalysis;

/// 示例：设置模糊测试入口点并添加类型转换
pub fn setup_fuzz_entries_example(pcpn: &mut PushdownColoredPetriNet) {
    // 1. 查找现有的模糊测试入口点
    let entry_points = pcpn.find_fuzz_entry_points();
    
    if entry_points.is_empty() {
        // 2. 如果没有找到，创建一个 &[u8] 类型的 place
        let fuzz_entry = pcpn.add_place("fuzz_input: &[u8]".to_string());
        
        // 3. 设置初始标记（表示有可用的模糊测试输入）
        let u8_slice_color = TokenColor::Reference {
            mutable: false,
            inner: Box::new(TokenColor::Slice(Box::new(TokenColor::Primitive("u8".to_string())))),
        };
        pcpn.set_initial_marking(fuzz_entry, u8_slice_color, 1);
        
        // 4. 添加常见类型的转换
        // 例如：从 &[u8] 生成 u32
        let u32_color = TokenColor::Primitive("u32".to_string());
        let (trans_idx, target_place) = pcpn.add_fuzz_conversion(fuzz_entry, u32_color, None);
        
        println!("创建了从 &[u8] 到 u32 的转换变迁: {}", trans_idx);
        println!("目标 place: {}", target_place);
    } else {
        println!("找到 {} 个模糊测试入口点", entry_points.len());
    }
    
    // 5. 或者使用自动添加常见类型转换的方法
    pcpn.add_common_fuzz_conversions();
}

/// 示例：分析模糊测试入口点的可达性
pub fn analyze_fuzz_reachability_example(pcpn: &PushdownColoredPetriNet) {
    let analysis = PcpnAnalysis::analyze(pcpn);
    
    println!("模糊测试入口点数量: {}", analysis.fuzz_entry_points.len());
    println!("从入口点可达的变迁数量: {}", analysis.fuzz_reachable_transitions.len());
    
    // 获取每个入口点的详细信息
    let entry_info = analysis.get_fuzz_entry_info(pcpn);
    for info in entry_info {
        println!(
            "入口点 {} ({}): 可达 {} 个变迁",
            info.place_idx, info.place_name, info.reachable_transitions
        );
    }
}

/// 示例：手动添加从 &[u8] 到特定类型的转换
pub fn add_custom_fuzz_conversion_example(pcpn: &mut PushdownColoredPetriNet) {
    // 假设已经有一个 &[u8] 类型的 place
    let fuzz_entry = pcpn.add_place("fuzz_input: &[u8]".to_string());
    
    // 添加从 &[u8] 到 Vec<u8> 的转换
    let vec_u8_color = TokenColor::Composite {
        name: "Vec".to_string(),
        type_args: vec![TokenColor::Primitive("u8".to_string())],
    };
    let (trans_idx, target_place) = pcpn.add_fuzz_conversion(fuzz_entry, vec_u8_color, None);
    
    println!("创建了从 &[u8] 到 Vec<u8> 的转换");
    println!("变迁索引: {}, 目标 place: {}", trans_idx, target_place);
    
    // 添加从 &[u8] 到 String 的转换
    let string_color = TokenColor::Composite {
        name: "String".to_string(),
        type_args: Vec::new(),
    };
    let (trans_idx2, target_place2) = pcpn.add_fuzz_conversion(fuzz_entry, string_color, None);
    
    println!("创建了从 &[u8] 到 String 的转换");
    println!("变迁索引: {}, 目标 place: {}", trans_idx2, target_place2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_entry_creation() {
        let mut pcpn = PushdownColoredPetriNet::new();
        
        // 创建模糊测试入口点
        let entry = pcpn.add_place("fuzz_input: &[u8]".to_string());
        let u8_slice_color = TokenColor::Reference {
            mutable: false,
            inner: Box::new(TokenColor::Slice(Box::new(TokenColor::Primitive("u8".to_string())))),
        };
        pcpn.set_initial_marking(entry, u8_slice_color, 1);
        
        // 查找入口点
        let entries = pcpn.find_fuzz_entry_points();
        assert!(entries.contains(&entry));
    }

    #[test]
    fn test_fuzz_conversion() {
        let mut pcpn = PushdownColoredPetriNet::new();
        
        // 创建入口点
        let entry = pcpn.add_place("fuzz_input: &[u8]".to_string());
        let u8_slice_color = TokenColor::Reference {
            mutable: false,
            inner: Box::new(TokenColor::Slice(Box::new(TokenColor::Primitive("u8".to_string())))),
        };
        pcpn.set_initial_marking(entry, u8_slice_color, 1);
        
        // 添加转换
        let u32_color = TokenColor::Primitive("u32".to_string());
        let (trans_idx, target_place) = pcpn.add_fuzz_conversion(entry, u32_color, None);
        
        // 验证转换已创建
        assert!(trans_idx < pcpn.transitions.len());
        assert!(target_place < pcpn.places.len());
        
        // 验证弧已创建
        let has_input_arc = pcpn.arcs.iter().any(|arc| {
            arc.is_input_arc && arc.from_idx == entry && arc.to_idx == trans_idx
        });
        assert!(has_input_arc);
        
        let has_output_arc = pcpn.arcs.iter().any(|arc| {
            !arc.is_input_arc && arc.from_idx == trans_idx && arc.to_idx == target_place
        });
        assert!(has_output_arc);
    }

    #[test]
    fn test_common_fuzz_conversions() {
        let mut pcpn = PushdownColoredPetriNet::new();
        
        // 自动添加常见类型转换
        pcpn.add_common_fuzz_conversions();
        
        // 验证入口点已创建
        let entries = pcpn.find_fuzz_entry_points();
        assert!(!entries.is_empty());
        
        // 验证转换变迁已创建
        let fuzz_transitions: Vec<_> = pcpn.transitions
            .iter()
            .enumerate()
            .filter(|(_, name)| name.starts_with("fuzz_parse_"))
            .collect();
        
        assert!(!fuzz_transitions.is_empty());
    }
}
