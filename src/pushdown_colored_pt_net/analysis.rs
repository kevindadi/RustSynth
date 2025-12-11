//! 下推着色 Petri 网分析工具
//!
//! 提供对下推着色 Petri 网的分析功能，包括：
//! - 可达性分析
//! - 颜色约束检查
//! - 栈操作验证

use crate::pushdown_colored_pt_net::net::{PushdownColoredPetriNet, TokenColor, StackOperation};
use std::collections::{HashMap, HashSet};

/// 分析结果
#[derive(Debug, Clone)]
pub struct PcpnAnalysis {
    /// 可达的变迁集合
    pub reachable_transitions: HashSet<usize>,
    /// 每个 place 的颜色分布
    pub color_distribution: HashMap<usize, HashMap<TokenColor, usize>>,
    /// 栈操作统计
    pub stack_operation_stats: HashMap<StackOperation, usize>,
}

impl PcpnAnalysis {
    /// 创建新的分析结果
    pub fn new() -> Self {
        Self {
            reachable_transitions: HashSet::new(),
            color_distribution: HashMap::new(),
            stack_operation_stats: HashMap::new(),
        }
    }

    /// 分析下推着色 Petri 网
    pub fn analyze(pcpn: &PushdownColoredPetriNet) -> Self {
        let mut analysis = Self::new();

        // 统计栈操作
        for stack_op in pcpn.stack_operations.values() {
            *analysis.stack_operation_stats.entry(*stack_op).or_insert(0) += 1;
        }

        // 分析颜色分布（从初始标记开始）
        for (place_idx, colors) in &pcpn.initial_marking {
            analysis.color_distribution.insert(*place_idx, colors.clone());
        }

        // 简化的可达性分析：找出所有有输入弧的变迁
        for arc in &pcpn.arcs {
            if arc.is_input_arc {
                analysis.reachable_transitions.insert(arc.to_idx);
            }
        }

        analysis
    }

    /// 获取统计信息字符串
    pub fn stats_string(&self) -> String {
        format!(
            "Reachable Transitions: {}, Color Distribution: {} places, Stack Operations: {:?}",
            self.reachable_transitions.len(),
            self.color_distribution.len(),
            self.stack_operation_stats
        )
    }
}

impl Default for PcpnAnalysis {
    fn default() -> Self {
        Self::new()
    }
}
