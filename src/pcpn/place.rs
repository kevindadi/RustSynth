//! 库所定义
//!
//! 库所表示类型的存储位置，token 在库所中累积

use std::fmt;
use serde::{Deserialize, Serialize};
use super::types::TypeId;

/// 库所 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlaceId(pub u32);

impl PlaceId {
    pub fn new(id: u32) -> Self {
        PlaceId(id)
    }
}

impl fmt::Display for PlaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// 库所种类
///
/// 区分不同所有权状态的库所
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaceKind {
    /// 所有权库所 p_own,τ
    /// 持有类型 τ 的所有权
    Own,

    /// 共享引用库所 p_shr,τ
    /// 持有 &τ 类型的 token
    SharedRef,

    /// 可变引用库所 p_mut,τ
    /// 持有 &mut τ 类型的 token
    MutRef,

    /// 值库所 p_val,τ
    /// 用于返回引用类型的函数输出
    Val,

    /// 自动构造库所
    /// 可以无限产生 token（用于基本类型）
    AutoConstruct,

    /// 目标库所
    /// 搜索的目标状态
    Goal,
}

/// 库所定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Place {
    /// 库所 ID
    pub id: PlaceId,
    /// 库所名称（用于可视化和调试）
    pub name: String,
    /// 库所种类
    pub kind: PlaceKind,
    /// 关联的类型 ID
    pub type_id: TypeId,
    /// 容量限制 (None = 无限)
    pub capacity: Option<usize>,
}

impl Place {
    /// 创建所有权库所
    pub fn own(id: PlaceId, name: String, type_id: TypeId) -> Self {
        Place {
            id,
            name,
            kind: PlaceKind::Own,
            type_id,
            capacity: None,
        }
    }

    /// 创建共享引用库所
    pub fn shared_ref(id: PlaceId, name: String, type_id: TypeId) -> Self {
        Place {
            id,
            name,
            kind: PlaceKind::SharedRef,
            type_id,
            capacity: None,
        }
    }

    /// 创建可变引用库所
    pub fn mut_ref(id: PlaceId, name: String, type_id: TypeId) -> Self {
        Place {
            id,
            name,
            kind: PlaceKind::MutRef,
            type_id,
            capacity: None,
        }
    }

    /// 创建自动构造库所
    pub fn auto_construct(id: PlaceId, name: String, type_id: TypeId) -> Self {
        Place {
            id,
            name,
            kind: PlaceKind::AutoConstruct,
            type_id,
            capacity: None,
        }
    }

    /// 创建目标库所
    pub fn goal(id: PlaceId, name: String, type_id: TypeId) -> Self {
        Place {
            id,
            name,
            kind: PlaceKind::Goal,
            type_id,
            capacity: Some(1),
        }
    }

    /// 是否可以无限产生 token
    pub fn is_source(&self) -> bool {
        self.kind == PlaceKind::AutoConstruct
    }
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind_str = match self.kind {
            PlaceKind::Own => "own",
            PlaceKind::SharedRef => "shr",
            PlaceKind::MutRef => "mut",
            PlaceKind::Val => "val",
            PlaceKind::AutoConstruct => "auto",
            PlaceKind::Goal => "goal",
        };
        write!(f, "{}_{}", kind_str, self.name)
    }
}

