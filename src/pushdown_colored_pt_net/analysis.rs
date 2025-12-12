//! 下推着色 Petri 网分析工具
//!
//! 提供对下推着色 Petri 网的分析功能，包括：
//! - 可达性分析
//! - 颜色约束检查
//! - 栈操作验证

use crate::pushdown_colored_pt_net::net::{PushdownColoredPetriNet, TokenColor, StackOperation};
use std::collections::{HashMap, HashSet, VecDeque};

/// 分析结果
#[derive(Debug, Clone)]
pub struct PcpnAnalysis {
    /// 可达的变迁集合
    pub reachable_transitions: HashSet<usize>,
    /// 每个 place 的颜色分布
    pub color_distribution: HashMap<usize, HashMap<TokenColor, usize>>,
    /// 栈操作统计
    pub stack_operation_stats: HashMap<StackOperation, usize>,
    /// 模糊测试入口点
    pub fuzz_entry_points: Vec<usize>,
    /// 从模糊测试入口点可达的变迁
    pub fuzz_reachable_transitions: HashSet<usize>,
}

impl PcpnAnalysis {
    /// 创建新的分析结果
    pub fn new() -> Self {
        Self {
            reachable_transitions: HashSet::new(),
            color_distribution: HashMap::new(),
            stack_operation_stats: HashMap::new(),
            fuzz_entry_points: Vec::new(),
            fuzz_reachable_transitions: HashSet::new(),
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

        // 查找模糊测试入口点
        analysis.fuzz_entry_points = pcpn.find_fuzz_entry_points();
        
        // 分析从模糊测试入口点可达的变迁
        analysis.analyze_fuzz_reachability(pcpn);

        analysis
    }

    /// 分析从模糊测试入口点可达的变迁
    fn analyze_fuzz_reachability(&mut self, pcpn: &PushdownColoredPetriNet) {
        // 使用 BFS 从模糊测试入口点开始探索
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut visited_places: HashSet<usize> = HashSet::new();
        
        // 初始化：将所有模糊测试入口点加入队列
        for &entry_idx in &self.fuzz_entry_points {
            visited_places.insert(entry_idx);
            queue.push_back(entry_idx);
        }

        // BFS 探索
        while let Some(place_idx) = queue.pop_front() {
            // 找出所有从这个 place 出发的弧（输入弧，place -> transition）
            for arc in &pcpn.arcs {
                if arc.is_input_arc && arc.from_idx == place_idx {
                    let trans_idx = arc.to_idx;
                    self.fuzz_reachable_transitions.insert(trans_idx);
                    
                    // 找出这个变迁的所有输出弧（transition -> place）
                    for output_arc in &pcpn.arcs {
                        if !output_arc.is_input_arc && output_arc.from_idx == trans_idx {
                            let target_place = output_arc.to_idx;
                            if !visited_places.contains(&target_place) {
                                visited_places.insert(target_place);
                                queue.push_back(target_place);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn stats_string(&self) -> String {
        format!(
            "Reachable Transitions: {}, Color Distribution: {} places, Stack Operations: {:?}, \
             Fuzz Entry Points: {}, Fuzz Reachable Transitions: {}",
            self.reachable_transitions.len(),
            self.color_distribution.len(),
            self.stack_operation_stats,
            self.fuzz_entry_points.len(),
            self.fuzz_reachable_transitions.len()
        )
    }

    /// 获取模糊测试入口点的详细信息
    pub fn get_fuzz_entry_info(&self, pcpn: &PushdownColoredPetriNet) -> Vec<FuzzEntryInfo> {
        self.fuzz_entry_points
            .iter()
            .map(|&idx| {
                let place_name = pcpn.places.get(idx).cloned().unwrap_or_default();
                let reachable_count = self
                    .fuzz_reachable_transitions
                    .iter()
                    .filter(|&&trans_idx| {
                        // 检查这个变迁是否直接从入口点可达
                        pcpn.arcs.iter().any(|arc| {
                            arc.is_input_arc && arc.from_idx == idx && arc.to_idx == trans_idx
                        })
                    })
                    .count();
                
                FuzzEntryInfo {
                    place_idx: idx,
                    place_name,
                    reachable_transitions: reachable_count,
                }
            })
            .collect()
    }
}

impl Default for PcpnAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

/// 模糊测试入口点信息
#[derive(Debug, Clone)]
pub struct FuzzEntryInfo {
    /// Place 索引
    pub place_idx: usize,
    /// Place 名称
    pub place_name: String,
    /// 从该入口点直接可达的变迁数量
    pub reachable_transitions: usize,
}
