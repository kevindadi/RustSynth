//! Petri 网展开 (Unfolding) 模块
//!
//! - **Occurrence Net**: 展开后的无环 Petri 网
//! - **Event**: 展开后的变迁实例（每个事件只发生一次）
//! - **Condition**: 展开后的 place 实例
//! - **Cut**: 一个配置的最终状态
//! - **Configuration**: 一个事件集合，表示一个可能的执行序列

use crate::pushdown_colored_pt_net::net::{PushdownColoredPetriNet, TokenColor, StackOperation};
use std::collections::{HashMap, HashSet, VecDeque};

/// 展开后的 Petri 网（Occurrence Net）
#[derive(Debug, Clone)]
pub struct UnfoldedPetriNet {
    /// 事件列表（展开后的变迁实例）
    pub events: Vec<Event>,
    /// 条件列表（展开后的 place 实例）
    pub conditions: Vec<Condition>,
    /// 事件之间的因果关系（< 关系）
    pub causality: HashMap<usize, HashSet<usize>>,
    /// 事件之间的冲突关系（# 关系）
    pub conflict: HashMap<usize, HashSet<usize>>,
    /// 事件之间的并发关系（|| 关系）
    pub concurrency: HashMap<usize, HashSet<usize>>,
    /// 初始条件集合
    pub initial_conditions: HashSet<usize>,
    /// 最终条件集合（可能的终止状态）
    pub final_conditions: HashSet<usize>,
    /// 事件到原始变迁的映射
    pub event_to_transition: HashMap<usize, usize>,
    /// 条件到原始 place 的映射
    pub condition_to_place: HashMap<usize, usize>,
}

/// 展开后的事件（变迁实例）
#[derive(Debug, Clone)]
pub struct Event {
    /// 事件 ID
    pub id: usize,
    /// 对应的原始变迁索引
    pub transition_idx: usize,
    /// 变迁名称
    pub name: String,
    /// 前置条件（输入 places）
    pub preconditions: Vec<usize>,
    /// 后置条件（输出 places）
    pub postconditions: Vec<usize>,
    /// 栈操作
    pub stack_operation: Option<StackOperation>,
}

/// 展开后的条件（place 实例）
#[derive(Debug, Clone)]
pub struct Condition {
    /// 条件 ID
    pub id: usize,
    /// 对应的原始 place 索引
    pub place_idx: usize,
    /// Place 名称
    pub name: String,
    /// Token 颜色
    pub color: Option<TokenColor>,
    /// 是否有 token（初始标记）
    pub has_token: bool,
}

/// 展开配置
#[derive(Debug, Clone)]
pub struct UnfoldingConfig {
    /// 最大展开深度
    pub max_depth: usize,
    /// 最大事件数量
    pub max_events: usize,
    /// 是否展开循环
    pub unfold_loops: bool,
    /// 循环展开次数
    pub loop_unfold_count: usize,
}

impl Default for UnfoldingConfig {
    fn default() -> Self {
        Self {
            max_depth: 10,
            max_events: 1000,
            unfold_loops: true,
            loop_unfold_count: 3,
        }
    }
}

