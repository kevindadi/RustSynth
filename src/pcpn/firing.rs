//! 发生规则
//!
//! 实现变迁的使能条件检查和触发逻辑

use std::collections::HashMap;

use super::types::TypeId;
use super::place::PlaceId;
use super::transition::{TransitionId, Transition, TransitionKind, StructuralKind, ParamPassing};
use super::arc::{Arc, ArcKind};
use super::marking::{Marking, Token, ValueIdGen};
use super::stack::{PushdownStack, StackFrame, StackOp};
use super::net::PcpnNet;

/// 配置 (Configuration)
///
/// Petri 网的完整状态: (M, σ)
#[derive(Debug, Clone)]
pub struct Config {
    /// 标记 M
    pub marking: Marking,
    /// 下推栈 σ
    pub stack: PushdownStack,
}

impl Config {
    pub fn new() -> Self {
        Config {
            marking: Marking::new(),
            stack: PushdownStack::new(),
        }
    }

    pub fn with_marking(marking: Marking) -> Self {
        Config {
            marking,
            stack: PushdownStack::new(),
        }
    }

    /// 计算配置的状态哈希
    pub fn state_hash(&self) -> u64 {
        let m_hash = self.marking.state_hash();
        let s_hash = self.stack.state_hash();
        m_hash.wrapping_mul(31).wrapping_add(s_hash)
    }

    /// 检查配置是否有效
    pub fn is_valid(&self, max_stack_depth: usize) -> bool {
        self.stack.depth() <= max_stack_depth
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// 使能绑定
///
/// 变迁使能时，记录选择的 token 绑定
#[derive(Debug, Clone)]
pub struct EnabledBinding {
    /// 变迁 ID
    pub transition_id: TransitionId,
    /// 输入绑定: 库所 -> 选择的 token 列表
    pub input_bindings: HashMap<PlaceId, Vec<Token>>,
    /// 将要产生的输出 token
    pub output_tokens: HashMap<PlaceId, Vec<Token>>,
    /// 栈操作
    pub stack_op: StackOp,
    /// 栈帧（如果需要 push）
    pub stack_frame: Option<StackFrame>,
}

impl EnabledBinding {
    pub fn new(transition_id: TransitionId) -> Self {
        EnabledBinding {
            transition_id,
            input_bindings: HashMap::new(),
            output_tokens: HashMap::new(),
            stack_op: StackOp::None,
            stack_frame: None,
        }
    }
}

/// 发生规则
///
/// 实现变迁的使能检查和触发
pub struct FiringRule<'a> {
    net: &'a PcpnNet,
}

impl<'a> FiringRule<'a> {
    pub fn new(net: &'a PcpnNet) -> Self {
        FiringRule { net }
    }

    /// 检查变迁是否使能
    ///
    /// 返回 Some(binding) 如果使能，None 如果不使能
    pub fn is_enabled(
        &self,
        config: &Config,
        transition_id: TransitionId,
        value_gen: &mut ValueIdGen,
    ) -> Option<EnabledBinding> {
        let transition = self.net.get_transition(transition_id)?;
        let input_arcs = self.net.get_input_arcs(transition_id);

        let mut binding = EnabledBinding::new(transition_id);

        // 1. 检查所有输入弧的 token 约束
        for arc in &input_arcs {
            if !self.check_input_arc(config, arc, &mut binding)? {
                return None;
            }
        }

        // 2. 检查栈前置条件
        if !self.check_stack_precondition(config, transition) {
            return None;
        }

        // 3. 检查特定变迁的守卫条件
        if !self.check_guard(config, transition, &binding) {
            return None;
        }

        // 4. 计算输出 token
        self.compute_outputs(transition, &mut binding, value_gen);

        // 5. 设置栈操作
        self.set_stack_operation(transition, &mut binding);

        Some(binding)
    }

