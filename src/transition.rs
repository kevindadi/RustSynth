//! Transition (变迁) 定义与 enabling/firing 逻辑
//!
//! 两类变迁：
//! 1. API Transition: 调用函数/方法
//! 2. Structural Transition: borrow/drop/deref 等结构性操作

use anyhow::{Context, Result};

use crate::api_extract::{ApiSignature, ParamMode, ReturnMode};
use crate::model::{BorrowFlag, Capability, State, Token, VarId};
use crate::type_norm::TypeContext;

/// Transition 类型
#[derive(Debug, Clone)]
pub enum Transition {
    /// API 调用
    ApiCall(ApiCallTransition),
    /// 结构性操作
    Structural(StructuralTransition),
}

impl Transition {
    pub fn description(&self) -> String {
        match self {
            Transition::ApiCall(t) => t.description(),
            Transition::Structural(t) => t.description(),
        }
    }
}

/// API 调用变迁
#[derive(Debug, Clone)]
pub struct ApiCallTransition {
    /// API 签名
    pub api: ApiSignature,
    /// 参数绑定: (ParamMode, TokenId, 适配策略)
    pub arg_bindings: Vec<ArgBinding>,
    /// 返回值 token id (如果有)
    pub return_var: Option<VarId>,
}

/// 参数绑定
#[derive(Debug, Clone)]
pub struct ArgBinding {
    /// 参数模式
    pub param: ParamMode,
    /// 使用的 token id
    pub token_id: VarId,
    /// 适配策略
    pub adaptation: AdaptationStrategy,
}

/// 适配策略
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdaptationStrategy {
    /// 直接使用 (token capability 匹配)
    Direct,
    /// own -> shr (临时 &)
    OwnedToShared,
    /// own -> mut (临时 &mut)
    OwnedToMut,
    /// mut -> shr (重借用 &*)
    MutToShared,
}

impl ApiCallTransition {
    pub fn description(&self) -> String {
        let args: Vec<_> = self
            .arg_bindings
            .iter()
            .map(|b| {
                let adapt = match b.adaptation {
                    AdaptationStrategy::Direct => "".to_string(),
                    AdaptationStrategy::OwnedToShared => "&".to_string(),
                    AdaptationStrategy::OwnedToMut => "&mut ".to_string(),
                    AdaptationStrategy::MutToShared => "&*".to_string(),
                };
                format!("{}v{}", adapt, b.token_id)
            })
            .collect();

        format!("call {}({})", self.api.full_path, args.join(", "))
    }
}

/// 结构性变迁
#[derive(Debug, Clone)]
pub enum StructuralTransition {
    /// Drop owned token (非借用中)
    DropOwned { token_id: VarId },
    /// 创建长期共享借用: let r = &x
    BorrowShr { owner_id: VarId, ref_id: VarId },
    /// 创建长期可变借用: let r = &mut x
    BorrowMut { owner_id: VarId, ref_id: VarId },
    /// 结束借用 (drop ref)
    EndBorrow { ref_id: VarId, owner_id: VarId },
}

impl StructuralTransition {
    pub fn description(&self) -> String {
        match self {
            StructuralTransition::DropOwned { token_id } => format!("drop v{}", token_id),
            StructuralTransition::BorrowShr { owner_id, ref_id } => {
                format!("let v{} = &v{}", ref_id, owner_id)
            }
            StructuralTransition::BorrowMut { owner_id, ref_id } => {
                format!("let v{} = &mut v{}", ref_id, owner_id)
            }
            StructuralTransition::EndBorrow { ref_id, owner_id } => {
                format!("end_borrow v{} (owner v{})", ref_id, owner_id)
            }
        }
    }
}

/// 生成所有 enabled transitions
pub fn generate_enabled_transitions(
    state: &State,
    apis: &[ApiSignature],
    _type_ctx: &TypeContext,
    max_borrow_depth: usize,
) -> Vec<Transition> {
    let mut transitions = Vec::new();

    // 1. 生成 API call transitions
    for api in apis {
        if let Some(api_trans) = try_enable_api_call(state, api) {
            transitions.extend(api_trans);
        }
    }

    // 2. 生成 structural transitions
    transitions.extend(generate_structural_transitions(state, max_borrow_depth));

    transitions
}

