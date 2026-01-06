//! 状态规范化 (α-重命名) 用于去重状态空间
//!
//! 将状态中的 VarId 重命名为规范形式 (v0, v1, v2, ...)
//! 确保结构相同但变量名不同的状态能够合并

use indexmap::IndexMap;
use std::collections::HashMap;

use crate::model::{State, Token, VarId};

/// 规范化状态 (返回新状态，用于 hash/比较)
pub fn canonicalize(state: &State) -> State {
    let mut renaming = HashMap::new();
    let mut next_canonical_id: VarId = 0;

    // 分配规范 ID 的顺序：
    // 1. 先按 loan stack (如果启用) 从栈顶到栈底
    // 2. 再按 Place (TypeKey 字典序) 中的 tokens (按 capability, ty, origin 排序)

    if let Some(stack) = &state.loan_stack {
        for frame in stack.iter().rev() {
            // 栈顶优先
            if !renaming.contains_key(&frame.owner) {
                renaming.insert(frame.owner, next_canonical_id);
                next_canonical_id += 1;
            }
            if !renaming.contains_key(&frame.reference) {
                renaming.insert(frame.reference, next_canonical_id);
                next_canonical_id += 1;
            }
        }
    }

    // 收集所有 tokens 并排序
    let mut sorted_tokens: Vec<Token> = state
        .places
        .values()
        .flat_map(|tokens| tokens.iter())
        .cloned()
        .collect();

    sorted_tokens.sort_by(|a, b| {
        // 排序顺序：cap, ty, origin, id
        (a.cap, &a.ty, a.origin, a.id).cmp(&(b.cap, &b.ty, b.origin, b.id))
    });

    // 分配规范 ID
    for token in &sorted_tokens {
        if !renaming.contains_key(&token.id) {
            renaming.insert(token.id, next_canonical_id);
            next_canonical_id += 1;
        }
        if let Some(origin) = token.origin {
            if !renaming.contains_key(&origin) {
                renaming.insert(origin, next_canonical_id);
                next_canonical_id += 1;
            }
        }
    }

    // 应用重命名
    apply_renaming(state, &renaming)
}

/// 应用重命名到状态
fn apply_renaming(state: &State, renaming: &HashMap<VarId, VarId>) -> State {
    let mut new_places = IndexMap::new();

    for (type_key, tokens) in &state.places {
        let renamed_tokens: Vec<Token> = tokens
            .iter()
            .map(|t| Token {
                cap: t.cap,
                id: *renaming.get(&t.id).unwrap_or(&t.id),
                ty: t.ty.clone(),
                origin: t.origin.map(|o| *renaming.get(&o).unwrap_or(&o)),
                is_copy: t.is_copy,
                meta: t.meta.clone(),
            })
            .collect();
        new_places.insert(type_key.clone(), renamed_tokens);
    }

    let mut new_owner_status = IndexMap::new();
    for (id, flag) in &state.owner_status {
        let new_id = *renaming.get(id).unwrap_or(id);
        new_owner_status.insert(new_id, flag.clone());
    }

    let new_loan_stack = state.loan_stack.as_ref().map(|stack| {
        stack
            .iter()
            .map(|frame| crate::model::LoanFrame {
                kind: frame.kind,
                owner: *renaming.get(&frame.owner).unwrap_or(&frame.owner),
                reference: *renaming.get(&frame.reference).unwrap_or(&frame.reference),
            })
            .collect()
    });

    State {
        places: new_places,
        owner_status: new_owner_status,
        loan_stack: new_loan_stack,
        next_var_id: state.next_var_id, // 保持不变
    }
}

/// 计算状态的哈希键 (用于去重)
pub fn state_hash_key(state: &State) -> String {
    let canonical = canonicalize(state);
    format!("{:?}", canonical.places) // 简化：使用 Debug 格式
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Capability, Token};

    #[test]
    fn test_canonicalize_simple() {
        let mut state = State::new(false);

        // 添加 tokens: id=17 (own), id=42 (shr, origin=17)
        state.add_token(Token::owned(17, "User".to_string(), false));
        state.add_token(Token::shared(42, "User".to_string(), 17));

        let canonical = canonicalize(&state);

        // 应该重命名为 v0, v1
        let tokens: Vec<_> = canonical.places.values().flat_map(|t| t).collect();
        assert_eq!(tokens.len(), 2);

        // 检查重命名
        let ids: Vec<_> = tokens.iter().map(|t| t.id).collect();
        assert!(ids.contains(&0));
        assert!(ids.contains(&1));
    }
}

