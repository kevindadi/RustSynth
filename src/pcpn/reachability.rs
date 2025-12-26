//! 可达性分析
//!
//! 实现 BFS/DFS 搜索和可达图构建

use std::collections::{HashSet, VecDeque};

use super::types::TypeId;
use super::place::PlaceId;
use super::transition::TransitionId;
use super::marking::{Token, ValueIdGen};
use super::net::PcpnNet;
use super::firing::{Config, FiringRule, EnabledBinding};
use super::witness::{Witness, WitnessStep};

/// 搜索配置
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// 最大搜索步数
    pub max_steps: usize,
    /// 最大栈深度
    pub max_stack_depth: usize,
    /// 每个库所最大 token 数
    pub max_tokens_per_place: usize,
    /// 搜索策略
    pub strategy: SearchStrategy,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            max_steps: 1000,
            max_stack_depth: 20,
            max_tokens_per_place: 10,
            strategy: SearchStrategy::BFS,
        }
    }
}

/// 搜索策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchStrategy {
    /// 广度优先搜索
    BFS,
    /// 深度优先搜索
    DFS,
    /// 最佳优先搜索（基于启发式）
    BestFirst,
    /// 最大 API 覆盖（贪婪搜索，尽可能调用更多不同的 API）
    MaxApiCoverage,
}

/// 可达性分析结果
#[derive(Debug, Clone)]
pub struct ReachabilityResult {
    /// 是否找到目标
    pub found: bool,
    /// 到达目标的 witness（API 序列）
    pub witness: Option<Witness>,
    /// 探索的状态数
    pub states_explored: usize,
    /// 生成的配置数
    pub configs_generated: usize,
    /// 搜索深度
    pub max_depth_reached: usize,
}

impl ReachabilityResult {
    pub fn not_found(states: usize, configs: usize, depth: usize) -> Self {
        ReachabilityResult {
            found: false,
            witness: None,
            states_explored: states,
            configs_generated: configs,
            max_depth_reached: depth,
        }
    }

    pub fn found(witness: Witness, states: usize, configs: usize) -> Self {
        ReachabilityResult {
            found: true,
            max_depth_reached: witness.len(),
            witness: Some(witness),
            states_explored: states,
            configs_generated: configs,
        }
    }
}

/// API 序列
#[derive(Debug, Clone)]
pub struct ApiSequence {
    /// API 调用列表
    pub calls: Vec<ApiCall>,
}

/// API 调用
#[derive(Debug, Clone)]
pub struct ApiCall {
    /// 函数路径
    pub path: String,
    /// 函数名
    pub name: String,
    /// 变迁 ID
    pub transition_id: TransitionId,
}

impl ApiSequence {
    pub fn new() -> Self {
        ApiSequence { calls: Vec::new() }
    }

    pub fn push(&mut self, call: ApiCall) {
        self.calls.push(call);
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }
}

impl Default for ApiSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// 目标条件
pub type GoalPredicate = Box<dyn Fn(&Config) -> bool>;

/// 可达性分析器
pub struct ReachabilityAnalyzer<'a> {
    net: &'a PcpnNet,
    config: SearchConfig,
}

impl<'a> ReachabilityAnalyzer<'a> {
    pub fn new(net: &'a PcpnNet, config: SearchConfig) -> Self {
        ReachabilityAnalyzer { net, config }
    }

    /// 搜索满足目标条件的配置
    pub fn search<F>(&self, initial: Config, goal: F) -> ReachabilityResult
    where
        F: Fn(&Config) -> bool,
    {
        match self.config.strategy {
            SearchStrategy::BFS => self.bfs_search(initial, goal),
            SearchStrategy::DFS => self.dfs_search(initial, goal),
            SearchStrategy::BestFirst => self.bfs_search(initial, goal), // 暂时用 BFS
            SearchStrategy::MaxApiCoverage => self.max_coverage_search(initial),
        }
    }

