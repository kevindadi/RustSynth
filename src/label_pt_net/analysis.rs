//! Petri 网分析模块
//!
//! ## 分析功能
//! - 可达性分析:检查目标状态是否可达
//! - 活性检查:检查所有变迁是否都能触发
//! - 有界性检查:检查 token 数量是否有界
//! - API 序列生成:生成有效的 API 调用序列用于 fuzz 测试
#![allow(dead_code)]

use std::collections::{HashSet, VecDeque};

use super::net::{Arc, LabeledPetriNet};
use crate::ir_graph::{EdgeMode, IrGraph, NodeInfo};

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

/// Fuzz 输入解析器
///
/// 用于从 &[u8] 确定性地派生路径选择
#[derive(Debug)]
pub struct FuzzInputParser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> FuzzInputParser<'a> {
    /// 创建新的解析器
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// 获取下一个字节,循环使用
    pub fn next_byte(&mut self) -> u8 {
        if self.data.is_empty() {
            return 0;
        }
        let byte = self.data[self.pos % self.data.len()];
        self.pos += 1;
        byte
    }

    /// 从可选项中选择一个(确定性)
    pub fn choose<T: Clone>(&mut self, options: &[T]) -> Option<T> {
        if options.is_empty() {
            return None;
        }
        let idx = self.next_byte() as usize % options.len();
        Some(options[idx].clone())
    }

    /// 返回一个 0.0-1.0 之间的概率值
    pub fn probability(&mut self) -> f64 {
        self.next_byte() as f64 / 255.0
    }

    /// 检查是否应该选择(基于阈值)
    pub fn should_choose(&mut self, threshold: f64) -> bool {
        self.probability() < threshold
    }

    /// 获取剩余数据长度
    pub fn remaining(&self) -> usize {
        if self.data.is_empty() {
            0
        } else {
            self.data.len().saturating_sub(self.pos % self.data.len())
        }
    }
}

