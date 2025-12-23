//! 状态空间搜索
//!
//! 实现 BFS 搜索和 witness 重构

use std::collections::{VecDeque, HashSet};
use crate::pushdown_colored_pt_net::runtime::*;
use crate::pushdown_colored_pt_net::env::Env;

/// 搜索配置
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// 每个库所的最大 token 数量
    pub max_tokens_per_place: usize,
    /// 最大栈深度
    pub max_stack_depth: usize,
    /// 最大搜索步数
    pub max_steps: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            max_tokens_per_place: 10,
            max_stack_depth: 20,
            max_steps: 1000,
        }
    }
}

/// 搜索状态
#[derive(Debug, Clone)]
struct SearchState {
    /// 配置
    config: Config,
    /// 从初始状态到当前状态的步数
    steps: usize,
    /// 父状态索引 (用于重构路径)
    parent_index: Option<usize>,
    /// 触发此状态的变迁 ID
    transition_id: Option<TransitionId>,
    /// 触发此状态的选择
    choice: Option<Choice>,
}

/// 搜索结果
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// 是否找到目标状态
    pub found: bool,
    /// 到达目标状态的路径 (变迁序列)
    pub witness: Vec<WitnessStep>,
    /// 访问的状态数量
    pub states_explored: usize,
    /// 生成的配置数量
    pub configs_generated: usize,
}

/// Witness 步骤
#[derive(Debug, Clone)]
pub struct WitnessStep {
    /// 变迁 ID
    pub transition_id: TransitionId,
    /// 变迁名称
    pub transition_name: String,
    /// 使用的选择
    pub choice: Choice,
}

/// BFS 搜索器
pub struct BFSSearcher<'a> {
    /// 变迁列表
    transitions: Vec<&'a dyn Transition>,
    /// 环境
    env: &'a dyn Env,
    /// 搜索配置
    search_config: SearchConfig,
    /// 值 ID 生成器
    value_gen: ValueIdGenerator,
}

impl<'a> BFSSearcher<'a> {
    /// 创建新的 BFS 搜索器
    pub fn new(
        transitions: Vec<&'a dyn Transition>,
        env: &'a dyn Env,
        search_config: SearchConfig,
    ) -> Self {
        BFSSearcher {
            transitions,
            env,
            search_config,
            value_gen: ValueIdGenerator::new(),
        }
    }

    /// 执行 BFS 搜索
    /// 
    /// 参数:
    /// - initial: 初始配置
    /// - goal: 目标配置检查函数 (返回 true 表示达到目标)
    /// 
    /// 返回:
    /// - SearchResult: 搜索结果
    pub fn search<F>(&mut self, initial: Config, goal: F) -> SearchResult
    where
        F: Fn(&Config) -> bool,
    {
        // 检查初始配置是否已经是目标
        if goal(&initial) {
            return SearchResult {
                found: true,
                witness: Vec::new(),
                states_explored: 1,
                configs_generated: 1,
            };
        }

        // 使用哈希集合存储已访问的配置 (用于去重)
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut states = Vec::new();

        // 添加初始状态
        let state_hash = self.config_hash(&initial);
        visited.insert(state_hash.clone());
        let initial_state = SearchState {
            config: initial,
            steps: 0,
            parent_index: None,
            transition_id: None,
            choice: None,
        };
        let initial_index = states.len();
        states.push(initial_state);
        queue.push_back(initial_index);

        let mut states_explored = 0;
        let mut configs_generated = 1;

        while let Some(state_index) = queue.pop_front() {
            states_explored += 1;

            if states_explored > self.search_config.max_steps {
                break;
            }

            let state = states[state_index].clone();

            // 检查是否达到目标
            if goal(&state.config) {
                return SearchResult {
                    found: true,
                    witness: self.reconstruct_witness(&states, state_index),
                    states_explored,
                    configs_generated,
                };
            }

            // 尝试所有变迁
            for transition in &self.transitions {
                // 创建临时值 ID 生成器用于检查
                let mut temp_value_gen = self.value_gen.clone();

                // 检查变迁是否可触发
                if let Some(choice) = transition.is_enabled(&state.config, self.env, &mut temp_value_gen) {
                    // 创建新配置
                    let mut new_config = state.config.clone();
                    
                    // 触发变迁
                    if transition.fire(&mut new_config, &choice, self.env, &mut temp_value_gen) {
                        // 更新值 ID 生成器
                        self.value_gen = temp_value_gen;

                        // 检查新配置是否有效
                        if !self.is_valid_config(&new_config) {
                            continue;
                        }

                        // 检查是否已访问
                        let new_hash = self.config_hash(&new_config);
                        if visited.contains(&new_hash) {
                            continue;
                        }

                        // 检查步数限制
                        if state.steps + 1 > self.search_config.max_steps {
                            continue;
                        }

                        // 添加新状态
                        visited.insert(new_hash);
                        configs_generated += 1;
                        let new_state = SearchState {
                            config: new_config,
                            steps: state.steps + 1,
                            parent_index: Some(state_index),
                            transition_id: Some(transition.id()),
                            choice: Some(choice.clone()),
                        };
                        let new_index = states.len();
                        states.push(new_state);
                        queue.push_back(new_index);
                    }
                }
            }
        }

        SearchResult {
            found: false,
            witness: Vec::new(),
            states_explored,
            configs_generated,
        }
    }

