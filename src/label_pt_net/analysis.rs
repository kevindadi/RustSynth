//! Petri 网分析模块
//!
//! 提供 LabeledPetriNet 的分析功能，支持 fuzz 测试用例生成：
//! - 守卫逻辑检查
//! - 可达性分析
//! - 活性检查
//! - 有界性检查
//! - API 调用序列生成

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::graph::NodeIndex;
use rand::prelude::*;

use super::{Arc, EdgeMode, LabeledPetriNet};
use crate::ir_graph::{IrGraph, NodeInfo};

/// 分析结果
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// 是否可达目标状态
    pub reachable: bool,
    /// 是否所有变迁都是活的
    pub live: bool,
    /// 是否 k-有界
    pub bounded: bool,
    /// 探索的状态数
    pub states_explored: usize,
}

/// API 调用序列
#[derive(Debug, Clone)]
pub struct ApiSequence {
    /// 变迁索引序列
    pub transition_indices: Vec<usize>,
    /// API 签名字符串序列
    pub api_calls: Vec<String>,
    /// 最终 marking
    pub final_marking: Vec<usize>,
}

impl LabeledPetriNet {
    // ========== 守卫逻辑 ==========

    /// 检查变迁的守卫条件是否满足
    ///
    /// 根据输入弧的标签检查：
    /// - MutRef：需要输入 place token == 1（独占访问）
    /// - Ref：允许多个共享引用
    /// - Move：需要至少有 weight 个 token
    /// - Implements/Require：约束检查（简化为 true）
    pub fn guard_satisfied(&self, trans_idx: usize, marking: &[usize]) -> bool {
        // 收集该变迁的所有输入弧
        let input_arcs: Vec<&Arc> = self
            .arcs
            .iter()
            .filter(|arc| arc.is_input_arc && arc.to_idx == trans_idx)
            .collect();

        // 检查是否有 MutRef 输入
        let has_mut_ref = input_arcs.iter().any(|arc| arc.label == EdgeMode::MutRef);

        for arc in &input_arcs {
            let place_idx = arc.from_idx;
            let tokens = marking.get(place_idx).copied().unwrap_or(0);

            match arc.label {
                EdgeMode::MutRef => {
                    // 可变引用需要独占访问：token 必须 >= 1
                    // 且不能有其他引用同时存在
                    if tokens < arc.weight {
                        return false;
                    }
                }
                EdgeMode::Ref => {
                    // 共享引用：需要至少有 weight 个 token
                    // 但如果同时有 MutRef，则不允许
                    if tokens < arc.weight || has_mut_ref {
                        return false;
                    }
                }
                EdgeMode::Move => {
                    // 移动语义：需要至少有 weight 个 token
                    if tokens < arc.weight {
                        return false;
                    }
                }
                EdgeMode::Implements | EdgeMode::Require => {
                    // Trait 约束：简化处理，假设满足
                    // 实际应检查类型是否实现了 Trait
                }
                _ => {
                    // 其他边类型：需要至少有 weight 个 token
                    if tokens < arc.weight {
                        return false;
                    }
                }
            }
        }

        true
    }

    /// 获取在当前 marking 下可触发的所有变迁
    pub fn enabled_transitions(&self, marking: &[usize]) -> Vec<usize> {
        (0..self.transitions.len())
            .filter(|&t| self.guard_satisfied(t, marking))
            .collect()
    }

    /// 触发变迁，返回新的 marking
    ///
    /// 注意：不检查守卫条件，调用前应先检查
    pub fn fire_transition(&self, trans_idx: usize, marking: &[usize]) -> Vec<usize> {
        let mut new_marking = marking.to_vec();

        // 处理输入弧（消耗 token）
        for arc in &self.arcs {
            if arc.is_input_arc && arc.to_idx == trans_idx {
                let place_idx = arc.from_idx;
                match arc.label {
                    EdgeMode::Ref | EdgeMode::MutRef => {
                        // 引用不消耗 token（借用语义）
                    }
                    _ => {
                        // 其他类型消耗 token
                        if new_marking[place_idx] >= arc.weight {
                            new_marking[place_idx] -= arc.weight;
                        }
                    }
                }
            }
        }

        // 处理输出弧（产生 token）
        for arc in &self.arcs {
            if !arc.is_input_arc && arc.from_idx == trans_idx {
                let place_idx = arc.to_idx;
                new_marking[place_idx] += arc.weight;
            }
        }

        new_marking
    }

