use std::collections::{HashSet, VecDeque};

use super::net::{PetriNet, PlaceId, TransitionId};
use super::type_repr::{BorrowKind, TypeDescriptor};

#[derive(Clone, Copy, Debug)]
pub struct SynthesisConfig {
    pub max_depth: usize,
    pub max_states: usize,
}

impl Default for SynthesisConfig {
    fn default() -> Self {
        Self {
            max_depth: 6,
            max_states: 10_000,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StepState {
    pub marking: Vec<u32>,
    pub available_borrows: std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
}

#[derive(Clone, Debug)]
pub struct SynthesisPlan {
    pub transitions: Vec<TransitionId>,
    pub states: Vec<StepState>, // 每个步骤后的状态（包括初始状态和每个变迁后的状态）
    pub place_indices: std::collections::HashMap<PlaceId, usize>, // PlaceId 到索引的映射
}

#[derive(Clone, Debug)]
pub enum SynthesisOutcome {
    Success(SynthesisPlan),
    InvalidTypes { missing: Vec<String> },
    LimitExceeded,
    GoalUnreachable,
}

pub struct Synthesizer<'a> {
    net: &'a PetriNet,
    config: SynthesisConfig,
}

impl<'a> Synthesizer<'a> {
    pub fn new(net: &'a PetriNet) -> Self {
        Self {
            net,
            config: SynthesisConfig::default(),
        }
    }

    pub fn with_config(net: &'a PetriNet, config: SynthesisConfig) -> Self {
        Self { net, config }
    }

    pub fn synthesize(
        &self,
        initial: &[TypeDescriptor],
        goal: &[TypeDescriptor],
    ) -> SynthesisOutcome {
        // 创建 PlaceId 到索引的映射
        let place_indices: std::collections::HashMap<PlaceId, usize> = self
            .net
            .places()
            .enumerate()
            .map(|(idx, (place_id, _))| (place_id, idx))
            .collect();

        let place_len = place_indices.len();
        let mut initial_marking = vec![0u32; place_len];
        // 跟踪每个库所中可用的借用类型集合
        let mut available_borrows: std::collections::HashMap<PlaceId, HashSet<BorrowKind>> =
            std::collections::HashMap::new();

        let mut missing = Vec::new();
        for descriptor in initial {
            if let Some(place_id) = self.net.place_id(descriptor) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    initial_marking[idx] += 1;
                    // 记录该库所中可用的借用类型
                    available_borrows
                        .entry(place_id)
                        .or_insert_with(HashSet::new)
                        .insert(descriptor.borrow_kind());
                }
            } else {
                missing.push(descriptor.display().to_string());
            }
        }

        let mut goal_tokens = vec![0u32; place_len];
        let mut goal_descriptors: Vec<(PlaceId, TypeDescriptor)> = Vec::new();
        for descriptor in goal {
            if let Some(place_id) = self.net.place_id(descriptor) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    goal_tokens[idx] += 1;
                    goal_descriptors.push((place_id, descriptor.clone()));
                }
            } else {
                missing.push(descriptor.display().to_string());
            }
        }

        if !missing.is_empty() {
            return SynthesisOutcome::InvalidTypes { missing };
        }

        // 记录初始状态
        let initial_state = StepState {
            marking: initial_marking.clone(),
            available_borrows: available_borrows.clone(),
        };

        if satisfies_goal(
            &initial_marking,
            &goal_tokens,
            &goal_descriptors,
            &available_borrows,
        ) {
            return SynthesisOutcome::Success(SynthesisPlan {
                transitions: Vec::new(),
                states: vec![initial_state],
                place_indices: place_indices.clone(),
            });
        }

        let mut visited = HashSet::new();
        visited.insert(initial_marking.clone());

        let mut queue: VecDeque<(
            Vec<u32>,
            Vec<TransitionId>,
            std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
            Vec<StepState>, // 状态序列
        )> = VecDeque::new();
        queue.push_back((
            initial_marking,
            Vec::new(),
            available_borrows,
            vec![initial_state],
        ));

        while let Some((marking, path, available_borrows, states)) = queue.pop_front() {
            if path.len() >= self.config.max_depth {
                continue;
            }

            for (transition_id, _transition) in self.net.transitions() {
                if !is_enabled(
                    &marking,
                    self.net,
                    transition_id,
                    &place_indices,
                    &available_borrows,
                ) {
                    continue;
                }

                let mut next_marking = marking.clone();
                let mut next_available_borrows = available_borrows.clone();
                fire_transition(
                    &mut next_marking,
                    &mut next_available_borrows,
                    self.net,
                    transition_id,
                    &place_indices,
                );

                if !visited.insert(next_marking.clone()) {
                    continue;
                }

                let mut next_path = path.clone();
                next_path.push(transition_id);

                // 记录新状态
                let next_state = StepState {
                    marking: next_marking.clone(),
                    available_borrows: next_available_borrows.clone(),
                };
                let mut next_states = states.clone();
                next_states.push(next_state);

                if satisfies_goal(
                    &next_marking,
                    &goal_tokens,
                    &goal_descriptors,
                    &next_available_borrows,
                ) {
                    return SynthesisOutcome::Success(SynthesisPlan {
                        transitions: next_path,
                        states: next_states,
                        place_indices: place_indices.clone(),
                    });
                }

                if visited.len() > self.config.max_states {
                    return SynthesisOutcome::LimitExceeded;
                }

                queue.push_back((next_marking, next_path, next_available_borrows, next_states));
            }
        }

        SynthesisOutcome::GoalUnreachable
    }
}

