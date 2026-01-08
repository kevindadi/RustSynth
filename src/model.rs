//! 核心数据模型：Token, State, OwnerStatus, LoanStack
//!
//! 资源网/状态机结构：
//! - Place(TypeKey): multiset<Token>
//! - Token 携带 capability (own/shr/mut) + 变量 id + 借用关系
//! - OwnerStatus 跟踪借用约束 (Free/ShrCount/MutActive)
//! - LoanStack 用于 pushdown LIFO 借用栈

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 变量 ID (用于生成代码和借用追踪)
pub type VarId = u32;

/// 类型键 (全称路径字符串, 例如: "std::vec::Vec", "mycrate::model::User")
pub type TypeKey = String;

/// Capability (所有权/借用模式)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// 拥有所有权 (owned)
    Own,
    /// 共享借用 (&T)
    Shr,
    /// 可变借用 (&mut T)
    Mut,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Shr => write!(f, "shr"),
            Capability::Mut => write!(f, "mut"),
        }
    }
}

/// Token (着色 token)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    /// Capability
    pub cap: Capability,
    /// 变量 ID
    pub id: VarId,
    /// 类型键 (base type, 不包含引用)
    pub ty: TypeKey,
    /// 如果是借用 (shr/mut)，指向它借用自哪个 owned token
    pub origin: Option<VarId>,
    /// 是否是 Copy 类型
    pub is_copy: bool,
    /// 可选元数据 (用于 debug/trace)
    pub meta: Option<String>,
}

impl Token {
    /// 创建一个 owned token
    pub fn owned(id: VarId, ty: TypeKey, is_copy: bool) -> Self {
        Token {
            cap: Capability::Own,
            id,
            ty,
            origin: None,
            is_copy,
            meta: None,
        }
    }

    /// 创建一个共享借用 token
    pub fn shared(id: VarId, ty: TypeKey, origin: VarId) -> Self {
        Token {
            cap: Capability::Shr,
            id,
            ty,
            origin: Some(origin),
            is_copy: true, // 引用总是 Copy
            meta: None,
        }
    }

    /// 创建一个可变借用 token
    pub fn mutable(id: VarId, ty: TypeKey, origin: VarId) -> Self {
        Token {
            cap: Capability::Mut,
            id,
            ty,
            origin: Some(origin),
            is_copy: false, // &mut T 不是 Copy
            meta: None,
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}({})", self.cap, self.ty)?;
        if let Some(origin) = self.origin {
            write!(f, "@v{}←v{}", self.id, origin)?;
        } else {
            write!(f, "@v{}", self.id)?;
        }
        Ok(())
    }
}

/// 借用标记 (用于跟踪 owner 的借用状态)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BorrowFlag {
    /// 自由状态 (无借用)
    Free,
    /// 有 n 个共享借用活跃
    ShrCount(usize),
    /// 有一个可变借用活跃
    MutActive,
}

impl BorrowFlag {
    /// 检查是否可以创建共享借用
    pub fn can_borrow_shr(&self) -> bool {
        !matches!(self, BorrowFlag::MutActive)
    }

    /// 检查是否可以创建可变借用
    pub fn can_borrow_mut(&self) -> bool {
        matches!(self, BorrowFlag::Free)
    }

    /// 添加一个共享借用
    pub fn add_shr(&mut self) {
        match self {
            BorrowFlag::Free => *self = BorrowFlag::ShrCount(1),
            BorrowFlag::ShrCount(n) => *self = BorrowFlag::ShrCount(*n + 1),
            BorrowFlag::MutActive => panic!("Cannot add shr when mut is active"),
        }
    }

    /// 移除一个共享借用
    pub fn remove_shr(&mut self) {
        match self {
            BorrowFlag::ShrCount(1) => *self = BorrowFlag::Free,
            BorrowFlag::ShrCount(n) => *self = BorrowFlag::ShrCount(*n - 1),
            _ => panic!("No shr to remove"),
        }
    }

    /// 设置可变借用活跃
    pub fn set_mut_active(&mut self) {
        assert!(matches!(self, BorrowFlag::Free));
        *self = BorrowFlag::MutActive;
    }

    /// 清除可变借用
    pub fn clear_mut(&mut self) {
        assert!(matches!(self, BorrowFlag::MutActive));
        *self = BorrowFlag::Free;
    }
}

/// Loan 栈帧 (用于 pushdown 模式)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoanFrame {
    /// 借用类型
    pub kind: Capability, // Shr or Mut
    /// 被借用的 owner
    pub owner: VarId,
    /// 借用引用的 id
    pub reference: VarId,
}

