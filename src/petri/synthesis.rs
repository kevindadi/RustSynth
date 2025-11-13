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
pub struct SynthesisPlan {
    pub transitions: Vec<TransitionId>,
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

        if satisfies_goal(
            &initial_marking,
            &goal_tokens,
            &goal_descriptors,
            &available_borrows,
        ) {
            return SynthesisOutcome::Success(SynthesisPlan {
                transitions: Vec::new(),
            });
        }

        let mut visited = HashSet::new();
        visited.insert(initial_marking.clone());

        let mut queue: VecDeque<(
            Vec<u32>,
            Vec<TransitionId>,
            std::collections::HashMap<PlaceId, HashSet<BorrowKind>>,
        )> = VecDeque::new();
        queue.push_back((initial_marking, Vec::new(), available_borrows));

        while let Some((marking, path, available_borrows)) = queue.pop_front() {
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

                if satisfies_goal(
                    &next_marking,
                    &goal_tokens,
                    &goal_descriptors,
                    &next_available_borrows,
                ) {
                    return SynthesisOutcome::Success(SynthesisPlan {
                        transitions: next_path,
                    });
                }

                if visited.len() > self.config.max_states {
                    return SynthesisOutcome::LimitExceeded;
                }

                queue.push_back((next_marking, next_path, next_available_borrows));
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
    net.transition_inputs(transition_id)
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