    /// 检查输入弧约束
    fn check_input_arc(
        &self,
        config: &Config,
        arc: &Arc,
        binding: &mut EnabledBinding,
    ) -> Option<bool> {
        let place_id = arc.from_place()?;
        let place = self.net.get_place(place_id)?;
        let multiset = config.marking.get(place_id)?;

        // 获取颜色约束
        let required_type = arc.color_constraint.or(Some(place.type_id));

        match arc.kind {
            ArcKind::Normal | ArcKind::SelfLoop => {
                // 需要消耗 token
                let tokens: Vec<_> = multiset
                    .tokens_of_type(required_type.unwrap())
                    .into_iter()
                    .take(arc.weight)
                    .cloned()
                    .collect();

                if tokens.len() < arc.weight {
                    return Some(false);
                }

                binding.input_bindings.insert(place_id, tokens);
            }
            ArcKind::Read => {
                // 只检查不消耗
                if multiset.count_type(required_type.unwrap()) < arc.weight {
                    return Some(false);
                }
            }
            ArcKind::Inhibitor => {
                // 库所必须为空
                if !multiset.is_empty() {
                    return Some(false);
                }
            }
        }

        Some(true)
    }

    /// 检查栈前置条件
    fn check_stack_precondition(&self, config: &Config, transition: &Transition) -> bool {
        match &transition.kind {
            TransitionKind::Structural(kind) => {
                match kind {
                    StructuralKind::EndMut => {
                        // 栈顶必须是可变借用帧
                        config.stack.top_matches(|f| f.is_mut_borrow())
                    }
                    StructuralKind::EndShrKeep | StructuralKind::EndShrLast => {
                        // 栈顶必须是共享借用帧
                        config.stack.top_matches(|f| {
                            matches!(f, StackFrame::SharedBorrow { .. })
                        })
                    }
                    StructuralKind::EndProjMut => {
                        // 栈顶必须是字段投影帧
                        config.stack.top_matches(|f| {
                            matches!(f, StackFrame::FieldProj { .. })
                        })
                    }
                    StructuralKind::BorrowShrFrz => {
                        // 需要已有共享借用
                        !config.stack.is_empty()
                    }
                    _ => true,
                }
            }
            _ => true,
        }
    }

    /// 检查守卫条件
    fn check_guard(
        &self,
        config: &Config,
        transition: &Transition,
        binding: &EnabledBinding,
    ) -> bool {
        match &transition.kind {
            TransitionKind::Structural(kind) => {
                match kind {
                    StructuralKind::CopyUse | StructuralKind::DupCopy => {
                        // 类型必须是 Copy
                        if let Some(tokens) = binding.input_bindings.values().next() {
                            if let Some(token) = tokens.first() {
                                return self.net.types.is_copy(token.type_id);
                            }
                        }
                        false
                    }
                    StructuralKind::DupCopy => {
                        // 还需要检查预算
                        transition.dup_budget.map(|b| b > 0).unwrap_or(false)
                    }
                    _ => true,
                }
            }
            TransitionKind::Signature(sig) => {
                // 检查所有参数类型匹配
                true // 已在输入弧检查中处理
            }
            _ => true,
        }
    }

