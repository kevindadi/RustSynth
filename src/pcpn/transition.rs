//! 变迁定义
//!
//! 变迁表示操作，分为结构变迁和签名诱导变迁

use std::fmt;
use serde::{Deserialize, Serialize};
use super::types::TypeId;
use super::stack::StackOp;

/// 变迁 ID
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TransitionId(pub u32);

impl TransitionId {
    pub fn new(id: u32) -> Self {
        TransitionId(id)
    }
}

impl fmt::Display for TransitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t{}", self.0)
    }
}

/// 变迁种类
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// 结构变迁 (S1-S16)
    Structural(StructuralKind),

    /// 签名诱导变迁 (API 调用)
    Signature(SignatureInfo),

    /// 自动构造变迁
    AutoConstruct {
        /// 目标类型
        target_type: TypeId,
        /// 构造方法
        method: AutoConstructMethod,
    },

    /// 展开变迁 (Result/Option)
    Unwrap(UnwrapKind),
}

/// 结构变迁种类 (S1-S18)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructuralKind {
    /// S1: Move - 所有权移动
    Move,

    /// S2: CopyUse - Copy 类型使用（自循环，不消耗原值）
    CopyUse,

    /// S3: DupCopy - Copy 类型复制（有限次，产生新 token）
    DupCopy,

    /// S4: CloneUse - Clone 类型使用（调用 clone() 方法）
    /// 与 CopyUse 不同，Clone 需要显式调用 .clone()
    CloneUse,

    /// S5: DupClone - Clone 类型复制（有限次，产生新 token）
    DupClone,

    /// S6: DropOwn - 销毁所有权
    DropOwn,

    /// S7: BorrowShrOwn - 从所有权创建共享借用
    BorrowShrOwn,

    /// S8: BorrowShrFrz - 从共享借用创建新共享借用（冻结）
    BorrowShrFrz,

    /// S9: BorrowMut - 创建可变借用
    BorrowMut,

    /// S10: EndMut - 结束可变借用
    EndMut,

    /// S11: EndShrKeep - 结束共享借用（保留其他借用）
    EndShrKeep,

    /// S12: EndShrLast - 结束最后一个共享借用
    EndShrLast,

    /// S13: ProjMove - 字段移动投影
    ProjMove,

    /// S14: ProjShr - 字段共享引用投影（重借用）
    ProjShr,

    /// S15: ProjMut - 字段可变引用投影（重借用）
    ProjMut,

    /// S16: EndProjMut - 结束字段可变投影
    EndProjMut,

    /// S17: ImplWitness - Trait 实现见证
    ImplWitness,

    /// S18: AssocCast - 关联类型转换
    AssocCast,
}

impl StructuralKind {
    /// 获取名称
    pub fn name(&self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::CopyUse => "CopyUse",
            Self::DupCopy => "DupCopy",
            Self::CloneUse => "CloneUse",
            Self::DupClone => "DupClone",
            Self::DropOwn => "DropOwn",
            Self::BorrowShrOwn => "BorrowShrOwn",
            Self::BorrowShrFrz => "BorrowShrFrz",
            Self::BorrowMut => "BorrowMut",
            Self::EndMut => "EndMut",
            Self::EndShrKeep => "EndShrKeep",
            Self::EndShrLast => "EndShrLast",
            Self::ProjMove => "ProjMove",
            Self::ProjShr => "ProjShr",
            Self::ProjMut => "ProjMut",
            Self::EndProjMut => "EndProjMut",
            Self::ImplWitness => "ImplWitness",
            Self::AssocCast => "AssocCast",
        }
    }

    /// 获取默认的栈操作
    pub fn default_stack_op(&self) -> StackOp {
        match self {
            Self::BorrowShrOwn | Self::BorrowMut => StackOp::Push,
            Self::EndMut | Self::EndShrLast | Self::EndShrKeep => StackOp::Pop,
            Self::ProjMove | Self::ProjShr | Self::ProjMut => StackOp::Push,
            Self::EndProjMut => StackOp::Pop,
            _ => StackOp::None,
        }
    }

    /// 是否需要 Copy trait
    pub fn requires_copy(&self) -> bool {
        matches!(self, Self::CopyUse | Self::DupCopy)
    }

    /// 是否需要 Clone trait
    pub fn requires_clone(&self) -> bool {
        matches!(self, Self::CloneUse | Self::DupClone)
    }

    /// 是否是投影操作（字段访问/重借用）
    pub fn is_projection(&self) -> bool {
        matches!(self, Self::ProjMove | Self::ProjShr | Self::ProjMut | Self::EndProjMut)
    }
}