    /// 检查配置是否有效
    fn is_valid_config(&self, config: &Config) -> bool {
        // 检查栈深度
        if config.stack.depth() > self.search_config.max_stack_depth {
            return false;
        }

        // 检查每个库所的 token 数量
        // 这里需要访问所有库所,简化实现
        // 实际应该遍历所有库所
        true
    }

    /// 计算配置的哈希值 (用于去重)
    fn config_hash(&self, config: &Config) -> ConfigHash {
        // 简化实现: 使用配置的字符串表示
        // 实际应该使用更高效的哈希方法
        ConfigHash {
            // 这里简化处理
            // 实际应该序列化 marking 和 stack
            marking: format!("{:?}", config.marking),
            stack: format!("{:?}", config.stack),
        }
    }

    /// 重构 witness 路径
    fn reconstruct_witness(&self, states: &[SearchState], final_index: usize) -> Vec<WitnessStep> {
        let mut witness = Vec::new();
        let mut current_index = Some(final_index);
        let states_len = states.len();

        // 从目标状态回溯到初始状态
        while let Some(idx) = current_index {
            if idx >= states_len {
                break;
            }
            let state = &states[idx];
            if let (Some(trans_id), Some(choice)) = (state.transition_id, &state.choice) {
                // 查找变迁名称
                let trans_name = self.transitions
                    .iter()
                    .find(|t| t.id() == trans_id)
                    .map(|t| t.name().to_string())
                    .unwrap_or_else(|| format!("T{}", trans_id.0));

                witness.push(WitnessStep {
                    transition_id: trans_id,
                    transition_name: trans_name,
                    choice: choice.clone(),
                });
            }
            current_index = state.parent_index;
        }

        // 反转路径 (从初始状态到目标状态)
        witness.reverse();
        witness
    }
}

/// 配置哈希 (用于去重)
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ConfigHash {
    marking: String,
    stack: String,
}

/// 辅助函数: 搜索直到找到满足条件的配置
pub fn search_until<F>(
    transitions: Vec<&dyn Transition>,
    env: &dyn Env,
    initial: Config,
    goal: F,
    search_config: SearchConfig,
) -> SearchResult
where
    F: Fn(&Config) -> bool,
{
    let mut searcher = BFSSearcher::new(transitions, env, search_config);
    searcher.search(initial, goal)
}

/// 辅助函数: 搜索所有可达配置 (直到达到限制)
pub fn explore_all(
    transitions: Vec<&dyn Transition>,
    env: &dyn Env,
    initial: Config,
    search_config: SearchConfig,
) -> Vec<Config> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut results = Vec::new();
    let mut value_gen = ValueIdGenerator::new();

    let initial_hash = format!("{:?}", initial);
    visited.insert(initial_hash);
    queue.push_back(initial.clone());
    results.push(initial.clone());

    let mut steps = 0;

    while let Some(config) = queue.pop_front() {
        if steps >= search_config.max_steps {
            break;
        }
        steps += 1;

        for transition in &transitions {
            let mut temp_value_gen = value_gen.clone();
            if let Some(choice) = transition.is_enabled(&config, env, &mut temp_value_gen) {
                let mut new_config = config.clone();
                if transition.fire(&mut new_config, &choice, env, &mut temp_value_gen) {
                    let new_hash = format!("{:?}", new_config);
                    if !visited.contains(&new_hash) {
                        visited.insert(new_hash);
                        queue.push_back(new_config.clone());
                        results.push(new_config);
                        value_gen = temp_value_gen;
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bfs_search() {
        // 创建简单的测试变迁
        // 这里需要实际的变迁实现
        // 简化测试
        let transitions: Vec<&dyn Transition> = vec![];
        let env = crate::pushdown_colored_pt_net::env::MockEnv::new();
        let initial = Config::new();
        let search_config = SearchConfig::default();

        let result = search_until(
            transitions,
            &env,
            initial,
            |_config| false, // 永远不会找到目标
            search_config,
        );

        assert!(!result.found);
    }
}