impl UnfoldedPetriNet {
    /// 创建空的展开网
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            conditions: Vec::new(),
            causality: HashMap::new(),
            conflict: HashMap::new(),
            concurrency: HashMap::new(),
            initial_conditions: HashSet::new(),
            final_conditions: HashSet::new(),
            event_to_transition: HashMap::new(),
            condition_to_place: HashMap::new(),
        }
    }

    /// 获取所有配置（可能的执行序列）
    ///
    /// 配置是一个事件集合，表示一个可能的执行序列
    pub fn get_configurations(&self, max_size: usize) -> Vec<Vec<usize>> {
        let mut configurations = Vec::new();
        let mut stack: Vec<Vec<usize>> = vec![Vec::new()];

        while let Some(current) = stack.pop() {
            if current.len() > max_size {
                continue;
            }

            // 检查当前配置是否完整（没有更多可触发的事件）
            let enabled = self.get_enabled_events(&current);
            if enabled.is_empty() {
                configurations.push(current);
                continue;
            }

            // 为每个可触发的事件创建新配置
            for event_id in enabled {
                let mut new_config = current.clone();
                new_config.push(event_id);
                stack.push(new_config);
            }
        }

        configurations
    }

    /// 获取在当前配置下可触发的事件
    fn get_enabled_events(&self, configuration: &[usize]) -> Vec<usize> {
        // 找出所有前置条件都满足的事件
        let mut enabled = Vec::new();

        for event in &self.events {
            // 检查是否已经在配置中
            if configuration.contains(&event.id) {
                continue;
            }

            // 检查前置条件是否都满足
            let all_satisfied = event.preconditions.iter().all(|&cond_id| {
                // 条件必须满足：要么是初始条件，要么被配置中的某个事件产生
                self.initial_conditions.contains(&cond_id)
                    || configuration.iter().any(|&event_id| {
                        self.events
                            .iter()
                            .find(|e| e.id == event_id)
                            .map(|e| e.postconditions.contains(&cond_id))
                            .unwrap_or(false)
                    })
            });

            // 检查是否与配置中的事件冲突
            let conflicts = self.conflict.get(&event.id).cloned().unwrap_or_default();
            let has_conflict = configuration.iter().any(|&eid| conflicts.contains(&eid));

            if all_satisfied && !has_conflict {
                enabled.push(event.id);
            }
        }

        enabled
    }

    /// 将配置转换为事件序列
    pub fn configuration_to_sequence(&self, configuration: &[usize]) -> Vec<String> {
        // 根据因果关系排序
        let mut sorted = configuration.to_vec();
        sorted.sort_by(|&a, &b| {
            // 如果 a 是 b 的原因，a 应该在前面
            if self.causality.get(&a).map(|s| s.contains(&b)).unwrap_or(false) {
                std::cmp::Ordering::Less
            } else if self.causality.get(&b).map(|s| s.contains(&a)).unwrap_or(false) {
                std::cmp::Ordering::Greater
            } else {
                // 并发事件可以任意顺序
                a.cmp(&b)
            }
        });

        sorted
            .iter()
            .filter_map(|&event_id| {
                self.events
                    .iter()
                    .find(|e| e.id == event_id)
                    .map(|e| e.name.clone())
            })
            .collect()
    }

    /// 获取统计信息
    pub fn stats(&self) -> UnfoldingStats {
        UnfoldingStats {
            event_count: self.events.len(),
            condition_count: self.conditions.len(),
            initial_condition_count: self.initial_conditions.len(),
            final_condition_count: self.final_conditions.len(),
            causality_relations: self.causality.values().map(|s| s.len()).sum(),
            conflict_relations: self.conflict.values().map(|s| s.len()).sum(),
            concurrency_relations: self.concurrency.values().map(|s| s.len()).sum(),
        }
    }
}

/// 展开统计信息
#[derive(Debug, Clone)]
pub struct UnfoldingStats {
    pub event_count: usize,
    pub condition_count: usize,
    pub initial_condition_count: usize,
    pub final_condition_count: usize,
    pub causality_relations: usize,
    pub conflict_relations: usize,
    pub concurrency_relations: usize,
}

impl Default for UnfoldedPetriNet {
    fn default() -> Self {
        Self::new()
    }
}