    /// 计算输出 token
    fn compute_outputs(
        &self,
        transition: &Transition,
        binding: &mut EnabledBinding,
        value_gen: &mut ValueIdGen,
    ) {
        let output_arcs = self.net.get_output_arcs(transition.id);

        for arc in output_arcs {
            if let Some(place_id) = arc.to_place() {
                let place = self.net.get_place(place_id);
                let type_id = arc.color_constraint
                    .or(place.map(|p| p.type_id))
                    .unwrap_or(TypeId::new(0));

                let mut tokens = Vec::new();
                for _ in 0..arc.weight {
                    let token = Token {
                        type_id,
                        value_id: value_gen.next(),
                    };
                    tokens.push(token);
                }

                binding.output_tokens.insert(place_id, tokens);
            }
        }

        // 处理自循环弧（Copy 类型）
        match &transition.kind {
            TransitionKind::Structural(StructuralKind::CopyUse) => {
                // 输入 token 也要作为输出
                for (place, tokens) in &binding.input_bindings {
                    binding.output_tokens.entry(*place)
                        .or_default()
                        .extend(tokens.clone());
                }
            }
            TransitionKind::Signature(sig) => {
                // 处理引用参数的自循环
                for param in &sig.params {
                    match param.passing {
                        ParamPassing::ByRef | ParamPassing::ByMutRef => {
                            // 引用不消耗，需要自循环
                            // 这里简化处理
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// 设置栈操作
    fn set_stack_operation(
        &self,
        transition: &Transition,
        binding: &mut EnabledBinding,
    ) {
        binding.stack_op = transition.stack_op;

        // 根据变迁类型创建栈帧
        if transition.stack_op == StackOp::Push {
            let frame = match &transition.kind {
                TransitionKind::Structural(kind) => {
                    self.create_structural_stack_frame(*kind, binding)
                }
                TransitionKind::Signature(sig) => {
                    Some(StackFrame::FnCall {
                        name: sig.name.clone(),
                        return_type: sig.return_type,
                    })
                }
                _ => None,
            };
            binding.stack_frame = frame;
        }
    }

    /// 创建结构变迁的栈帧
    fn create_structural_stack_frame(
        &self,
        kind: StructuralKind,
        binding: &EnabledBinding,
    ) -> Option<StackFrame> {
        // 获取输入 token 的信息
        let (value_id, type_id) = binding.input_bindings
            .values()
            .next()
            .and_then(|tokens| tokens.first())
            .map(|t| (t.value_id, t.type_id))?;

        match kind {
            StructuralKind::BorrowShrOwn => {
                Some(StackFrame::SharedBorrow {
                    value_id,
                    type_id,
                    count: 1,
                })
            }
            StructuralKind::BorrowMut => {
                Some(StackFrame::MutBorrow {
                    value_id,
                    type_id,
                })
            }
            StructuralKind::ProjMove | StructuralKind::ProjShr | StructuralKind::ProjMut => {
                Some(StackFrame::FieldProj {
                    parent_id: value_id,
                    field: "field".to_string(), // 实际应从弧标签获取
                    field_type: type_id,
                })
            }
            _ => None,
        }
    }

    /// 触发变迁
    ///
    /// 应用绑定到配置，产生新配置
    pub fn fire(&self, config: &mut Config, binding: &EnabledBinding) -> bool {
        // 1. 消耗输入 token
        for (place_id, tokens) in &binding.input_bindings {
            for token in tokens {
                if !config.marking.remove(*place_id, token) {
                    return false; // 不应该发生
                }
            }
        }

        // 2. 产生输出 token
        for (place_id, tokens) in &binding.output_tokens {
            for token in tokens {
                config.marking.add(*place_id, token.clone());
            }
        }

        // 3. 执行栈操作
        match binding.stack_op {
            StackOp::Push => {
                if let Some(frame) = &binding.stack_frame {
                    config.stack.push(frame.clone());
                }
            }
            StackOp::Pop => {
                config.stack.pop();
            }
            StackOp::Replace => {
                config.stack.pop();
                if let Some(frame) = &binding.stack_frame {
                    config.stack.push(frame.clone());
                }
            }
            StackOp::None => {}
        }

        true
    }

    /// 获取所有使能的变迁
    pub fn enabled_transitions(
        &self,
        config: &Config,
        value_gen: &mut ValueIdGen,
    ) -> Vec<EnabledBinding> {
        let mut enabled = Vec::new();

        for transition in self.net.transitions() {
            // 克隆 value_gen 用于检查
            let mut temp_gen = value_gen.clone();
            if let Some(binding) = self.is_enabled(config, transition.id, &mut temp_gen) {
                enabled.push(binding);
            }
        }

        // 按优先级排序
        enabled.sort_by(|a, b| {
            let prio_a = self.net.get_transition(a.transition_id)
                .map(|t| t.priority)
                .unwrap_or(0);
            let prio_b = self.net.get_transition(b.transition_id)
                .map(|t| t.priority)
                .unwrap_or(0);
            prio_b.cmp(&prio_a) // 降序
        });

        enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcpn::place::PlaceKind;
    use crate::pcpn::types::{RustType, PrimitiveKind};

    #[test]
    fn test_config_state_hash() {
        let config1 = Config::new();
        let config2 = Config::new();

        assert_eq!(config1.state_hash(), config2.state_hash());
    }

    #[test]
    fn test_enabled_binding() {
        let binding = EnabledBinding::new(TransitionId::new(0));
        assert!(binding.input_bindings.is_empty());
    }
}

