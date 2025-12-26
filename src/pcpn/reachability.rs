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