/// 签名信息（API 调用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureInfo {
    /// 完整路径（如 "base64::engine::GeneralPurpose"）
    pub path: String,
    /// 函数/方法名
    pub name: String,
    /// 所属类型名称（如 "GeneralPurpose"）
    pub owner_type: Option<String>,
    /// 所属类型完整路径
    pub owner_path: Option<String>,
    /// 参数信息
    pub params: Vec<ParamInfo>,
    /// 返回类型
    pub return_type: Option<TypeId>,
    /// 返回类型名称（用于代码生成）
    pub return_type_name: Option<String>,
    /// 是否是 const fn
    pub is_const: bool,
    /// 是否是 async fn
    pub is_async: bool,
    /// 是否是 unsafe fn
    pub is_unsafe: bool,
    /// 是否是方法（有 self 参数）
    pub is_method: bool,
    /// self 参数的传递方式（如果是方法）
    pub self_param: Option<SelfKind>,
    /// 是否有外部依赖（参数类型来自外部 crate）
    pub has_external_deps: bool,
    /// 外部类型列表（需要 LLM 辅助处理）
    pub external_types: Vec<String>,
}

/// 参数信息
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamInfo {
    /// 参数名
    pub name: String,
    /// 参数类型 ID
    pub type_id: TypeId,
    /// 参数类型名称（用于代码生成）
    pub type_name: String,
    /// 传递方式
    pub passing: ParamPassing,
    /// 是否是外部类型
    pub is_external: bool,
}

/// 参数传递方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParamPassing {
    /// 按值传递（所有权转移）
    ByValue,
    /// 共享引用 &T
    ByRef,
    /// 可变引用 &mut T
    ByMutRef,
}

/// self 参数种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SelfKind {
    /// self (所有权)
    Owned,
    /// &self
    Ref,
    /// &mut self
    MutRef,
}

/// 自动构造方法
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AutoConstructMethod {
    /// 基本类型字面量
    Literal,
    /// Default::default()
    Default,
    /// Clone::clone()
    Clone,
    /// Copy（隐式）
    Copy,
    /// const fn 调用
    ConstFn { path: String },
}

/// 展开种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnwrapKind {
    /// Result::unwrap() -> Ok value
    ResultOk,
    /// Result -> Err value
    ResultErr,
    /// Option::unwrap() -> Some value
    OptionSome,
    /// Option -> None
    OptionNone,
}

/// 变迁定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    /// 变迁 ID
    pub id: TransitionId,
    /// 变迁名称
    pub name: String,
    /// 变迁种类
    pub kind: TransitionKind,
    /// 栈操作
    pub stack_op: StackOp,
    /// 优先级（用于搜索时的排序）
    pub priority: i32,
    /// DupCopy 的预算（仅用于 S3）
    pub dup_budget: Option<usize>,
}

impl Transition {
    /// 创建结构变迁
    pub fn structural(
        id: TransitionId,
        kind: StructuralKind,
        type_id: TypeId,
    ) -> Self {
        Transition {
            id,
            name: format!("{}_{}", kind.name(), type_id),
            kind: TransitionKind::Structural(kind),
            stack_op: kind.default_stack_op(),
            priority: 0,
            dup_budget: if kind == StructuralKind::DupCopy { Some(3) } else { None },
        }
    }

    /// 创建签名诱导变迁
    pub fn signature(id: TransitionId, info: SignatureInfo) -> Self {
        Transition {
            id,
            name: info.name.clone(),
            kind: TransitionKind::Signature(info),
            stack_op: StackOp::None,
            priority: 10, // API 调用优先级更高
            dup_budget: None,
        }
    }

    /// 创建自动构造变迁
    pub fn auto_construct(
        id: TransitionId,
        target_type: TypeId,
        method: AutoConstructMethod,
    ) -> Self {
        Transition {
            id,
            name: format!("auto_construct_{}", target_type),
            kind: TransitionKind::AutoConstruct { target_type, method },
            stack_op: StackOp::None,
            priority: -10, // 自动构造优先级较低
            dup_budget: None,
        }
    }

    /// 是否是 API 调用
    pub fn is_api_call(&self) -> bool {
        matches!(self.kind, TransitionKind::Signature(_))
    }

    /// 获取签名信息（如果是 API 调用）
    pub fn signature_info(&self) -> Option<&SignatureInfo> {
        match &self.kind {
            TransitionKind::Signature(info) => Some(info),
            _ => None,
        }
    }
}

impl fmt::Display for Transition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