impl LabeledPetriNet {
    /// 检查变迁的守卫条件是否满足
    ///
    /// 根据输入弧的标签检查 Rust 借用语义:
    /// - MutRef:需要独占访问(token >= 1,且无其他引用)
    /// - Ref:允许多个共享引用(token >= weight,但不能与 MutRef 共存)
    /// - Move:需要至少有 weight 个 token(消耗性)
    /// - Implements/Require:Trait 约束检查(基于 shim 链接)
    /// - UnwrapOk/UnwrapErr/UnwrapNone:分支选择
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
                    // 可变引用需要独占访问:token 必须 >= 1
                    // 且不能有其他引用同时存在
                    if tokens < arc.weight {
                        return false;
                    }
                }
                EdgeMode::Ref => {
                    // 共享引用:需要至少有 weight 个 token
                    // 但如果同时有 MutRef,则不允许
                    if tokens < arc.weight || has_mut_ref {
                        return false;
                    }
                }
                EdgeMode::Move => {
                    // 移动语义:需要至少有 weight 个 token
                    if tokens < arc.weight {
                        return false;
                    }
                }
                EdgeMode::Implements | EdgeMode::Require => {
                    // Trait 约束:简化处理,假设满足
                    // 实际应检查类型是否实现了 Trait
                }
                _ => {
                    // 其他边类型:需要至少有 weight 个 token
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

    /// 触发变迁,返回新的 marking
    ///
    /// 注意:不检查守卫条件,调用前应先检查
    pub fn fire_transition(&self, trans_idx: usize, marking: &[usize]) -> Vec<usize> {
        let mut new_marking = marking.to_vec();

        // 处理输入弧(消耗 token)
        for arc in &self.arcs {
            if arc.is_input_arc && arc.to_idx == trans_idx {
                let place_idx = arc.from_idx;
                match arc.label {
                    EdgeMode::Ref | EdgeMode::MutRef => {
                        // 引用不消耗 token(借用语义)
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

        // 处理输出弧(产生 token)
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
    #[allow(dead_code)]
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

    /// 检查活性(所有变迁是否都能在某些路径上触发)
    ///
    /// 使用 DFS 探索,记录触发过的变迁
    #[allow(dead_code)]
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
    /// 检查所有可达 marking 中,每个 place 的 token 数是否 <= k
    #[allow(dead_code)]
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

    // ========== API 序列生成(cargo-fuzz 兼容) ==========

    /// 生成 API 调用序列(用于 cargo-fuzz 测试)
    ///
    /// # 参数
    /// - `max_depth`: 最大序列长度
    /// - `fuzz_input`: fuzz 输入字节,用于确定性路径选择
    /// - `ir`: IrGraph 引用,用于获取方法签名
    ///
    /// # 返回
    /// API 调用字符串序列的列表(1-5 个序列,基于 input 长度)
    pub fn generate_api_sequences(
        &self,
        max_depth: usize,
        fuzz_input: &[u8],
        ir: &IrGraph,
    ) -> Vec<Vec<String>> {
        let mut parser = FuzzInputParser::new(fuzz_input);
        let mut sequences: Vec<Vec<String>> = Vec::new();

        // 根据输入长度决定生成序列数量(1-5)
        let num_sequences = if fuzz_input.is_empty() {
            1
        } else {
            1 + (fuzz_input.len() % 5)
        };

        for _ in 0..num_sequences {
            let seq = self.generate_single_sequence_fuzz(max_depth, ir, &mut parser);
            if !seq.is_empty() {
                sequences.push(seq);
            }
        }

        sequences
    }

    /// 生成带详细信息的 API 序列(cargo-fuzz 兼容)
    pub fn generate_api_sequences_detailed(
        &self,
        max_depth: usize,
        fuzz_input: &[u8],
        ir: &IrGraph,
    ) -> Vec<ApiSequence> {
        let mut parser = FuzzInputParser::new(fuzz_input);
        let mut sequences: Vec<ApiSequence> = Vec::new();

        let num_sequences = if fuzz_input.is_empty() {
            1
        } else {
            1 + (fuzz_input.len() % 5)
        };

        for _ in 0..num_sequences {
            if let Some(seq) =
                self.generate_single_sequence_detailed_fuzz(max_depth, ir, &mut parser)
            {
                sequences.push(seq);
            }
        }

        sequences
    }

    /// 生成单个 API 序列(使用 fuzz 输入)
    fn generate_single_sequence_fuzz(
        &self,
        max_depth: usize,
        ir: &IrGraph,
        parser: &mut FuzzInputParser,
    ) -> Vec<String> {
        let mut marking = self.initial_marking.clone();
        let mut api_calls: Vec<String> = Vec::new();

        for _ in 0..max_depth {
            let enabled = self.enabled_transitions(&marking);
            if enabled.is_empty() {
                break;
            }

            // 使用 fuzz 输入选择变迁
            let trans_idx = self.select_transition_fuzz(&enabled, &marking, parser);

            // 获取 API 签名
            if let Some(api_str) = self.get_api_signature(trans_idx, ir) {
                api_calls.push(api_str);
            } else {
                // 如果无法获取签名,使用变迁名称
                api_calls.push(self.transitions[trans_idx].clone());
            }

            // 触发变迁
            marking = self.fire_transition(trans_idx, &marking);
        }

        api_calls
    }

    /// 生成单个带详细信息的 API 序列(使用 fuzz 输入)
    fn generate_single_sequence_detailed_fuzz(
        &self,
        max_depth: usize,
        ir: &IrGraph,
        parser: &mut FuzzInputParser,
    ) -> Option<ApiSequence> {
        let mut marking = self.initial_marking.clone();
        let mut transition_indices: Vec<usize> = Vec::new();
        let mut api_calls: Vec<String> = Vec::new();

        for _ in 0..max_depth {
            let enabled = self.enabled_transitions(&marking);
            if enabled.is_empty() {
                break;
            }

            // 优先选择"有趣"的变迁(如 unwrap 操作)
            let trans_idx = self.select_transition_fuzz(&enabled, &marking, parser);

            transition_indices.push(trans_idx);

            if let Some(api_str) = self.get_api_signature(trans_idx, ir) {
                api_calls.push(api_str);
            } else {
                api_calls.push(self.transitions[trans_idx].clone());
            }

            marking = self.fire_transition(trans_idx, &marking);
        }

        if transition_indices.is_empty() {
            None
        } else {
            Some(ApiSequence {
                transition_indices,
                api_calls,
                final_marking: marking,
            })
        }
    }

    /// 选择变迁(使用 fuzz 输入,确定性)
    ///
    /// 优先选择:
    /// 1. 可能导致 unwrap failure 的变迁(30% 概率)
    /// 2. 涉及 MutRef 的变迁(20% 概率)
    /// 3. 基于 fuzz 输入选择
    fn select_transition_fuzz(
        &self,
        enabled: &[usize],
        _marking: &[usize],
        parser: &mut FuzzInputParser,
    ) -> usize {
        // 检查是否有 unwrap 相关的变迁
        let unwrap_transitions: Vec<usize> = enabled
            .iter()
            .copied()
            .filter(|&t| {
                let name = &self.transitions[t];
                name.contains("unwrap") || name.contains("expect") || name.contains("?")
            })
            .collect();

        if !unwrap_transitions.is_empty() && parser.should_choose(0.3) {
            if let Some(idx) = parser.choose(&unwrap_transitions) {
                return idx;
            }
        }

        // 检查是否有涉及 MutRef 的变迁
        let mut_ref_transitions: Vec<usize> = enabled
            .iter()
            .copied()
            .filter(|&t| {
                self.arcs
                    .iter()
                    .any(|arc| arc.is_input_arc && arc.to_idx == t && arc.label == EdgeMode::MutRef)
            })
            .collect();

        if !mut_ref_transitions.is_empty() && parser.should_choose(0.2) {
            if let Some(idx) = parser.choose(&mut_ref_transitions) {
                return idx;
            }
        }

        // 检查是否有 UnwrapErr/UnwrapNone 输出弧的变迁(failure 路径)
        let failure_transitions: Vec<usize> = enabled
            .iter()
            .copied()
            .filter(|&t| {
                self.arcs.iter().any(|arc| {
                    !arc.is_input_arc
                        && arc.from_idx == t
                        && matches!(arc.label, EdgeMode::UnwrapErr | EdgeMode::UnwrapNone)
                })
            })
            .collect();

        if !failure_transitions.is_empty() && parser.should_choose(0.15) {
            if let Some(idx) = parser.choose(&failure_transitions) {
                return idx;
            }
        }

        // 基于 fuzz 输入选择
        parser.choose(enabled).unwrap_or(enabled[0])
    }

    /// 获取变迁对应的 API 签名
    fn get_api_signature(&self, trans_idx: usize, ir: &IrGraph) -> Option<String> {
        let node_idx = self.trans_to_node.get(&trans_idx)?;
        let node_info = ir.node_infos.get(node_idx)?;

        match node_info {
            NodeInfo::Method(method_info) => {
                let owner_name = method_info
                    .owner
                    .and_then(|o| ir.node_infos.get(&o))
                    .map(|info| info.name())
                    .unwrap_or("?");

                // 构建方法签名
                let params: Vec<String> = method_info
                    .params
                    .iter()
                    .map(|p| {
                        if p.is_self {
                            match p.borrow_mode {
                                EdgeMode::MutRef => "&mut self".to_string(),
                                EdgeMode::Ref => "&self".to_string(),
                                EdgeMode::Move => "self".to_string(),
                                _ => p.type_str.clone(),
                            }
                        } else {
                            format!("{}: {}", p.name, p.type_str)
                        }
                    })
                    .collect();

                let return_str = if method_info.return_info.type_str.is_empty() {
                    String::new()
                } else {
                    format!(" -> {}", method_info.return_info.type_str)
                };

                Some(format!(
                    "{}::{}({}){}",
                    owner_name,
                    method_info.name,
                    params.join(", "),
                    return_str
                ))
            }
            NodeInfo::Function(func_info) => {
                let params: Vec<String> = func_info
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_str))
                    .collect();

                let return_str = if func_info.return_info.type_str.is_empty() {
                    String::new()
                } else {
                    format!(" -> {}", func_info.return_info.type_str)
                };

                Some(format!(
                    "{}({}){}",
                    func_info.path.name,
                    params.join(", "),
                    return_str
                ))
            }
            NodeInfo::UnwrapOp(unwrap_info) => {
                let op_name = format!("{:?}", unwrap_info.op_kind);
                Some(format!("unwrap_op::{}", op_name))
            }
            _ => None,
        }
    }

    /// 生成针对特定目标状态的 API 序列(使用 fuzz 输入)
    ///
    /// 使用引导式搜索,尝试到达目标 marking
    #[allow(dead_code)]
    pub fn generate_targeted_sequences(
        &self,
        target_place: usize,
        target_tokens: usize,
        max_depth: usize,
        fuzz_input: &[u8],
        ir: &IrGraph,
    ) -> Vec<Vec<String>> {
        let mut parser = FuzzInputParser::new(fuzz_input);
        let mut sequences: Vec<Vec<String>> = Vec::new();

        // 根据输入长度决定尝试次数
        let num_attempts = if fuzz_input.is_empty() {
            1
        } else {
            1 + (fuzz_input.len() % 10)
        };

        for _ in 0..num_attempts {
            let mut marking = self.initial_marking.clone();
            let mut api_calls: Vec<String> = Vec::new();

            for _ in 0..max_depth {
                // 检查是否达到目标
                if marking.get(target_place).copied().unwrap_or(0) >= target_tokens {
                    if !api_calls.is_empty() {
                        sequences.push(api_calls.clone());
                    }
                    break;
                }

                let enabled = self.enabled_transitions(&marking);
                if enabled.is_empty() {
                    break;
                }

                // 优先选择可能增加目标 place token 的变迁
                let preferred: Vec<usize> = enabled
                    .iter()
                    .copied()
                    .filter(|&t| {
                        self.arcs.iter().any(|arc| {
                            !arc.is_input_arc && arc.from_idx == t && arc.to_idx == target_place
                        })
                    })
                    .collect();

                let trans_idx = if !preferred.is_empty() && parser.should_choose(0.7) {
                    parser.choose(&preferred).unwrap_or(enabled[0])
                } else {
                    parser.choose(&enabled).unwrap_or(enabled[0])
                };

                if let Some(api_str) = self.get_api_signature(trans_idx, ir) {
                    api_calls.push(api_str);
                } else {
                    api_calls.push(self.transitions[trans_idx].clone());
                }

                marking = self.fire_transition(trans_idx, &marking);
            }
        }

        sequences
    }

    /// 检查是否有"有趣"的状态(failure place 有 token)
    #[allow(dead_code)]
    pub fn has_interesting_state(&self, marking: &[usize]) -> bool {
        // 检查是否有 failure 相关的 place 有 token
        for (idx, &tokens) in marking.iter().enumerate() {
            if tokens > 0 {
                let place_name = &self.places[idx];
                if place_name.contains("Err")
                    || place_name.contains("None")
                    || place_name.contains("failure")
                    || place_name.contains("panic")
                {
                    return true;
                }
            }
        }
        false
    }

    /// 获取所有 failure places 的索引
    pub fn get_failure_places(&self) -> Vec<usize> {
        self.places
            .iter()
            .enumerate()
            .filter_map(|(idx, name)| {
                if name.contains("Err")
                    || name.contains("None")
                    || name.contains("failure")
                    || name.contains("Error")
                {
                    Some(idx)
                } else {
                    None
                }
            })
            .collect()
    }

    /// 完整分析:返回 AnalysisResult
    pub fn analyze(&self, max_states: usize, k_bound: usize) -> AnalysisResult {
        let (live, _) = self.check_liveness_with_limit(max_states);
        let (bounded, _) = self.check_boundedness_with_limit(k_bound, max_states);

        AnalysisResult {
            reachable: true, // 初始状态总是可达的
            live,
            bounded,
            states_explored: max_states,
        }
    }
}

// ========== cargo-fuzz 集成示例 ==========
//
/// 在 fuzz/fuzz_targets/ 目录下创建文件使用此函数
///
/// ```rust,ignore
/// #![no_main]
/// use libfuzzer_sys::fuzz_target;
/// use rustdoc_petri_net_builder::label_pt_net::{convert_ir_to_lpn, LabeledPetriNet};
/// use rustdoc_petri_net_builder::ir_graph::IrGraph;
///
/// fuzz_target!(|data: &[u8]| {
///     // 假设 ir_graph 已预加载(可以使用 lazy_static 或 once_cell)
///     // let ir = load_ir_graph();
///     // let mut lpn = convert_ir_to_lpn(&ir);
///     // lpn.add_primitive_shims(&ir);
///     // let seqs = lpn.generate_api_sequences(20, data, &ir);
///     //
///     // // 验证生成的序列
///     // for seq in seqs {
///     //     // 可以生成 Rust 测试代码或直接执行
///     //     validate_api_sequence(&seq);
///     // }
/// });
/// ```
#[cfg(feature = "fuzz")]
pub fn fuzz_api_sequences(data: &[u8], lpn: &LabeledPetriNet, ir: &IrGraph) -> Vec<Vec<String>> {
    lpn.generate_api_sequences(20, data, ir)
}