/// 展开 Petri 网
///
/// 将原始的 Petri 网展开为无环的 Occurrence Net
pub fn unfold_petri_net(
    pcpn: &PushdownColoredPetriNet,
    config: UnfoldingConfig,
) -> UnfoldedPetriNet {
    let mut unfolded = UnfoldedPetriNet::new();
    let mut event_counter = 0;
    let mut condition_counter = 0;

    // 创建初始条件（从初始标记）
    let mut initial_conditions_map: HashMap<usize, Vec<usize>> = HashMap::new();
    for (place_idx, colors) in &pcpn.initial_marking {
        for (color, count) in colors {
            for _ in 0..*count {
                let cond_id = condition_counter;
                condition_counter += 1;

                let condition = Condition {
                    id: cond_id,
                    place_idx: *place_idx,
                    name: format!("{}_{}", pcpn.places[*place_idx], cond_id),
                    color: Some(color.clone()),
                    has_token: true,
                };

                unfolded.conditions.push(condition);
                unfolded.initial_conditions.insert(cond_id);
                unfolded
                    .condition_to_place
                    .insert(cond_id, *place_idx);

                initial_conditions_map
                    .entry(*place_idx)
                    .or_insert_with(Vec::new)
                    .push(cond_id);
            }
        }
    }

    // 如果没有初始标记，为每个 place 创建一个初始条件
    if initial_conditions_map.is_empty() {
        for (place_idx, _) in pcpn.places.iter().enumerate() {
            let cond_id = condition_counter;
            condition_counter += 1;

            let condition = Condition {
                id: cond_id,
                place_idx,
                name: format!("{}_{}", pcpn.places[place_idx], cond_id),
                color: None,
                has_token: false,
            };

            unfolded.conditions.push(condition);
            unfolded.initial_conditions.insert(cond_id);
            unfolded.condition_to_place.insert(cond_id, place_idx);

            initial_conditions_map
                .entry(place_idx)
                .or_insert_with(Vec::new)
                .push(cond_id);
        }
    }

    // BFS 展开：从初始条件开始，逐步展开可触发的事件
    let mut queue: VecDeque<(Vec<usize>, usize)> = VecDeque::new();
    queue.push_back((initial_conditions_map.values().flatten().copied().collect(), 0));

    let mut visited_configs: HashSet<Vec<usize>> = HashSet::new();
    let mut place_to_conditions: HashMap<usize, Vec<usize>> = initial_conditions_map;

    while let Some((current_conditions, depth)) = queue.pop_front() {
        if depth >= config.max_depth || unfolded.events.len() >= config.max_events {
            // 标记当前条件为最终条件
            for cond_id in &current_conditions {
                unfolded.final_conditions.insert(*cond_id);
            }
            continue;
        }

        // 检查是否已访问过此配置（简化检查）
        let config_key = current_conditions.clone();
        if visited_configs.contains(&config_key) {
            continue;
        }
        visited_configs.insert(config_key.clone());

        let _current_conditions = &current_conditions;

        // 找出所有可触发的事件
        let enabled_transitions = find_enabled_transitions(pcpn, &current_conditions, &place_to_conditions);

        for trans_idx in enabled_transitions {
            // 创建新事件
            let event_id = event_counter;
            event_counter += 1;

            // 收集前置条件
            let mut preconditions = Vec::new();
            let mut consumed_conditions = Vec::new();

            for arc in &pcpn.arcs {
                if arc.is_input_arc && arc.to_idx == trans_idx {
                    // 找到对应的条件
                    if let Some(conditions) = place_to_conditions.get(&arc.from_idx) {
                        if let Some(&cond_id) = conditions.first() {
                            preconditions.push(cond_id);
                            consumed_conditions.push((arc.from_idx, cond_id));
                        }
                    }
                }
            }

            // 创建后置条件
            let mut postconditions = Vec::new();
            for arc in &pcpn.arcs {
                if !arc.is_input_arc && arc.from_idx == trans_idx {
                    let cond_id = condition_counter;
                    condition_counter += 1;

                    let condition = Condition {
                        id: cond_id,
                        place_idx: arc.to_idx,
                        name: format!("{}_{}", pcpn.places[arc.to_idx], cond_id),
                        color: arc.color_constraint.clone(),
                        has_token: false,
                    };

                    unfolded.conditions.push(condition);
                    postconditions.push(cond_id);
                    unfolded.condition_to_place.insert(cond_id, arc.to_idx);

                    // 更新 place 到条件的映射
                    place_to_conditions
                        .entry(arc.to_idx)
                        .or_insert_with(Vec::new)
                        .push(cond_id);
                }
            }

            // 创建事件
            let event = Event {
                id: event_id,
                transition_idx: trans_idx,
                name: pcpn.transitions[trans_idx].clone(),
                preconditions: preconditions.clone(),
                postconditions: postconditions.clone(),
                stack_operation: pcpn.stack_operations.get(&trans_idx).copied(),
            };

            unfolded.events.push(event);
            unfolded.event_to_transition.insert(event_id, trans_idx);

            // 建立因果关系（条件 -> 事件）
            for pre in &preconditions {
                unfolded
                    .causality
                    .entry(*pre)
                    .or_insert_with(HashSet::new)
                    .insert(event_id);
            }

            // 更新当前条件（移除消耗的条件，添加产生的条件）
            let mut new_conditions = current_conditions.clone();
            for (place_idx, cond_id) in &consumed_conditions {
                new_conditions.retain(|&c| c != *cond_id);
                if let Some(conditions) = place_to_conditions.get_mut(place_idx) {
                    conditions.retain(|&c| c != *cond_id);
                }
            }
            new_conditions.extend(&postconditions);

            // 添加到队列继续展开
            queue.push_back((new_conditions, depth + 1));
        }
    }

    // 计算冲突关系（简化版：如果两个事件共享前置条件，它们可能冲突）
    for i in 0..unfolded.events.len() {
        for j in (i + 1)..unfolded.events.len() {
            let event_i = &unfolded.events[i];
            let event_j = &unfolded.events[j];

            // 如果两个事件共享前置条件，它们可能冲突
            let shared_pre = event_i
                .preconditions
                .iter()
                .any(|&p| event_j.preconditions.contains(&p));

            if shared_pre {
                unfolded
                    .conflict
                    .entry(event_i.id)
                    .or_insert_with(HashSet::new)
                    .insert(event_j.id);
                unfolded
                    .conflict
                    .entry(event_j.id)
                    .or_insert_with(HashSet::new)
                    .insert(event_i.id);
            }
        }
    }

    unfolded
}