/// 状态 (包含所有 places + 借用状态 + 可选的 loan stack)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    /// Places: TypeKey -> Vec<Token> (multiset)
    pub places: IndexMap<TypeKey, Vec<Token>>,

    /// Owner 借用状态: VarId -> BorrowFlag
    pub owner_status: IndexMap<VarId, BorrowFlag>,

    /// 可选: Loan Stack (用于 pushdown LIFO 借用)
    pub loan_stack: Option<Vec<LoanFrame>>,

    /// 下一个可用的变量 ID
    pub next_var_id: VarId,
}

impl State {
    /// 创建空状态
    pub fn new(enable_loan_stack: bool) -> Self {
        State {
            places: IndexMap::new(),
            owner_status: IndexMap::new(),
            loan_stack: if enable_loan_stack {
                Some(Vec::new())
            } else {
                None
            },
            next_var_id: 0,
        }
    }

    /// 分配新变量 ID
    pub fn alloc_var_id(&mut self) -> VarId {
        let id = self.next_var_id;
        self.next_var_id += 1;
        id
    }

    /// 添加 token 到对应的 place
    pub fn add_token(&mut self, token: Token) {
        let place = self.places.entry(token.ty.clone()).or_insert_with(Vec::new);
        place.push(token.clone());

        // 如果是 owned token，初始化其 owner status
        if token.cap == Capability::Own {
            self.owner_status
                .entry(token.id)
                .or_insert(BorrowFlag::Free);
        }
    }

    /// 移除指定的 token
    pub fn remove_token(&mut self, token: &Token) -> bool {
        if let Some(place) = self.places.get_mut(&token.ty) {
            if let Some(pos) = place.iter().position(|t| t.id == token.id) {
                place.remove(pos);
                // 如果 place 为空，可选择移除（为了规范化，保留空 place）
                return true;
            }
        }
        false
    }

    /// 查找所有符合条件的 tokens
    pub fn find_tokens<F>(&self, predicate: F) -> Vec<Token>
    where
        F: Fn(&Token) -> bool,
    {
        self.places
            .values()
            .flat_map(|tokens| tokens.iter())
            .filter(|t| predicate(t))
            .cloned()
            .collect()
    }

    /// 获取指定类型和 capability 的 tokens
    pub fn get_tokens(&self, ty: &TypeKey, cap: Capability) -> Vec<Token> {
        self.places
            .get(ty)
            .map(|tokens| {
                tokens
                    .iter()
                    .filter(|t| t.cap == cap)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 检查 owner 是否可以创建共享借用
    pub fn can_borrow_shr(&self, owner_id: VarId) -> bool {
        self.owner_status
            .get(&owner_id)
            .map(|flag| flag.can_borrow_shr())
            .unwrap_or(false)
    }

    /// 检查 owner 是否可以创建可变借用
    pub fn can_borrow_mut(&self, owner_id: VarId) -> bool {
        self.owner_status
            .get(&owner_id)
            .map(|flag| flag.can_borrow_mut())
            .unwrap_or(false)
    }

    /// 统计总 token 数
    pub fn total_tokens(&self) -> usize {
        self.places.values().map(|v| v.len()).sum()
    }

    /// 检查是否达到封闭状态 (无未结束的借用)
    pub fn is_closed(&self) -> bool {
        // 所有 owner 都是 Free 状态
        self.owner_status
            .values()
            .all(|flag| matches!(flag, BorrowFlag::Free))
            // 如果启用了 loan stack，stack 必须为空
            && self
                .loan_stack
                .as_ref()
                .map(|stack| stack.is_empty())
                .unwrap_or(true)
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "State {{")?;
        writeln!(f, "  Places:")?;
        for (ty, tokens) in &self.places {
            if !tokens.is_empty() {
                writeln!(f, "    {}: [", ty)?;
                for token in tokens {
                    writeln!(f, "      {},", token)?;
                }
                writeln!(f, "    ]")?;
            }
        }
        writeln!(f, "  OwnerStatus:")?;
        for (id, flag) in &self.owner_status {
            writeln!(f, "    v{}: {:?}", id, flag)?;
        }
        if let Some(stack) = &self.loan_stack {
            if !stack.is_empty() {
                writeln!(f, "  LoanStack:")?;
                for frame in stack.iter().rev() {
                    writeln!(
                        f,
                        "    {:?}: v{} ← v{}",
                        frame.kind, frame.reference, frame.owner
                    )?;
                }
            }
        }
        writeln!(f, "  next_var_id: {}", self.next_var_id)?;
        write!(f, "}}")
    }
}