fn satisfies_goal(
    marking: &[u32],
    goal: &[u32],
    goal_descriptors: &[(PlaceId, TypeDescriptor)],
    available_borrows: &std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
) -> bool {
    // 首先检查标记数量
    if !marking
        .iter()
        .zip(goal.iter())
        .all(|(have, need)| have >= need)
    {
        return false;
    }

    // 然后检查每个目标类型的借用类型兼容性（需要完全匹配）
    for (place_id, goal_descriptor) in goal_descriptors {
        let required_borrow = goal_descriptor.borrow_kind();

        // 获取该库所中可用的借用类型集合
        if let Some(available_set) = available_borrows.get(place_id) {
            // 检查是否有任何可用的借用类型与目标类型完全匹配
            if !available_set.iter().any(|&available_borrow| {
                is_goal_borrow_compatible(available_borrow, required_borrow)
            }) {
                return false;
            }
        } else {
            // 如果没有记录，假设是 Owned
            if !is_goal_borrow_compatible(BorrowKind::Owned, required_borrow) {
                return false;
            }
        }
    }

    true
}

/// 检查借用类型是否兼容
///
/// 兼容性规则（用于输入弧）：
/// - 如果弧需要 Owned，只有 Owned 可以满足
/// - 如果弧需要 SharedRef，Owned 或 SharedRef 可以满足（可以从 Owned 借用得到 SharedRef）
/// - 如果弧需要 MutRef，Owned 或 MutRef 可以满足（可以从 Owned 借用得到 MutRef）
/// - 相同类型总是兼容
///
/// 注意：这个函数用于检查输入弧的兼容性。对于目标类型，需要完全匹配。
fn is_borrow_compatible(available: BorrowKind, required: BorrowKind) -> bool {
    match (available, required) {
        // 相同类型总是兼容
        (a, r) if a == r => true,
        // 如果弧需要 Owned，只有 Owned 可以满足
        (_, BorrowKind::Owned) => available == BorrowKind::Owned,
        // 如果弧需要 SharedRef，Owned 或 SharedRef 可以满足
        (BorrowKind::Owned, BorrowKind::SharedRef) => true,
        (BorrowKind::SharedRef, BorrowKind::SharedRef) => true,
        // 如果弧需要 MutRef，Owned 或 MutRef 可以满足
        (BorrowKind::Owned, BorrowKind::MutRef) => true,
        (BorrowKind::MutRef, BorrowKind::MutRef) => true,
        // 其他情况不兼容
        _ => false,
    }
}

/// 检查目标类型的借用类型是否匹配
///
/// 对于目标类型，需要完全匹配（不能从 T 得到 &T 或 &mut T）
fn is_goal_borrow_compatible(available: BorrowKind, required: BorrowKind) -> bool {
    available == required
}

fn is_enabled(
    marking: &[u32],
    net: &PetriNet,
    transition_id: TransitionId,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
    available_borrows: &std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
) -> bool {
    // 首先检查所有有 Place 的输入弧是否满足
    let all_place_inputs_satisfied = net.transition_inputs(transition_id)
        .all(|(place_id, arc_data)| {
            if let Some(&idx) = place_indices.get(&place_id) {
                let available = marking[idx];
                if available < arc_data.weight {
                    return false;
                }
                // 检查借用类型兼容性
                if let Some(required_borrow) = arc_data.borrow_kind {
                    // 获取库所中可用的借用类型集合
                    if let Some(available_set) = available_borrows.get(&place_id) {
                        // 检查是否有任何可用的借用类型与所需类型兼容
                        if !available_set.iter().any(|&available_borrow| {
                            is_borrow_compatible(available_borrow, required_borrow)
                        }) {
                            return false;
                        }
                    } else {
                        // 如果没有记录，假设是 Owned（向后兼容）
                        if !is_borrow_compatible(BorrowKind::Owned, required_borrow) {
                            return false;
                        }
                    }
                }
                true
            } else {
                false
            }
        });

    if !all_place_inputs_satisfied {
        return false;
    }

    // 检查泛型参数的 guard：对于没有 Place 的输入参数（泛型参数），检查是否有 token 满足约束
    if let Some(transition) = net.transition(transition_id) {
        let summary = &transition.summary;
        
        // 收集所有有 Place 的输入参数
        let place_input_descriptors: HashSet<_> = net.transition_inputs(transition_id)
            .filter_map(|(place_id, _)| {
                net.place(place_id).map(|place| place.descriptor.clone())
            })
            .collect();

        // 检查所有输入参数（包括泛型参数）
        for input_param in &summary.inputs {
            // 如果该输入参数没有对应的 Place（即泛型参数），需要 guard 检查
            if !place_input_descriptors.contains(&input_param.descriptor.normalized()) {
                if !check_generic_guard(
                    &input_param.descriptor,
                    marking,
                    net,
                    place_indices,
                    available_borrows,
                    &summary.trait_bounds,
                ) {
                    return false;
                }
            }
        }
    }

    true
}

