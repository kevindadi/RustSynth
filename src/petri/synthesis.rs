use std::collections::{HashSet, VecDeque};

use super::net::{PetriNet, PlaceId, TransitionId};
use super::type_repr::TypeDescriptor;

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
    InvalidTypes {
        missing: Vec<String>,
    },
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
        let place_indices: std::collections::HashMap<PlaceId, usize> = self.net
            .places()
            .enumerate()
            .map(|(idx, (place_id, _))| (place_id, idx))
            .collect();
        
        let place_len = place_indices.len();
        let mut initial_marking = vec![0u32; place_len];

        let mut missing = Vec::new();
        for descriptor in initial {
            if let Some(place_id) = self.net.place_id(descriptor) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    initial_marking[idx] += 1;
                }
            } else {
                missing.push(descriptor.display().to_string());
            }
        }

        let mut goal_tokens = vec![0u32; place_len];
        for descriptor in goal {
            if let Some(place_id) = self.net.place_id(descriptor) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    goal_tokens[idx] += 1;
                }
            } else {
                missing.push(descriptor.display().to_string());
            }
        }

        if !missing.is_empty() {
            return SynthesisOutcome::InvalidTypes { missing };
        }

        if satisfies_goal(&initial_marking, &goal_tokens) {
            return SynthesisOutcome::Success(SynthesisPlan {
                transitions: Vec::new(),
            });
        }

        let mut visited = HashSet::new();
        visited.insert(initial_marking.clone());

        let mut queue: VecDeque<(Vec<u32>, Vec<TransitionId>)> = VecDeque::new();
        queue.push_back((initial_marking, Vec::new()));

        while let Some((marking, path)) = queue.pop_front() {
            if path.len() >= self.config.max_depth {
                continue;
            }

            for (transition_id, _transition) in self.net.transitions() {
                if !is_enabled(&marking, self.net, transition_id, &place_indices) {
                    continue;
                }

                let mut next_marking = marking.clone();
                fire_transition(&mut next_marking, self.net, transition_id, &place_indices);

                if !visited.insert(next_marking.clone()) {
                    continue;
                }

                let mut next_path = path.clone();
                next_path.push(transition_id);

                if satisfies_goal(&next_marking, &goal_tokens) {
                    return SynthesisOutcome::Success(SynthesisPlan { transitions: next_path });
                }

                if visited.len() > self.config.max_states {
                    return SynthesisOutcome::LimitExceeded;
                }

                queue.push_back((next_marking, next_path));
            }
        }

        SynthesisOutcome::GoalUnreachable
    }
}

fn satisfies_goal(marking: &[u32], goal: &[u32]) -> bool {
    marking
        .iter()
        .zip(goal.iter())
        .all(|(have, need)| have >= need)
}

fn is_enabled(
    marking: &[u32],
    net: &PetriNet,
    transition_id: TransitionId,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
) -> bool {
    net.transition_inputs(transition_id).all(|(place_id, arc_data)| {
        if let Some(&idx) = place_indices.get(&place_id) {
            let available = marking[idx];
            available >= arc_data.weight
        } else {
            false
        }
    })
}

fn fire_transition(
    marking: &mut [u32],
    net: &PetriNet,
    transition_id: TransitionId,
    place_indices: &std::collections::HashMap<PlaceId, usize>,
) {
    // 消耗输入弧的令牌
    for (place_id, arc_data) in net.transition_inputs(transition_id) {
        if let Some(&idx) = place_indices.get(&place_id) {
            marking[idx] = marking[idx].saturating_sub(arc_data.weight);
        }
    }

    // 产生输出弧的令牌
    for (place_id, arc_data) in net.transition_outputs(transition_id) {
        if let Some(&idx) = place_indices.get(&place_id) {
            marking[idx] += arc_data.weight;
        }
    }
}

