use std::collections::{HashSet, VecDeque};

use super::net::{ArcMultiplicity, PetriNet, Transition, TransitionId};
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
        let place_len = self.net.place_count();
        let mut initial_marking = vec![0u32; place_len];

        let mut missing = Vec::new();
        for descriptor in initial {
            if let Some(place_id) = self.net.place_id(descriptor) {
                initial_marking[place_id.0] += 1;
            } else {
                missing.push(descriptor.display().to_string());
            }
        }

        let mut goal_tokens = vec![0u32; place_len];
        for descriptor in goal {
            if let Some(place_id) = self.net.place_id(descriptor) {
                goal_tokens[place_id.0] += 1;
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

            for transition in self.net.transitions() {
                if !is_enabled(&marking, transition) {
                    continue;
                }

                let mut next_marking = marking.clone();
                fire_transition(&mut next_marking, transition);

                if !visited.insert(next_marking.clone()) {
                    continue;
                }

                let mut next_path = path.clone();
                next_path.push(transition.id);

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

fn is_enabled(marking: &[u32], transition: &Transition) -> bool {
    transition.inputs.iter().all(|input| {
        let available = marking[input.place.0];
        match input.multiplicity {
            ArcMultiplicity::One => available >= 1,
            ArcMultiplicity::Many(n) => available >= n,
        }
    })
}

fn fire_transition(marking: &mut [u32], transition: &Transition) {
    for input in &transition.inputs {
        match input.multiplicity {
            ArcMultiplicity::One => marking[input.place.0] = marking[input.place.0].saturating_sub(1),
            ArcMultiplicity::Many(n) => {
                marking[input.place.0] = marking[input.place.0].saturating_sub(n);
            }
        }
    }

    for output in &transition.outputs {
        match output.multiplicity {
            ArcMultiplicity::One => marking[output.place.0] += 1,
            ArcMultiplicity::Many(n) => marking[output.place.0] += n,
        }
    }
}

