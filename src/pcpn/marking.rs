//! 标记定义
//!
//! 标记表示 Petri 网的状态：每个库所中的 token 集合

use std::collections::HashMap;
use std::fmt;
use serde::{Deserialize, Serialize};
use super::place::PlaceId;
use super::types::TypeId;

/// Token (值实例)
#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    /// 类型 ID
    pub type_id: TypeId,
    /// 值 ID (唯一标识这个值实例)
    pub value_id: ValueId,
}

/// 值 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValueId(pub u64);

impl ValueId {
    pub fn new(id: u64) -> Self {
        ValueId(id)
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// 值 ID 生成器
#[derive(Debug, Clone, Default)]
pub struct ValueIdGen {
    next: u64,
}

impl ValueIdGen {
    pub fn new() -> Self {
        Self { next: 0 }
    }

    pub fn next(&mut self) -> ValueId {
        let id = ValueId(self.next);
        self.next += 1;
        id
    }
}

/// 多重集合
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MultiSet {
    /// Token -> 数量
    tokens: HashMap<Token, usize>,
}

impl MultiSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加 token
    pub fn add(&mut self, token: Token) {
        *self.tokens.entry(token).or_insert(0) += 1;
    }

    /// 添加多个相同的 token
    pub fn add_n(&mut self, token: Token, n: usize) {
        *self.tokens.entry(token).or_insert(0) += n;
    }

    /// 移除 token (返回是否成功)
    pub fn remove(&mut self, token: &Token) -> bool {
        if let Some(count) = self.tokens.get_mut(token) {
            if *count > 0 {
                *count -= 1;
                if *count == 0 {
                    self.tokens.remove(token);
                }
                return true;
            }
        }
        false
    }

    /// 获取 token 数量
    pub fn count(&self, token: &Token) -> usize {
        self.tokens.get(token).copied().unwrap_or(0)
    }

    /// 获取指定类型的所有 token
    pub fn tokens_of_type(&self, type_id: TypeId) -> Vec<&Token> {
        self.tokens
            .keys()
            .filter(|t| t.type_id == type_id)
            .collect()
    }

    /// 是否包含指定类型的 token
    pub fn has_type(&self, type_id: TypeId) -> bool {
        self.tokens.keys().any(|t| t.type_id == type_id)
    }

    /// 获取指定类型的 token 总数
    pub fn count_type(&self, type_id: TypeId) -> usize {
        self.tokens
            .iter()
            .filter(|(t, _)| t.type_id == type_id)
            .map(|(_, count)| count)
            .sum()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// 总 token 数
    pub fn total(&self) -> usize {
        self.tokens.values().sum()
    }

    /// 迭代所有 token
    pub fn iter(&self) -> impl Iterator<Item = (&Token, usize)> {
        self.tokens.iter().map(|(t, c)| (t, *c))
    }
}

/// 标记 (Marking)
///
/// 表示 Petri 网的完整状态
#[derive(Debug, Clone, Default)]
pub struct Marking {
    /// 库所 -> 多重集合
    places: HashMap<PlaceId, MultiSet>,
}

impl Marking {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加 token 到库所
    pub fn add(&mut self, place: PlaceId, token: Token) {
        self.places
            .entry(place)
            .or_insert_with(MultiSet::new)
            .add(token);
    }

    /// 添加多个 token 到库所
    pub fn add_n(&mut self, place: PlaceId, token: Token, n: usize) {
        self.places
            .entry(place)
            .or_insert_with(MultiSet::new)
            .add_n(token, n);
    }

    /// 从库所移除 token
    pub fn remove(&mut self, place: PlaceId, token: &Token) -> bool {
        if let Some(ms) = self.places.get_mut(&place) {
            ms.remove(token)
        } else {
            false
        }
    }

    /// 获取库所的多重集合
    pub fn get(&self, place: PlaceId) -> Option<&MultiSet> {
        self.places.get(&place)
    }

    /// 获取或创建库所的多重集合
    pub fn get_mut(&mut self, place: PlaceId) -> &mut MultiSet {
        self.places.entry(place).or_insert_with(MultiSet::new)
    }

    /// 检查库所是否有指定类型的 token
    pub fn has_type(&self, place: PlaceId, type_id: TypeId) -> bool {
        self.places
            .get(&place)
            .map(|ms| ms.has_type(type_id))
            .unwrap_or(false)
    }

    /// 获取库所中指定类型的 token 数量
    pub fn count_type(&self, place: PlaceId, type_id: TypeId) -> usize {
        self.places
            .get(&place)
            .map(|ms| ms.count_type(type_id))
            .unwrap_or(0)
    }

    /// 获取库所中的所有 token（展开）
    pub fn tokens_in(&self, place: PlaceId) -> Vec<Token> {
        self.places
            .get(&place)
            .map(|ms| {
                ms.iter()
                    .flat_map(|(t, count)| std::iter::repeat(t.clone()).take(count))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 计算状态的哈希值（用于去重）
    pub fn state_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        let mut places: Vec<_> = self.places.iter().collect();
        places.sort_by_key(|(p, _)| p.0);

        for (place, ms) in places {
            place.0.hash(&mut hasher);
            for (token, count) in ms.iter() {
                token.type_id.0.hash(&mut hasher);
                token.value_id.0.hash(&mut hasher);
                count.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    /// 获取所有非空库所
    pub fn non_empty_places(&self) -> Vec<PlaceId> {
        self.places
            .iter()
            .filter(|(_, ms)| !ms.is_empty())
            .map(|(p, _)| *p)
            .collect()
    }

    /// 迭代所有库所和它们的 token
    pub fn iter_places(&self) -> impl Iterator<Item = (&PlaceId, &MultiSet)> {
        self.places.iter()
    }

    /// 获取所有库所的快照
    pub fn place_summary(&self) -> Vec<(PlaceId, usize)> {
        self.places
            .iter()
            .filter(|(_, ms)| !ms.is_empty())
            .map(|(p, ms)| (*p, ms.total()))
            .collect()
    }
}

impl fmt::Display for Marking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .places
            .iter()
            .filter(|(_, ms)| !ms.is_empty())
            .map(|(p, ms)| format!("{}:{}", p, ms.total()))
            .collect();
        write!(f, "{{{}}}", parts.join(", "))
    }
}