    /// 最大 API 覆盖搜索 - 贪婪地尝试调用尽可能多的不同 API
    pub fn max_coverage_search(&self, initial: Config) -> ReachabilityResult {
        let mut current = initial;
        let mut covered_apis: HashSet<TransitionId> = HashSet::new();
        let mut witness_steps: Vec<WitnessStep> = Vec::new();
        let mut value_gen = ValueIdGen::new();
        let firing_rule = FiringRule::new(self.net);
        
        let mut steps = 0;
        let max_steps = self.config.max_steps;
        
        // 贪婪搜索：每次选择一个未覆盖的 API
        loop {
            if steps >= max_steps {
                break;
            }
            steps += 1;
            
            // 获取所有使能的变迁
            let enabled = firing_rule.enabled_transitions(&current, &mut value_gen);
            
            if enabled.is_empty() {
                break;
            }
            
            // 优先选择未覆盖的 API 变迁
            let mut best_binding: Option<EnabledBinding> = None;
            let mut best_is_new_api = false;
            
            for binding in enabled {
                let trans_id = binding.transition_id;
                let is_api = self.net.get_transition(trans_id)
                    .map(|t| t.is_api_call())
                    .unwrap_or(false);
                
                if is_api && !covered_apis.contains(&trans_id) {
                    // 未覆盖的 API，优先选择
                    best_binding = Some(binding);
                    best_is_new_api = true;
                    break;
                } else if best_binding.is_none() {
                    // 保存一个备选（结构变迁或已覆盖的 API）
                    best_binding = Some(binding);
                }
            }
            
            // 如果没有找到任何可用变迁，尝试继续
            let Some(binding) = best_binding else {
                break;
            };
            
            let trans_id = binding.transition_id;
            let trans_name = self.net.get_transition(trans_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let is_api = self.net.get_transition(trans_id)
                .map(|t| t.is_api_call())
                .unwrap_or(false);
            
            // 触发变迁
            if firing_rule.fire(&mut current, &binding) {
                if is_api {
                    covered_apis.insert(trans_id);
                    witness_steps.push(WitnessStep {
                        transition_id: trans_id,
                        transition_name: trans_name,
                        is_api_call: true,
                    });
                }
                
                // 如果没有新的 API 可以覆盖了，提前结束
                if !best_is_new_api && is_api {
                    // 检查是否还有未覆盖的 API 可达
                    let remaining = firing_rule.enabled_transitions(&current, &mut value_gen)
                        .iter()
                        .any(|b| {
                            self.net.get_transition(b.transition_id)
                                .map(|t| t.is_api_call() && !covered_apis.contains(&b.transition_id))
                                .unwrap_or(false)
                        });
                    if !remaining {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        
        let witness = Witness { steps: witness_steps };
        ReachabilityResult::found(witness, steps, covered_apis.len())
    }

    /// 生成可达性图（DOT 格式）
    pub fn generate_reachability_graph(&self, initial: Config, max_states: usize) -> String {
        let mut dot = String::from("digraph ReachabilityGraph {\n");
        dot.push_str("    rankdir=LR;\n");
        dot.push_str("    node [shape=box, fontname=\"monospace\"];\n");
        dot.push_str("    edge [fontname=\"monospace\", fontsize=10];\n\n");
        
        let mut visited: HashSet<u64> = HashSet::new();
        let mut queue: VecDeque<(Config, usize)> = VecDeque::new();
        let mut value_gen = ValueIdGen::new();
        let firing_rule = FiringRule::new(self.net);
        
        let initial_hash = initial.state_hash();
        visited.insert(initial_hash);
        queue.push_back((initial.clone(), 0));
        
        // 添加初始状态节点
        dot.push_str(&format!("    s0 [label=\"Initial\\n{}\", style=filled, fillcolor=lightgreen];\n", 
            self.format_marking_short(&initial)));
        
        let mut state_count = 1;
        let mut edges: Vec<(usize, usize, String)> = Vec::new();
        let mut state_map: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
        state_map.insert(initial_hash, 0);
        
        while let Some((current, current_id)) = queue.pop_front() {
            if state_count >= max_states {
                break;
            }
            
            let enabled = firing_rule.enabled_transitions(&current, &mut value_gen);
            
            for binding in enabled {
                let mut new_config = current.clone();
                
                if firing_rule.fire(&mut new_config, &binding) {
                    let new_hash = new_config.state_hash();
                    
                    let target_id = if let Some(&id) = state_map.get(&new_hash) {
                        id
                    } else {
                        let new_id = state_count;
                        state_count += 1;
                        visited.insert(new_hash);
                        state_map.insert(new_hash, new_id);
                        
                        let trans = self.net.get_transition(binding.transition_id);
                        let is_api = trans.map(|t| t.is_api_call()).unwrap_or(false);
                        let color = if is_api { "lightblue" } else { "lightyellow" };
                        
                        dot.push_str(&format!("    s{} [label=\"State {}\\n{}\", style=filled, fillcolor={}];\n", 
                            new_id, new_id, self.format_marking_short(&new_config), color));
                        
                        queue.push_back((new_config, new_id));
                        new_id
                    };
                    
                    let trans_name = self.net.get_transition(binding.transition_id)
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| "?".to_string());
                    let trans = self.net.get_transition(binding.transition_id);
                    let is_api = trans.map(|t| t.is_api_call()).unwrap_or(false);
                    let edge_color = if is_api { "blue" } else { "gray" };
                    
                    edges.push((current_id, target_id, format!("{} [color={}]", trans_name, edge_color)));
                }
            }
        }
        
        // 添加边
        for (from, to, label) in edges {
            dot.push_str(&format!("    s{} -> s{} [label=\"{}\"];\n", from, to, label));
        }
        
        dot.push_str("}\n");
        dot
    }
    
    /// 格式化 marking 为简短字符串
    fn format_marking_short(&self, config: &Config) -> String {
        let mut parts = Vec::new();
        for (place_id, count) in config.marking.place_summary() {
            if count > 0 {
                if let Some(place) = self.net.get_place(place_id) {
                    let type_name = self.net.types.get(place.type_id)
                        .map(|t| t.short_name())
                        .unwrap_or_else(|| "?".to_string());
                    parts.push(format!("{}:{}", type_name, count));
                }
            }
        }
        if parts.is_empty() {
            "empty".to_string()
        } else {
            parts.join(", ")
        }
    }

    /// 搜索能够在指定库所产生指定类型 token 的路径
    pub fn search_for_token(
        &self,
        initial: Config,
        target_place: PlaceId,
        target_type: TypeId,
    ) -> ReachabilityResult {
        self.search(initial, |config| {
            config.marking.has_type(target_place, target_type)
        })
    }

    /// BFS 搜索
    fn bfs_search<F>(&self, initial: Config, goal: F) -> ReachabilityResult
    where
        F: Fn(&Config) -> bool,
    {
        // 检查初始状态
        if goal(&initial) {
            return ReachabilityResult::found(Witness::empty(), 1, 1);
        }

        let mut visited: HashSet<u64> = HashSet::new();
        let mut queue: VecDeque<SearchState> = VecDeque::new();
        let mut value_gen = ValueIdGen::new();

        // 初始状态
        let initial_hash = initial.state_hash();
        visited.insert(initial_hash);
        queue.push_back(SearchState {
            config: initial,
            depth: 0,
            parent_idx: None,
            transition: None,
            binding_snapshot: None,
        });

        // 存储所有状态用于路径重构
        let mut states: Vec<SearchState> = Vec::new();
        states.push(queue.front().unwrap().clone());

        let mut states_explored = 0;
        let mut configs_generated = 1;
        let mut max_depth = 0;

        let firing_rule = FiringRule::new(self.net);

        while let Some(current) = queue.pop_front() {
            let current_idx = states.len() - 1;
            states_explored += 1;
            max_depth = max_depth.max(current.depth);

            // 检查是否达到目标
            if goal(&current.config) {
                let witness = self.reconstruct_witness(&states, current_idx);
                return ReachabilityResult::found(witness, states_explored, configs_generated);
            }

            // 检查步数限制
            if states_explored >= self.config.max_steps {
                break;
            }

            // 获取所有使能的变迁
            let enabled = firing_rule.enabled_transitions(&current.config, &mut value_gen);

            for binding in enabled {
                // 克隆配置并触发变迁
                let mut new_config = current.config.clone();
                #[allow(unused)]
                let mut temp_gen = value_gen.clone();

                if firing_rule.fire(&mut new_config, &binding) {
                    // 检查配置有效性
                    if !new_config.is_valid(self.config.max_stack_depth) {
                        continue;
                    }

                    // 检查是否已访问
                    let new_hash = new_config.state_hash();
                    if visited.contains(&new_hash) {
                        continue;
                    }

                    visited.insert(new_hash);
                    configs_generated += 1;

                    let new_state = SearchState {
                        config: new_config,
                        depth: current.depth + 1,
                        parent_idx: Some(current_idx),
                        transition: Some(binding.transition_id),
                        binding_snapshot: Some(BindingSnapshot::from_binding(&binding)),
                    };

                    states.push(new_state.clone());
                    queue.push_back(new_state);
                }
            }
        }

        ReachabilityResult::not_found(states_explored, configs_generated, max_depth)
    }

    /// DFS 搜索
    fn dfs_search<F>(&self, initial: Config, goal: F) -> ReachabilityResult
    where
        F: Fn(&Config) -> bool,
    {
        // 使用栈代替队列
        let mut visited: HashSet<u64> = HashSet::new();
        let mut stack: Vec<SearchState> = Vec::new();
        let mut value_gen = ValueIdGen::new();

        let initial_hash = initial.state_hash();
        visited.insert(initial_hash);
        stack.push(SearchState {
            config: initial,
            depth: 0,
            parent_idx: None,
            transition: None,
            binding_snapshot: None,
        });

        let mut states: Vec<SearchState> = Vec::new();
        let mut states_explored = 0;
        let mut configs_generated = 1;
        let mut max_depth = 0;

        let firing_rule = FiringRule::new(self.net);

        while let Some(current) = stack.pop() {
            let current_idx = states.len();
            states.push(current.clone());
            states_explored += 1;
            max_depth = max_depth.max(current.depth);

            if goal(&current.config) {
                let witness = self.reconstruct_witness(&states, current_idx);
                return ReachabilityResult::found(witness, states_explored, configs_generated);
            }

            if states_explored >= self.config.max_steps {
                break;
            }

            let enabled = firing_rule.enabled_transitions(&current.config, &mut value_gen);

            for binding in enabled.into_iter().rev() {
                let mut new_config = current.config.clone();

                if firing_rule.fire(&mut new_config, &binding) {
                    if !new_config.is_valid(self.config.max_stack_depth) {
                        continue;
                    }

                    let new_hash = new_config.state_hash();
                    if visited.contains(&new_hash) {
                        continue;
                    }

                    visited.insert(new_hash);
                    configs_generated += 1;

                    stack.push(SearchState {
                        config: new_config,
                        depth: current.depth + 1,
                        parent_idx: Some(current_idx),
                        transition: Some(binding.transition_id),
                        binding_snapshot: Some(BindingSnapshot::from_binding(&binding)),
                    });
                }
            }
        }

        ReachabilityResult::not_found(states_explored, configs_generated, max_depth)
    }

    /// 从状态序列重构 witness
    fn reconstruct_witness(&self, states: &[SearchState], final_idx: usize) -> Witness {
        let mut steps = Vec::new();
        let mut current_idx = Some(final_idx);

        while let Some(idx) = current_idx {
            if idx >= states.len() {
                break;
            }

            let state = &states[idx];

            if let Some(trans_id) = state.transition {
                let trans_name = self.net.get_transition(trans_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("t{}", trans_id.0));

                steps.push(WitnessStep {
                    transition_id: trans_id,
                    transition_name: trans_name,
                    is_api_call: self.net.get_transition(trans_id)
                        .map(|t| t.is_api_call())
                        .unwrap_or(false),
                });
            }

            current_idx = state.parent_idx;
        }

        steps.reverse();
        Witness { steps }
    }

    /// 探索所有可达配置（直到限制）
    pub fn explore_all(&self, initial: Config) -> Vec<Config> {
        let mut visited: HashSet<u64> = HashSet::new();
        let mut queue: VecDeque<Config> = VecDeque::new();
        let mut results = Vec::new();
        let mut value_gen = ValueIdGen::new();

        let initial_hash = initial.state_hash();
        visited.insert(initial_hash);
        queue.push_back(initial.clone());
        results.push(initial);

        let firing_rule = FiringRule::new(self.net);
        let mut steps = 0;

        while let Some(current) = queue.pop_front() {
            if steps >= self.config.max_steps {
                break;
            }
            steps += 1;

            let enabled = firing_rule.enabled_transitions(&current, &mut value_gen);

            for binding in enabled {
                let mut new_config = current.clone();

                if firing_rule.fire(&mut new_config, &binding) {
                    let new_hash = new_config.state_hash();
                    if !visited.contains(&new_hash) {
                        visited.insert(new_hash);
                        queue.push_back(new_config.clone());
                        results.push(new_config);
                    }
                }
            }
        }

        results
    }
}

/// 搜索状态
#[derive(Debug, Clone)]
struct SearchState {
    config: Config,
    depth: usize,
    parent_idx: Option<usize>,
    transition: Option<TransitionId>,
    binding_snapshot: Option<BindingSnapshot>,
}

/// 绑定快照（用于路径重构）
#[derive(Debug, Clone)]
struct BindingSnapshot {
    input_types: Vec<TypeId>,
    output_types: Vec<TypeId>,
}

impl BindingSnapshot {
    fn from_binding(binding: &EnabledBinding) -> Self {
        BindingSnapshot {
            input_types: binding.input_bindings
                .values()
                .flat_map(|tokens: &Vec<Token>| tokens.iter().map(|t| t.type_id))
                .collect(),
            output_types: binding.output_tokens
                .values()
                .flat_map(|tokens: &Vec<Token>| tokens.iter().map(|t| t.type_id))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_config_default() {
        let config = SearchConfig::default();
        assert_eq!(config.max_steps, 1000);
        assert_eq!(config.strategy, SearchStrategy::BFS);
    }

    #[test]
    fn test_api_sequence() {
        let mut seq = ApiSequence::new();
        assert!(seq.is_empty());

        seq.push(ApiCall {
            path: "test::foo".to_string(),
            name: "foo".to_string(),
            transition_id: TransitionId::new(0),
        });

        assert_eq!(seq.len(), 1);
    }
}