/// 找出在当前条件下可触发的变迁
fn find_enabled_transitions(
    pcpn: &PushdownColoredPetriNet,
    _current_conditions: &[usize],
    place_to_conditions: &HashMap<usize, Vec<usize>>,
) -> Vec<usize> {
    let mut enabled = Vec::new();

    for (trans_idx, _) in pcpn.transitions.iter().enumerate() {
        let mut can_fire = true;

        // 检查所有输入弧是否满足
        for arc in &pcpn.arcs {
            if arc.is_input_arc && arc.to_idx == trans_idx {
                // 检查对应的 place 是否有可用的条件
                if let Some(conditions) = place_to_conditions.get(&arc.from_idx) {
                    if conditions.is_empty() {
                        can_fire = false;
                        break;
                    }
                } else {
                    can_fire = false;
                    break;
                }
            }
        }

        if can_fire {
            enabled.push(trans_idx);
        }
    }

    enabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir_graph::EdgeMode;

    #[test]
    fn test_simple_unfolding() {
        let mut pcpn = PushdownColoredPetriNet::new();
        
        // 创建简单的 Petri 网：P1 -> T1 -> P2
        let p1 = pcpn.add_place("P1".to_string());
        let t1 = pcpn.add_transition("T1".to_string());
        let p2 = pcpn.add_place("P2".to_string());

        let color = TokenColor::Primitive("u8".to_string());
        pcpn.set_initial_marking(p1, color.clone(), 1);
        pcpn.add_input_arc(p1, t1, EdgeMode::Move, 1, None, Some(color.clone()));
        pcpn.add_output_arc(t1, p2, EdgeMode::Move, 1, None, Some(color));

        let config = UnfoldingConfig::default();
        let unfolded = unfold_petri_net(&pcpn, config);

        assert!(!unfolded.events.is_empty());
        assert!(!unfolded.conditions.is_empty());
    }
}
