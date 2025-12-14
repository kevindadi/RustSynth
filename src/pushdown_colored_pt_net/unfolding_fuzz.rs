//! 基于展开的模糊测试用例生成
//!
//! 使用 Petri 网展开来生成更系统的测试用例

use crate::pushdown_colored_pt_net::unfolding::{UnfoldedPetriNet, UnfoldingConfig};
use crate::pushdown_colored_pt_net::net::PushdownColoredPetriNet;

/// 基于展开生成模糊测试序列
pub struct UnfoldingBasedFuzzer {
    unfolded: UnfoldedPetriNet,
}

impl UnfoldingBasedFuzzer {
    pub fn new(pcpn: &PushdownColoredPetriNet, config: UnfoldingConfig) -> Self {
        let unfolded = crate::pushdown_colored_pt_net::unfolding::unfold_petri_net(pcpn, config);
        Self { unfolded }
    }

    /// 生成所有可能的执行序列
    pub fn generate_all_sequences(&self, max_sequence_length: usize) -> Vec<Vec<String>> {
        let configurations = self.unfolded.get_configurations(max_sequence_length);
        
        configurations
            .iter()
            .map(|config| self.unfolded.configuration_to_sequence(config))
            .collect()
    }

    pub fn generate_coverage_sequences(&self) -> Vec<Vec<String>> {
        let mut covered_events = HashSet::new();
        let mut sequences = Vec::new();

        while covered_events.len() < self.unfolded.events.len() {
            let mut best_config = None;
            let mut best_coverage = 0;

            let all_configs = self.unfolded.get_configurations(20);
            for config in all_configs {
                let new_events: HashSet<usize> = config
                    .iter()
                    .filter(|&&eid| !covered_events.contains(&eid))
                    .copied()
                    .collect();

                if new_events.len() > best_coverage {
                    best_coverage = new_events.len();
                    best_config = Some(config);
                }
            }

            if let Some(config) = best_config {
                let sequence = self.unfolded.configuration_to_sequence(&config);
                sequences.push(sequence.clone());
                
                for &event_id in &config {
                    covered_events.insert(event_id);
                }
            } else {
                break;
            }
        }

        sequences
    }

    pub fn generate_targeted_sequences(
        &self,
        target_place_idx: usize,
        max_depth: usize,
    ) -> Vec<Vec<String>> {
        let mut sequences = Vec::new();
        let configurations = self.unfolded.get_configurations(max_depth);

        for config in configurations {
            let reaches_target = self
                .unfolded
                .conditions
                .iter()
                .any(|cond| {
                    cond.place_idx == target_place_idx
                        && config.iter().any(|&event_id| {
                            self.unfolded
                                .events
                                .iter()
                                .find(|e| e.id == event_id)
                                .map(|e| e.postconditions.contains(&cond.id))
                                .unwrap_or(false)
                        })
                });

            if reaches_target {
                let sequence = self.unfolded.configuration_to_sequence(&config);
                sequences.push(sequence);
            }
        }

        sequences
    }

    pub fn stats(&self) -> crate::pushdown_colored_pt_net::unfolding::UnfoldingStats {
        self.unfolded.stats()
    }
}

use std::collections::HashSet;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pushdown_colored_pt_net::net::{PushdownColoredPetriNet, TokenColor};
    use crate::ir_graph::EdgeMode;

    #[test]
    fn test_unfolding_fuzzer() {
        let mut pcpn = PushdownColoredPetriNet::new();
        
        let p1 = pcpn.add_place("P1".to_string());
        let t1 = pcpn.add_transition("T1".to_string());
        let p2 = pcpn.add_place("P2".to_string());

        let color = TokenColor::Primitive("u8".to_string());
        pcpn.set_initial_marking(p1, color.clone(), 1);
        pcpn.add_input_arc(p1, t1, EdgeMode::Move, 1, None, Some(color.clone()));
        pcpn.add_output_arc(t1, p2, EdgeMode::Move, 1, None, Some(color));

        let config = UnfoldingConfig::default();
        let fuzzer = UnfoldingBasedFuzzer::new(&pcpn, config);
        
        let sequences = fuzzer.generate_all_sequences(10);
        assert!(!sequences.is_empty());
    }
}
