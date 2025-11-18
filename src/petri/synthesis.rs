use std::collections::{HashSet, VecDeque};

use super::net::{PetriNet, PlaceId, TransitionId};
use rustdoc_types::Id;

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
}

#[derive(Clone, Debug)]
pub struct SynthesisPlan {
    pub transitions: Vec<TransitionId>,
    pub states: Vec<StepState>, // 每个步骤后的状态(包括初始状态和每个变迁后的状态)
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

    pub fn synthesize(&self, initial: &[Id], goal: &[Id]) -> SynthesisOutcome {
        // 创建 PlaceId 到索引的映射
        let place_indices: std::collections::HashMap<PlaceId, usize> = self
            .net
            .places()
            .enumerate()
            .map(|(idx, (place_id, _))| (place_id, idx))
            .collect();

        let place_len = place_indices.len();
        let mut initial_marking = vec![0u32; place_len];

        let mut missing = Vec::new();
        for item_id in initial {
            if let Some(place_id) = self.net.place_id(*item_id) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    initial_marking[idx] += 1;
                }
            } else {
                missing.push(format!("Item ID: {}", item_id.0));
            }
        }

        let mut goal_tokens = vec![0u32; place_len];
        let mut goal_place_ids: Vec<PlaceId> = Vec::new();
        for item_id in goal {
            if let Some(place_id) = self.net.place_id(*item_id) {
                if let Some(&idx) = place_indices.get(&place_id) {
                    goal_tokens[idx] += 1;
                    goal_place_ids.push(place_id);
                }
            } else {
                missing.push(format!("Item ID: {}", item_id.0));
            }
        }

        if !missing.is_empty() {
            return SynthesisOutcome::InvalidTypes { missing };
        }

        // 记录初始状态
        let initial_state = StepState {
            marking: initial_marking.clone(),
        };

        if satisfies_goal(&initial_marking, &goal_tokens) {
            return SynthesisOutcome::Success(SynthesisPlan {
                transitions: Vec::new(),
                states: vec![initial_state],
                place_indices: place_indices.clone(),
            });
        }

        let mut visited = HashSet::new();
        visited.insert(initial_marking.clone());

        let mut queue: VecDeque<(Vec<u32>, Vec<TransitionId>, Vec<StepState>)> = VecDeque::new();
        queue.push_back((initial_marking, Vec::new(), vec![initial_state]));

        while let Some((marking, path, states)) = queue.pop_front() {
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

                // 记录新状态
                let next_state = StepState {
                    marking: next_marking.clone(),
                };
                let mut next_states = states.clone();
                next_states.push(next_state);

                if satisfies_goal(&next_marking, &goal_tokens) {
                    return SynthesisOutcome::Success(SynthesisPlan {
                        transitions: next_path,
                        states: next_states,
                        place_indices: place_indices.clone(),
                    });
                }

                if visited.len() > self.config.max_states {
                    return SynthesisOutcome::LimitExceeded;
                }

                queue.push_back((next_marking, next_path, next_states));
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
    // 检查所有输入弧是否满足
    net.transition_inputs(transition_id)
        .all(|(place_id, arc_data)| {
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
