//! 弧定义
//!
//! 弧连接库所和变迁，定义 token 流动和颜色约束

use serde::{Deserialize, Serialize};
use super::place::PlaceId;
use super::transition::TransitionId;
use super::types::TypeId;

/// 弧 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArcId(pub u32);

/// 弧种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArcKind {
    /// 普通弧：消耗/产生 token
    Normal,
    /// 读弧：检查但不消耗（用于共享引用）
    Read,
    /// 抑制弧：当库所为空时使能
    Inhibitor,
    /// 自循环弧：消耗后立即产生（用于 Copy 类型）
    SelfLoop,
}

/// 弧定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arc {
    /// 弧 ID
    pub id: ArcId,
    /// 是否是输入弧（库所 -> 变迁）
    pub is_input: bool,
    /// 源（输入弧：库所 ID；输出弧：变迁 ID）
    pub source_place: Option<PlaceId>,
    pub source_transition: Option<TransitionId>,
    /// 目标（输入弧：变迁 ID；输出弧：库所 ID）
    pub target_place: Option<PlaceId>,
    pub target_transition: Option<TransitionId>,
    /// 弧种类
    pub kind: ArcKind,
    /// 权重（token 数量）
    pub weight: usize,
    /// 颜色约束（类型约束）
    pub color_constraint: Option<TypeId>,
    /// 标签（参数名等）
    pub label: Option<String>,
}

impl Arc {
    /// 创建输入弧（库所 -> 变迁）
    pub fn input(
        id: ArcId,
        place: PlaceId,
        transition: TransitionId,
        kind: ArcKind,
        weight: usize,
        color: Option<TypeId>,
    ) -> Self {
        Arc {
            id,
            is_input: true,
            source_place: Some(place),
            source_transition: None,
            target_place: None,
            target_transition: Some(transition),
            kind,
            weight,
            color_constraint: color,
            label: None,
        }
    }

    /// 创建输出弧（变迁 -> 库所）
    pub fn output(
        id: ArcId,
        transition: TransitionId,
        place: PlaceId,
        kind: ArcKind,
        weight: usize,
        color: Option<TypeId>,
    ) -> Self {
        Arc {
            id,
            is_input: false,
            source_place: None,
            source_transition: Some(transition),
            target_place: Some(place),
            target_transition: None,
            kind,
            weight,
            color_constraint: color,
            label: None,
        }
    }

    /// 设置标签
    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }

    /// 获取源库所（输入弧）
    pub fn from_place(&self) -> Option<PlaceId> {
        if self.is_input {
            self.source_place
        } else {
            None
        }
    }

    /// 获取目标库所（输出弧）
    pub fn to_place(&self) -> Option<PlaceId> {
        if !self.is_input {
            self.target_place
        } else {
            None
        }
    }

    /// 获取关联的变迁
    pub fn transition(&self) -> TransitionId {
        if self.is_input {
            self.target_transition.unwrap()
        } else {
            self.source_transition.unwrap()
        }
    }
}