/// 检查泛型参数的 guard：当前 marking 中是否有 token 满足该泛型参数的约束
fn check_generic_guard(
    _generic_descriptor: &TypeDescriptor,
    marking: &[u32],
    net: &PetriNet,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
    _available_borrows: &std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
    trait_bounds: &[std::sync::Arc<str>],
) -> bool {
    // 如果泛型参数没有 trait 约束，所有类型都可以满足（但这里应该至少有一个 token）
    if trait_bounds.is_empty() {
        // 检查 marking 中是否有任何 token
        return marking.iter().any(|&count| count > 0);
    }

    // 对于每个有 token 的 Place，检查是否满足泛型参数的 trait 约束
    for (place_id, place) in net.places() {
        if let Some(&idx) = place_indices.get(&place_id) {
            if marking[idx] > 0 {
                // 检查该 Place 是否满足泛型参数的所有 trait 约束
                if trait_bounds_satisfied(place, trait_bounds) {
                    // 还需要检查借用类型兼容性（如果有要求）
                    // 泛型参数默认是 Owned，但可以从其他借用类型转换
                    return true;
                }
            }
        }
    }

    false
}

/// 检查 Place 是否满足给定的 trait 约束
fn trait_bounds_satisfied(
    place: &super::net::Place,
    trait_bounds: &[std::sync::Arc<str>],
) -> bool {
    // 获取 Place 实现的 trait 列表
    let implemented_traits: HashSet<&str> = place
        .implemented_traits
        .iter()
        .map(|t| t.as_ref())
        .collect();

    // 检查是否所有要求的 trait 都被实现了
    trait_bounds.iter().all(|bound| {
        let bound_str = bound.as_ref();
        
        // 提取 trait 名称（去掉可能的关联类型约束，如 Iterator<Item = u8> 中的 Iterator）
        let trait_name = if let Some(lt_pos) = bound_str.find('<') {
            &bound_str[..lt_pos].trim()
        } else {
            bound_str.trim()
        };
        
        // 移除可能的 '?' 修饰符
        let trait_name = trait_name.trim_start_matches('?').trim();
        
        // 提取简单名称（最后一部分）
        let simple_name = trait_name.split("::").last().unwrap_or(trait_name);
        
        // 检查是否实现了该 trait（支持完整路径或简单名称匹配）
        implemented_traits.iter().any(|&impl_trait| {
            impl_trait == trait_name
                || impl_trait.ends_with(&format!("::{}", simple_name))
                || impl_trait == simple_name
        })
    })
}

fn fire_transition(
    marking: &mut [u32],
    available_borrows: &mut std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
    net: &PetriNet,
    transition_id: TransitionId,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
) {
    // 消耗输入弧的令牌
    for (place_id, arc_data) in net.transition_inputs(transition_id) {
        if let Some(&idx) = place_indices.get(&place_id) {
            marking[idx] = marking[idx].saturating_sub(arc_data.weight);
            // 注意：消耗后，借用类型集合保持不变（因为可能有多个令牌）
        }
    }

    // 产生输出弧的令牌
    for (place_id, arc_data) in net.transition_outputs(transition_id) {
        if let Some(&idx) = place_indices.get(&place_id) {
            marking[idx] += arc_data.weight;
            // 记录输出弧产生的借用类型
            if let Some(output_borrow) = arc_data.borrow_kind {
                available_borrows
                    .entry(place_id)
                    .or_insert_with(HashSet::new)
                    .insert(output_borrow);
            } else {
                // 如果没有指定，假设是 Owned
                available_borrows
                    .entry(place_id)
                    .or_insert_with(HashSet::new)
                    .insert(BorrowKind::Owned);
            }
        }
    }
}