    // ========== 性质分析 ==========

    /// 检查目标 marking 是否可达
    ///
    /// 使用 BFS 探索状态空间
    pub fn check_reachability(&self, target_marking: &[usize]) -> bool {
        self.check_reachability_with_limit(target_marking, 10000).0
    }

    /// 带状态限制的可达性检查
    pub fn check_reachability_with_limit(
        &self,
        target_marking: &[usize],
        max_states: usize,
    ) -> (bool, usize) {
        let mut visited: HashSet<Vec<usize>> = HashSet::new();
        let mut queue: VecDeque<Vec<usize>> = VecDeque::new();

        queue.push_back(self.initial_marking.clone());
        visited.insert(self.initial_marking.clone());

        while let Some(current) = queue.pop_front() {
            if visited.len() > max_states {
                return (false, visited.len());
            }

            if current == target_marking {
                return (true, visited.len());
            }

            for trans_idx in self.enabled_transitions(&current) {
                let new_marking = self.fire_transition(trans_idx, &current);
                if !visited.contains(&new_marking) {
                    visited.insert(new_marking.clone());
                    queue.push_back(new_marking);
                }
            }
        }

        (false, visited.len())
    }

    /// 检查活性（所有变迁是否都能在某些路径上触发）
    ///
    /// 使用 DFS 探索，记录触发过的变迁
    pub fn check_liveness(&self) -> bool {
        self.check_liveness_with_limit(10000).0
    }

    /// 带状态限制的活性检查
    pub fn check_liveness_with_limit(&self, max_states: usize) -> (bool, HashSet<usize>) {
        let mut visited: HashSet<Vec<usize>> = HashSet::new();
        let mut fired_transitions: HashSet<usize> = HashSet::new();
        let mut stack: Vec<Vec<usize>> = vec![self.initial_marking.clone()];

        while let Some(current) = stack.pop() {
            if visited.len() > max_states {
                break;
            }

            if visited.contains(&current) {
                continue;
            }
            visited.insert(current.clone());

            for trans_idx in self.enabled_transitions(&current) {
                fired_transitions.insert(trans_idx);
                let new_marking = self.fire_transition(trans_idx, &current);
                if !visited.contains(&new_marking) {
                    stack.push(new_marking);
                }
            }
        }

        let all_live = fired_transitions.len() == self.transitions.len();
        (all_live, fired_transitions)
    }

    /// 检查 k-有界性
    ///
    /// 检查所有可达 marking 中，每个 place 的 token 数是否 <= k
    pub fn check_boundedness(&self, k: usize) -> bool {
        self.check_boundedness_with_limit(k, 10000).0
    }

    /// 带状态限制的有界性检查
    pub fn check_boundedness_with_limit(&self, k: usize, max_states: usize) -> (bool, usize) {
        let mut visited: HashSet<Vec<usize>> = HashSet::new();
        let mut stack: Vec<Vec<usize>> = vec![self.initial_marking.clone()];
        let mut max_tokens = 0usize;

        while let Some(current) = stack.pop() {
            if visited.len() > max_states {
                return (max_tokens <= k, max_tokens);
            }

            if visited.contains(&current) {
                continue;
            }

            // 检查当前 marking 是否超过 k
            for &tokens in &current {
                max_tokens = max_tokens.max(tokens);
                if tokens > k {
                    return (false, max_tokens);
                }
            }

            visited.insert(current.clone());

            for trans_idx in self.enabled_transitions(&current) {
                let new_marking = self.fire_transition(trans_idx, &current);
                if !visited.contains(&new_marking) {
                    stack.push(new_marking);
                }
            }
        }

        (true, max_tokens)
    }
}