/// 尝试启用 API 调用 (枚举所有可能的参数绑定)
fn try_enable_api_call(state: &State, api: &ApiSignature) -> Option<Vec<Transition>> {
    let all_params = api.all_params();
    if all_params.is_empty() {
        // 无参数函数
        let return_var = Some(state.next_var_id);
        return Some(vec![Transition::ApiCall(ApiCallTransition {
            api: api.clone(),
            arg_bindings: vec![],
            return_var,
        })]);
    }

    // 递归枚举参数绑定
    let mut results = Vec::new();
    enumerate_bindings(state, api, &all_params, 0, vec![], &mut results);

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

/// 递归枚举参数绑定
fn enumerate_bindings(
    state: &State,
    api: &ApiSignature,
    params: &[ParamMode],
    param_idx: usize,
    current_bindings: Vec<ArgBinding>,
    results: &mut Vec<Transition>,
) {
    if param_idx >= params.len() {
        // 所有参数都绑定完成
        let return_var = Some(state.next_var_id + current_bindings.len() as u32);
        results.push(Transition::ApiCall(ApiCallTransition {
            api: api.clone(),
            arg_bindings: current_bindings,
            return_var,
        }));
        return;
    }

    let param = &params[param_idx];
    let candidates = find_candidate_tokens(state, param);

    // 限制候选数量 (防止组合爆炸)
    const MAX_CANDIDATES: usize = 3;
    for candidate in candidates.iter().take(MAX_CANDIDATES) {
        let mut new_bindings = current_bindings.clone();
        new_bindings.push(candidate.clone());
        enumerate_bindings(state, api, params, param_idx + 1, new_bindings, results);
    }
}

/// 查找参数的候选 tokens (包括适配)
fn find_candidate_tokens(state: &State, param: &ParamMode) -> Vec<ArgBinding> {
    let mut candidates = Vec::new();
    let type_key = param.type_key();

    match param {
        ParamMode::ByValue(_, is_copy) => {
            // 需要 owned token
            for token in state.get_tokens(type_key, Capability::Own) {
                // 检查是否可以移动 (未被借用 或 is_copy)
                if *is_copy || state.can_borrow_mut(token.id) {
                    candidates.push(ArgBinding {
                        param: param.clone(),
                        token_id: token.id,
                        adaptation: AdaptationStrategy::Direct,
                    });
                }
            }
        }
        ParamMode::SharedRef(_) => {
            // 1. 直接使用 shr token
            for token in state.get_tokens(type_key, Capability::Shr) {
                candidates.push(ArgBinding {
                    param: param.clone(),
                    token_id: token.id,
                    adaptation: AdaptationStrategy::Direct,
                });
            }

            // 2. 从 owned token 临时借用 &
            for token in state.get_tokens(type_key, Capability::Own) {
                if state.can_borrow_shr(token.id) {
                    candidates.push(ArgBinding {
                        param: param.clone(),
                        token_id: token.id,
                        adaptation: AdaptationStrategy::OwnedToShared,
                    });
                }
            }

            // 3. 从 mut token 重借用 &*
            for token in state.get_tokens(type_key, Capability::Mut) {
                candidates.push(ArgBinding {
                    param: param.clone(),
                    token_id: token.id,
                    adaptation: AdaptationStrategy::MutToShared,
                });
            }
        }
        ParamMode::MutRef(_) => {
            // 1. 直接使用 mut token
            for token in state.get_tokens(type_key, Capability::Mut) {
                candidates.push(ArgBinding {
                    param: param.clone(),
                    token_id: token.id,
                    adaptation: AdaptationStrategy::Direct,
                });
            }

            // 2. 从 owned token 临时可变借用 &mut
            for token in state.get_tokens(type_key, Capability::Own) {
                if state.can_borrow_mut(token.id) {
                    candidates.push(ArgBinding {
                        param: param.clone(),
                        token_id: token.id,
                        adaptation: AdaptationStrategy::OwnedToMut,
                    });
                }
            }
        }
    }

    candidates
}

/// 生成结构性变迁
fn generate_structural_transitions(state: &State, max_borrow_depth: usize) -> Vec<Transition> {
    let mut transitions = Vec::new();

    // 当前借用深度
    let current_depth = state
        .loan_stack
        .as_ref()
        .map(|s| s.len())
        .unwrap_or(0);

    // 1. Drop owned tokens (未被借用的)
    for tokens in state.places.values() {
        for token in tokens {
            if token.cap == Capability::Own {
                if let Some(flag) = state.owner_status.get(&token.id) {
                    if matches!(flag, BorrowFlag::Free) {
                        transitions.push(Transition::Structural(StructuralTransition::DropOwned {
                            token_id: token.id,
                        }));
                    }
                }
            }
        }
    }

    // 2. 创建新借用 (如果未超过深度限制)
    if current_depth < max_borrow_depth {
        for tokens in state.places.values() {
            for token in tokens {
                if token.cap == Capability::Own {
                    let new_ref_id = state.next_var_id;

                    // 共享借用
                    if state.can_borrow_shr(token.id) {
                        transitions.push(Transition::Structural(StructuralTransition::BorrowShr {
                            owner_id: token.id,
                            ref_id: new_ref_id,
                        }));
                    }

                    // 可变借用
                    if state.can_borrow_mut(token.id) {
                        transitions.push(Transition::Structural(StructuralTransition::BorrowMut {
                            owner_id: token.id,
                            ref_id: new_ref_id,
                        }));
                    }
                }
            }
        }
    }

    // 3. 结束借用 (drop ref)
    for tokens in state.places.values() {
        for token in tokens {
            if token.cap != Capability::Own {
                if let Some(origin) = token.origin {
                    transitions.push(Transition::Structural(StructuralTransition::EndBorrow {
                        ref_id: token.id,
                        owner_id: origin,
                    }));
                }
            }
        }
    }

    transitions
}

/// 应用 transition 到 state (生成新 state)
pub fn apply_transition(
    state: &State,
    transition: &Transition,
    type_ctx: &TypeContext,
) -> Result<State> {
    let mut new_state = state.clone();

    match transition {
        Transition::ApiCall(call) => apply_api_call(&mut new_state, call, type_ctx)?,
        Transition::Structural(structural) => apply_structural(&mut new_state, structural)?,
    }

    Ok(new_state)
}

/// 应用 API 调用
fn apply_api_call(
    state: &mut State,
    call: &ApiCallTransition,
    _type_ctx: &TypeContext,
) -> Result<()> {
    // 1. 处理参数 (consume/adapt)
    for binding in &call.arg_bindings {
        let token = state
            .find_tokens(|t| t.id == binding.token_id)
            .into_iter()
            .next()
            .context("找不到参数 token")?;

        match binding.adaptation {
            AdaptationStrategy::Direct => {
                // 直接使用
                if token.cap == Capability::Own && !token.is_copy {
                    // Move (consume)
                    state.remove_token(&token);
                    if let Some(flag) = state.owner_status.get_mut(&token.id) {
                        // 确保没有活跃借用
                        assert!(matches!(flag, BorrowFlag::Free));
                    }
                    state.owner_status.swap_remove(&token.id);
                }
                // shr/mut token 或 copy type: 不消耗
            }
            AdaptationStrategy::OwnedToShared | AdaptationStrategy::OwnedToMut => {
                // 临时借用 (在调用结束后自动结束，不需要显式建模)
                // 只需检查借用规则
                if binding.adaptation == AdaptationStrategy::OwnedToShared {
                    assert!(state.can_borrow_shr(token.id));
                } else {
                    assert!(state.can_borrow_mut(token.id));
                }
            }
            AdaptationStrategy::MutToShared => {
                // 临时重借用
            }
        }
    }

    // 2. 产生返回值
    match &call.api.return_mode {
        ReturnMode::OwnedValue(type_key, is_copy) => {
            let return_id = state.alloc_var_id();
            let return_token = Token::owned(return_id, type_key.clone(), *is_copy);
            state.add_token(return_token);
        }
        ReturnMode::SharedRef(type_key) => {
            // 返回引用：需要绑定 origin
            // 简化：尝试从第一个参数推断 origin
            let origin = call
                .arg_bindings
                .first()
                .map(|b| b.token_id)
                .context("返回引用但无法推断 origin")?;

            let return_id = state.alloc_var_id();
            let return_token = Token::shared(return_id, type_key.clone(), origin);
            state.add_token(return_token);

            // 更新 owner status
            if let Some(flag) = state.owner_status.get_mut(&origin) {
                flag.add_shr();
            }
        }
        ReturnMode::MutRef(type_key) => {
            let origin = call
                .arg_bindings
                .first()
                .map(|b| b.token_id)
                .context("返回可变引用但无法推断 origin")?;

            let return_id = state.alloc_var_id();
            let return_token = Token::mutable(return_id, type_key.clone(), origin);
            state.add_token(return_token);

            if let Some(flag) = state.owner_status.get_mut(&origin) {
                flag.set_mut_active();
            }
        }
        ReturnMode::Unit => {
            // 无返回值
        }
    }

    Ok(())
}

/// 应用结构性变迁
fn apply_structural(state: &mut State, structural: &StructuralTransition) -> Result<()> {
    match structural {
        StructuralTransition::DropOwned { token_id } => {
            let token = state
                .find_tokens(|t| t.id == *token_id)
                .into_iter()
                .next()
                .context("找不到要 drop 的 token")?;
            assert_eq!(token.cap, Capability::Own);

            state.remove_token(&token);
            state.owner_status.swap_remove(token_id);
        }
        StructuralTransition::BorrowShr { owner_id, ref_id } => {
            let owner = state
                .find_tokens(|t| t.id == *owner_id)
                .into_iter()
                .next()
                .context("找不到 owner")?;

            assert!(state.can_borrow_shr(*owner_id));

            let ref_token = Token::shared(*ref_id, owner.ty.clone(), *owner_id);
            state.add_token(ref_token);

            if let Some(flag) = state.owner_status.get_mut(owner_id) {
                flag.add_shr();
            }

            // 更新 loan stack (如果启用)
            if let Some(stack) = &mut state.loan_stack {
                stack.push(crate::model::LoanFrame {
                    kind: Capability::Shr,
                    owner: *owner_id,
                    reference: *ref_id,
                });
            }
        }
        StructuralTransition::BorrowMut { owner_id, ref_id } => {
            let owner = state
                .find_tokens(|t| t.id == *owner_id)
                .into_iter()
                .next()
                .context("找不到 owner")?;

            assert!(state.can_borrow_mut(*owner_id));

            let ref_token = Token::mutable(*ref_id, owner.ty.clone(), *owner_id);
            state.add_token(ref_token);

            if let Some(flag) = state.owner_status.get_mut(owner_id) {
                flag.set_mut_active();
            }

            if let Some(stack) = &mut state.loan_stack {
                stack.push(crate::model::LoanFrame {
                    kind: Capability::Mut,
                    owner: *owner_id,
                    reference: *ref_id,
                });
            }
        }
        StructuralTransition::EndBorrow { ref_id, owner_id } => {
            let ref_token = state
                .find_tokens(|t| t.id == *ref_id)
                .into_iter()
                .next()
                .context("找不到要结束的 borrow")?;

            let is_shr = ref_token.cap == Capability::Shr;
            state.remove_token(&ref_token);

            // 更新 owner status
            if let Some(flag) = state.owner_status.get_mut(owner_id) {
                if is_shr {
                    flag.remove_shr();
                } else {
                    flag.clear_mut();
                }
            }

            // 更新 loan stack (如果启用且满足 LIFO)
            if let Some(stack) = &mut state.loan_stack {
                if let Some(top) = stack.last() {
                    if top.reference == *ref_id {
                        stack.pop();
                    } else {
                        // 违反 LIFO (保守拒绝)
                        anyhow::bail!("违反 LIFO 借用约束");
                    }
                }
            }
        }
    }

    Ok(())
}

