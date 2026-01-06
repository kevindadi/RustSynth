//! 可达性搜索：BFS/DFS + bounds + 目标谓词

use anyhow::Result;
use std::collections::{HashMap, VecDeque};

use crate::api_extract::ApiSignature;
use crate::canonicalize::state_hash_key;
use crate::model::State;
use crate::transition::{apply_transition, generate_enabled_transitions, Transition};
use crate::type_norm::TypeContext;

/// 搜索配置
pub struct SearchConfig {
    pub max_steps: usize,
    pub max_tokens_per_type: usize,
    pub max_borrow_depth: usize,
    pub enable_loan_stack: bool,
    pub target_type: Option<String>,
}

/// 执行可达性搜索
///
/// 返回：Option<(final_state, trace)>
pub fn search(
    apis: &[ApiSignature],
    type_ctx: &TypeContext,
    config: &SearchConfig,
) -> Result<Option<(State, Vec<Transition>)>> {
    // 初始状态 (空)
    let initial_state = State::new(config.enable_loan_stack);

    // BFS 队列: (state, parent_hash, transition_from_parent)
    let mut queue = VecDeque::new();
    queue.push_back((initial_state.clone(), None, None));

    // 已访问状态 (用 canonical hash)
    let mut visited = HashMap::new();
    visited.insert(state_hash_key(&initial_state), (None, None));

    // 搜索循环
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 10000;

    while let Some((state, _parent_hash, _trans_from_parent)) = queue.pop_front() {
        iterations += 1;
        if iterations > MAX_ITERATIONS {
            tracing::warn!("达到最大迭代次数 {}", MAX_ITERATIONS);
            break;
        }

        let current_hash = state_hash_key(&state);

        // 检查目标条件
        if check_goal(&state, config) {
            tracing::info!("✓ 找到目标状态 (迭代 {})", iterations);
            // 重建 trace
            let trace = reconstruct_trace(&visited, &current_hash);
            return Ok(Some((state, trace)));
        }

        // 检查步数限制
        let depth = compute_depth(&visited, &current_hash);
        if depth >= config.max_steps {
            continue; // 不继续扩展
        }

        // 生成 enabled transitions
        let transitions =
            generate_enabled_transitions(&state, apis, type_ctx, config.max_borrow_depth);

        tracing::debug!(
            "State depth={}, tokens={}, transitions={}",
            depth,
            state.total_tokens(),
            transitions.len()
        );

        // 应用 transitions
        for trans in transitions {
            if let Ok(next_state) = apply_transition(&state, &trans, type_ctx) {
                // 检查 token 数量限制
                if !check_bounds(&next_state, config) {
                    continue;
                }

                let next_hash = state_hash_key(&next_state);

                // 检查是否已访问
                if !visited.contains_key(&next_hash) {
                    visited.insert(
                        next_hash.clone(),
                        (Some(current_hash.clone()), Some(trans.clone())),
                    );
                    queue.push_back((next_state, Some(current_hash.clone()), Some(trans)));
                }
            }
        }
    }

    tracing::info!("搜索完成：访问 {} 个状态", visited.len());
    Ok(None)
}

/// 检查是否达到目标
fn check_goal(state: &State, config: &SearchConfig) -> bool {
    // 目标 1: 达到封闭状态 (无未结束的借用)
    if !state.is_closed() {
        return false;
    }

    // 目标 2: 至少有一些 tokens (非空)
    if state.total_tokens() == 0 {
        return false;
    }

    // 目标 3: 如果指定了 target_type，检查是否有该类型的 owned token
    if let Some(target_type) = &config.target_type {
        let has_target = state
            .places
            .get(target_type)
            .map(|tokens| !tokens.is_empty())
            .unwrap_or(false);
        if !has_target {
            return false;
        }
    }

    true
}

/// 检查状态是否满足 bounds
fn check_bounds(state: &State, config: &SearchConfig) -> bool {
    // 检查每种类型的 token 数量
    for (_, tokens) in &state.places {
        if tokens.len() > config.max_tokens_per_type {
            return false;
        }
    }

    // 检查借用深度
    if let Some(stack) = &state.loan_stack {
        if stack.len() > config.max_borrow_depth {
            return false;
        }
    }

    true
}

/// 计算状态的深度 (从初始状态开始的步数)
fn compute_depth(
    visited: &HashMap<String, (Option<String>, Option<Transition>)>,
    hash: &str,
) -> usize {
    let mut depth = 0;
    let mut current = hash.to_string();

    while let Some((parent, _)) = visited.get(&current) {
        if let Some(parent_hash) = parent {
            depth += 1;
            current = parent_hash.clone();
        } else {
            break;
        }
    }

    depth
}

/// 重建 trace (从初始状态到目标状态的 transitions)
fn reconstruct_trace(
    visited: &HashMap<String, (Option<String>, Option<Transition>)>,
    final_hash: &str,
) -> Vec<Transition> {
    let mut trace = Vec::new();
    let mut current = final_hash.to_string();

    while let Some((parent, trans)) = visited.get(&current) {
        if let Some(t) = trans {
            trace.push(t.clone());
        }
        if let Some(parent_hash) = parent {
            current = parent_hash.clone();
        } else {
            break;
        }
    }

    trace.reverse();
    trace
}

