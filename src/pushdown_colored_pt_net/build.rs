//! 变迁构建器
//!
//! 实现所有结构变迁 (S1..S16) 和签名诱导变迁

use crate::pushdown_colored_pt_net::runtime::*;
use crate::pushdown_colored_pt_net::types::*;
use crate::pushdown_colored_pt_net::env::Env;

/// 结构变迁类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuralTransitionKind {
    /// S1: Move
    Move,
    /// S2: CopyUse
    CopyUse,
    /// S3: DupCopy (可选,有界)
    DupCopy,
    /// S4: DropOwn
    DropOwn,
    /// S5: BorrowShrOwn
    BorrowShrOwn,
    /// S6: BorrowShrFrz
    BorrowShrFrz,
    /// S7: BorrowMut
    BorrowMut,
    /// S8: EndMut
    EndMut,
    /// S9: EndShrKeep
    EndShrKeep,
    /// S10: EndShrLast
    EndShrLast,
    /// S11: ProjMove
    ProjMove,
    /// S12: ProjShr
    ProjShr,
    /// S13: ProjMut
    ProjMut,
    /// S14: EndProjMut
    EndProjMut,
    /// S15: Impl witness
    ImplWitness,
    /// S16: AssocCast
    AssocCast,
}

/// 结构变迁实现
#[derive(Debug)]
pub struct StructuralTransition {
    id: TransitionId,
    kind: StructuralTransitionKind,
    input_arcs: Vec<InputArc>,
    output_arcs: Vec<OutputArc>,
    /// DupCopy 的预算 (仅用于 DupCopy)
    dup_budget: Option<usize>,
}

impl StructuralTransition {
    /// 创建 Move 变迁 (S1)
    /// 
    /// Move: 消耗 p_own,τ 中的一个 token,产生到目标库所的 token
    pub fn new_move(id: TransitionId, from_place: Place, to_place: Place, ty: TypeExpr) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::Move,
            input_arcs: vec![InputArc {
                place: from_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty.clone()),
            }],
            output_arcs: vec![OutputArc {
                place: to_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new({
                    let ty_clone = ty.clone();
                    move |_marking, _stack, value_id_gen| {
                        // 生成新值 ID
                        Color::new(ty_clone.clone(), value_id_gen.next())
                    }
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 CopyUse 变迁 (S2)
    /// 
    /// CopyUse: 对于 Copy 类型,消耗 p_own,τ 中的一个 token,产生到目标库所的 token
    /// 如果类型是 Copy,则自循环输出 token 回到 p_own,τ
    pub fn new_copy_use(
        id: TransitionId,
        from_place: Place,
        to_place: Place,
        ty: TypeExpr,
        is_copy: bool,
    ) -> Self {
        let mut output_arcs = vec![OutputArc {
            place: to_place,
            weight: ArcWeight::new(1),
                color_gen: Some(Box::new({
                    let ty_clone = ty.clone();
                    move |_marking, _stack, value_id_gen| Color::new(ty_clone.clone(), value_id_gen.next())
                })),
        }];

        // 如果是 Copy 类型,添加自循环
        if is_copy {
            output_arcs.push(OutputArc {
                place: from_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new({
                    let ty_clone = ty.clone();
                            move |_marking, _stack, _value_id_gen| {
                                // 找到消耗的 token 并返回
                                // 这里简化处理: 使用选择中的 token
                                // 实际应该从 choice 中获取
                                Color::new(ty_clone.clone(), ValueId::new(0))
                            }
                })),
            });
        }

        StructuralTransition {
            id,
            kind: StructuralTransitionKind::CopyUse,
            input_arcs: vec![InputArc {
                place: from_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty),
            }],
            output_arcs,
            dup_budget: None,
        }
    }

    /// 创建 DupCopy 变迁 (S3)
    /// 
    /// DupCopy: 对于 Copy 类型,复制 token (有界)
    pub fn new_dup_copy(
        id: TransitionId,
        place: Place,
        ty: TypeExpr,
        budget: usize,
    ) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::DupCopy,
            input_arcs: vec![InputArc {
                place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty.clone()),
            }],
            output_arcs: vec![OutputArc {
                place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: Some(budget),
        }
    }

    /// 创建 DropOwn 变迁 (S4)
    /// 
    /// DropOwn: 消耗 p_own,τ 中的一个 token (drop)
    pub fn new_drop_own(id: TransitionId, place: Place, ty: TypeExpr) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::DropOwn,
            input_arcs: vec![InputArc {
                place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty),
            }],
            output_arcs: vec![],
            dup_budget: None,
        }
    }

    /// 创建 BorrowShrOwn 变迁 (S5)
    /// 
    /// BorrowShrOwn: 消耗 p_own,τ,产生 p_shr,&τ 和栈帧
    pub fn new_borrow_shr_own(
        id: TransitionId,
        own_place: Place,
        shr_place: Place,
        ty: TypeExpr,
    ) -> Self {
        let ref_ty = TypeExpr::shared_ref(ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::BorrowShrOwn,
            input_arcs: vec![InputArc {
                place: own_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty),
            }],
            output_arcs: vec![OutputArc {
                place: shr_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 BorrowShrFrz 变迁 (S6)
    /// 
    /// BorrowShrFrz: 从 p_shr,&τ 产生新的共享借用 (冻结)
    pub fn new_borrow_shr_frz(
        id: TransitionId,
        shr_place: Place,
        ty: TypeExpr,
    ) -> Self {
        let ref_ty = TypeExpr::shared_ref(ty);
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::BorrowShrFrz,
            input_arcs: vec![InputArc {
                place: shr_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ref_ty.clone()),
            }],
            output_arcs: vec![OutputArc {
                place: shr_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 BorrowMut 变迁 (S7)
    /// 
    /// BorrowMut: 消耗 p_own,τ,产生 p_mut,&mut τ 和栈帧
    pub fn new_borrow_mut(
        id: TransitionId,
        own_place: Place,
        mut_place: Place,
        ty: TypeExpr,
    ) -> Self {
        let ref_ty = TypeExpr::mut_ref(ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::BorrowMut,
            input_arcs: vec![InputArc {
                place: own_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty),
            }],
            output_arcs: vec![OutputArc {
                place: mut_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 EndMut 变迁 (S8)
    /// 
    /// EndMut: 结束可变借用,消耗 p_mut,&mut τ,恢复 p_own,τ,弹出栈帧
    pub fn new_end_mut(
        id: TransitionId,
        mut_place: Place,
        own_place: Place,
        ty: TypeExpr,
    ) -> Self {
        let ref_ty = TypeExpr::mut_ref(ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::EndMut,
            input_arcs: vec![InputArc {
                place: mut_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ref_ty),
            }],
            output_arcs: vec![OutputArc {
                place: own_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 EndShrKeep 变迁 (S9)
    /// 
    /// EndShrKeep: 结束共享借用但保留其他借用,消耗 p_shr,&τ,弹出栈帧
    pub fn new_end_shr_keep(id: TransitionId, shr_place: Place, ty: TypeExpr) -> Self {
        let ref_ty = TypeExpr::shared_ref(ty);
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::EndShrKeep,
            input_arcs: vec![InputArc {
                place: shr_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ref_ty),
            }],
            output_arcs: vec![],
            dup_budget: None,
        }
    }

    /// 创建 EndShrLast 变迁 (S10)
    /// 
    /// EndShrLast: 结束最后一个共享借用,消耗 p_shr,&τ,恢复 p_own,τ,弹出栈帧
    pub fn new_end_shr_last(
        id: TransitionId,
        shr_place: Place,
        own_place: Place,
        ty: TypeExpr,
    ) -> Self {
        let ref_ty = TypeExpr::shared_ref(ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::EndShrLast,
            input_arcs: vec![InputArc {
                place: shr_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ref_ty),
            }],
            output_arcs: vec![OutputArc {
                place: own_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 ProjMove 变迁 (S11)
    /// 
    /// ProjMove: 字段移动投影,消耗 p_own,Struct,产生 p_own,FieldTy,压入栈帧
    pub fn new_proj_move(
        id: TransitionId,
        struct_place: Place,
        field_place: Place,
        struct_ty: TypeExpr,
        field_ty: TypeExpr,
        field_name: String,
    ) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::ProjMove,
            input_arcs: vec![InputArc {
                place: struct_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(struct_ty),
            }],
            output_arcs: vec![OutputArc {
                place: field_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new({
                    let field_ty_clone = field_ty.clone();
                    move |_marking, _stack, value_id_gen| Color::new(field_ty_clone.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 ProjShr 变迁 (S12)
    /// 
    /// ProjShr: 字段共享引用投影,从 p_shr,&Struct 产生 p_shr,&FieldTy,压入栈帧
    pub fn new_proj_shr(
        id: TransitionId,
        struct_place: Place,
        field_place: Place,
        struct_ty: TypeExpr,
        field_ty: TypeExpr,
        field_name: String,
    ) -> Self {
        let struct_ref_ty = TypeExpr::shared_ref(struct_ty);
        let field_ref_ty = TypeExpr::shared_ref(field_ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::ProjShr,
            input_arcs: vec![InputArc {
                place: struct_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(struct_ref_ty),
            }],
            output_arcs: vec![OutputArc {
                place: field_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(field_ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 ProjMut 变迁 (S13)
    /// 
    /// ProjMut: 字段可变引用投影,消耗 p_mut,&mut Struct,产生 p_mut,&mut FieldTy,压入栈帧
    pub fn new_proj_mut(
        id: TransitionId,
        struct_place: Place,
        field_place: Place,
        struct_ty: TypeExpr,
        field_ty: TypeExpr,
        field_name: String,
    ) -> Self {
        let struct_ref_ty = TypeExpr::mut_ref(struct_ty);
        let field_ref_ty = TypeExpr::mut_ref(field_ty.clone());
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::ProjMut,
            input_arcs: vec![InputArc {
                place: struct_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(struct_ref_ty),
            }],
            output_arcs: vec![OutputArc {
                place: field_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(field_ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 EndProjMut 变迁 (S14)
    /// 
    /// EndProjMut: 结束字段可变引用投影,恢复 p_mut,&mut Struct,弹出栈帧
    pub fn new_end_proj_mut(
        id: TransitionId,
        field_place: Place,
        struct_place: Place,
        struct_ty: TypeExpr,
        field_ty: TypeExpr,
        field_name: String,
    ) -> Self {
        let struct_ref_ty = TypeExpr::mut_ref(struct_ty);
        let field_ref_ty = TypeExpr::mut_ref(field_ty);
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::EndProjMut,
            input_arcs: vec![InputArc {
                place: field_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(field_ref_ty),
            }],
            output_arcs: vec![OutputArc {
                place: struct_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(struct_ref_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 ImplWitness 变迁 (S15)
    /// 
    /// ImplWitness: 实现见证,用于 trait 实现
    pub fn new_impl_witness(
        id: TransitionId,
        from_place: Place,
        to_place: Place,
        ty: TypeExpr,
        trait_name: String,
    ) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::ImplWitness,
            input_arcs: vec![InputArc {
                place: from_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(ty.clone()),
            }],
            output_arcs: vec![OutputArc {
                place: to_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    /// 创建 AssocCast 变迁 (S16)
    /// 
    /// AssocCast: 关联类型转换
    pub fn new_assoc_cast(
        id: TransitionId,
        from_place: Place,
        to_place: Place,
        from_ty: TypeExpr,
        to_ty: TypeExpr,
        trait_name: String,
        assoc_name: String,
    ) -> Self {
        StructuralTransition {
            id,
            kind: StructuralTransitionKind::AssocCast,
            input_arcs: vec![InputArc {
                place: from_place,
                weight: ArcWeight::new(1),
                color_constraint: Some(from_ty.clone()),
            }],
            output_arcs: vec![OutputArc {
                place: to_place,
                weight: ArcWeight::new(1),
                color_gen: Some(Box::new(move |_marking, _stack, value_id_gen| {
                    Color::new(to_ty.clone(), value_id_gen.next())
                })),
            }],
            dup_budget: None,
        }
    }

    fn kind_name(&self) -> &str {
        match self.kind {
            StructuralTransitionKind::Move => "Move",
            StructuralTransitionKind::CopyUse => "CopyUse",
            StructuralTransitionKind::DupCopy => "DupCopy",
            StructuralTransitionKind::DropOwn => "DropOwn",
            StructuralTransitionKind::BorrowShrOwn => "BorrowShrOwn",
            StructuralTransitionKind::BorrowShrFrz => "BorrowShrFrz",
            StructuralTransitionKind::BorrowMut => "BorrowMut",
            StructuralTransitionKind::EndMut => "EndMut",
            StructuralTransitionKind::EndShrKeep => "EndShrKeep",
            StructuralTransitionKind::EndShrLast => "EndShrLast",
            StructuralTransitionKind::ProjMove => "ProjMove",
            StructuralTransitionKind::ProjShr => "ProjShr",
            StructuralTransitionKind::ProjMut => "ProjMut",
            StructuralTransitionKind::EndProjMut => "EndProjMut",
            StructuralTransitionKind::ImplWitness => "ImplWitness",
            StructuralTransitionKind::AssocCast => "AssocCast",
        }
    }
}

impl Transition for StructuralTransition {
    fn id(&self) -> TransitionId {
        self.id
    }

    fn name(&self) -> &str {
        self.kind_name()
    }

    fn input_arcs(&self) -> &[InputArc] {
        &self.input_arcs
    }

    fn output_arcs(&self) -> &[OutputArc] {
        &self.output_arcs
    }

    fn is_enabled(
        &self,
        config: &Config,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> Option<Choice> {
        let mut choice = Choice::new();

        // 检查所有输入弧
        for arc in &self.input_arcs {
            let multiset = config.marking.get_multiset(arc.place);
            let weight = arc.weight.value();

            // 查找匹配的颜色
            let matching_colors: Vec<_> = multiset
                .iter()
                .filter(|(color, count)| {
                    *count >= weight
                        && arc.color_constraint.as_ref().map_or(true, |constraint| {
                            // 类型匹配检查
                            env.unify_types(&color.ty, constraint)
                        })
                })
                .flat_map(|(color, _)| {
                    (0..weight).map(move |_| color.clone())
                })
                .collect();

            if matching_colors.len() < weight {
                return None; // 没有足够的匹配 token
            }

            // 选择前 weight 个
            for color in matching_colors.into_iter().take(weight) {
                choice.add_selection(arc.place, color);
            }
        }

        // 检查守卫条件
        if !self.check_guards(config, env) {
            return None;
        }

        Some(choice)
    }

    fn fire(
        &self,
        config: &mut Config,
        choice: &Choice,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> bool {
        // 消耗输入 token
        for arc in &self.input_arcs {
            if let Some(colors) = choice.selections.get(&arc.place) {
                for color in colors {
                    if !config.marking.remove_token(arc.place, color) {
                        return false;
                    }
                }
            }
        }

        // 产生输出 token
        for arc in &self.output_arcs {
            if let Some(color_gen) = &arc.color_gen {
                for _ in 0..arc.weight.value() {
                    let color = color_gen(&config.marking, &config.stack, value_gen);
                    config.marking.add_token(arc.place, color);
                }
            }
        }

        // 执行栈操作
        self.apply_stack_operation(config, choice, env);

        true
    }

    fn stack_operation(&self) -> StackOp {
        match self.kind {
            StructuralTransitionKind::BorrowShrOwn => {
                // 需要从 choice 中获取值 ID,这里简化处理
                StackOp::Push(StackFrame::SharedBorrow {
                    value_id: ValueId::new(0), // 实际应该从 choice 获取
                    ty: TypeExpr::Primitive("T".to_string()), // 实际应该从输入获取
                })
            }
            StructuralTransitionKind::BorrowMut => {
                StackOp::Push(StackFrame::MutBorrow {
                    value_id: ValueId::new(0),
                    ty: TypeExpr::Primitive("T".to_string()),
                })
            }
            StructuralTransitionKind::EndMut | StructuralTransitionKind::EndShrKeep | StructuralTransitionKind::EndShrLast => {
                StackOp::Pop(None) // 实际应该匹配栈顶
            }
            StructuralTransitionKind::ProjMove | StructuralTransitionKind::ProjShr | StructuralTransitionKind::ProjMut => {
                StackOp::Push(StackFrame::FieldProj {
                    parent_id: ValueId::new(0),
                    field: "field".to_string(),
                    ty: TypeExpr::Primitive("T".to_string()),
                })
            }
            StructuralTransitionKind::EndProjMut => {
                StackOp::Pop(None)
            }
            _ => StackOp::None,
        }
    }
}

impl StructuralTransition {
    /// 检查守卫条件
    fn check_guards(&self, config: &Config, env: &dyn Env) -> bool {
        match self.kind {
            StructuralTransitionKind::CopyUse => {
                // 需要检查类型是否是 Copy
                // 这里简化处理,实际应该从输入弧获取类型
                true
            }
            StructuralTransitionKind::DupCopy => {
                // 检查预算
                if let Some(budget) = self.dup_budget {
                    // 这里应该检查是否还有预算
                    // 简化实现
                    budget > 0
                } else {
                    false
                }
            }
            StructuralTransitionKind::BorrowShrFrz => {
                // 检查栈顶是否匹配
                if let Some(frame) = config.stack.top() {
                    matches!(frame, StackFrame::SharedBorrow { .. })
                } else {
                    false
                }
            }
            StructuralTransitionKind::EndMut | StructuralTransitionKind::EndShrKeep | StructuralTransitionKind::EndShrLast => {
                // 检查栈顶是否匹配
                config.stack.top().is_some()
            }
            StructuralTransitionKind::EndProjMut => {
                // 检查栈顶是否是字段投影
                if let Some(frame) = config.stack.top() {
                    matches!(frame, StackFrame::FieldProj { .. })
                } else {
                    false
                }
            }
            _ => true,
        }
    }

    /// 应用栈操作
    fn apply_stack_operation(&self, config: &mut Config, choice: &Choice, env: &dyn Env) {
        match self.stack_operation() {
            StackOp::Push(frame) => {
                config.stack.push(frame);
            }
            StackOp::Pop(expected) => {
                if let Some(popped) = config.stack.pop() {
                    // 检查是否匹配期望的栈帧
                    if let Some(expected_frame) = expected {
                        // 实际应该检查是否匹配
                    }
                }
            }
            StackOp::Replace(frame) => {
                config.stack.pop();
                config.stack.push(frame);
            }
            StackOp::None => {}
        }
    }
}

/// 签名诱导变迁
/// 
/// 表示基于函数/方法签名的变迁 t_f
#[derive(Debug)]
pub struct SignatureTransition {
    id: TransitionId,
    name: String,
    /// 参数信息: (参数名, 类型, 传递方式)
    params: Vec<(String, TypeExpr, ParamPassing)>,
    /// 返回类型
    return_ty: Option<TypeExpr>,
    /// 返回类型是值还是引用
    return_kind: ReturnKind,
    /// 输入弧: 参数库所
    input_arcs: Vec<InputArc>,
    /// 输出弧: 返回库所
    output_arcs: Vec<OutputArc>,
}

/// 参数传递方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamPassing {
    /// 按值传递
    ByValue,
    /// 共享引用传递 &T
    BySharedRef,
    /// 可变引用传递 &mut T
    ByMutRef,
}

/// 返回类型种类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnKind {
    /// 返回值 (产生到 p_own)
    Own,
    /// 返回引用 (产生到 p_val)
    Ref,
}

impl SignatureTransition {
    /// 创建签名诱导变迁
    pub fn new(
        id: TransitionId,
        name: String,
        params: Vec<(String, TypeExpr, ParamPassing)>,
        return_ty: Option<TypeExpr>,
        return_kind: ReturnKind,
        own_place: Place,
        shr_place: Place,
        mut_place: Place,
        val_place: Place,
    ) -> Self {
        let mut input_arcs = Vec::new();
        let mut output_arcs = Vec::new();

        // 构建输入弧
        for (_, param_ty, passing) in &params {
            match passing {
                ParamPassing::ByValue => {
                    input_arcs.push(InputArc {
                        place: own_place,
                        weight: ArcWeight::new(1),
                        color_constraint: Some(param_ty.clone()),
                    });
                }
                ParamPassing::BySharedRef => {
                    let ref_ty = TypeExpr::shared_ref(param_ty.clone());
                    // 自循环: 引用 token 不消耗
                    input_arcs.push(InputArc {
                        place: shr_place,
                        weight: ArcWeight::new(1),
                        color_constraint: Some(ref_ty.clone()),
                    });
                    // 自循环输出
                    output_arcs.push(OutputArc {
                        place: shr_place,
                        weight: ArcWeight::new(1),
                        color_gen: Some(Box::new({
                            let ref_ty_clone = ref_ty.clone();
                            move |_marking, _stack, _value_id_gen| {
                                // 返回相同的引用 token
                                // 简化实现: 从 marking 中获取
                                Color::new(ref_ty_clone.clone(), ValueId::new(0))
                            }
                        })),
                    });
                }
                ParamPassing::ByMutRef => {
                    let ref_ty = TypeExpr::mut_ref(param_ty.clone());
                    // 自循环: 可变引用 token 不消耗
                    input_arcs.push(InputArc {
                        place: mut_place,
                        weight: ArcWeight::new(1),
                        color_constraint: Some(ref_ty.clone()),
                    });
                    // 自循环输出
                    output_arcs.push(OutputArc {
                        place: mut_place,
                        weight: ArcWeight::new(1),
                        color_gen: Some(Box::new({
                            let ref_ty_clone = ref_ty.clone();
                            move |_marking, _stack, _value_id_gen| {
                                Color::new(ref_ty_clone.clone(), ValueId::new(0))
                            }
                        })),
                    });
                }
            }
        }

        // 构建输出弧
        if let Some(rt) = &return_ty {
            match return_kind {
                ReturnKind::Own => {
                    output_arcs.push(OutputArc {
                        place: own_place,
                        weight: ArcWeight::new(1),
                        color_gen: Some(Box::new({
                            let rt_clone = rt.clone();
                            move |_marking, _stack, value_id_gen| {
                                Color::new(rt_clone.clone(), value_id_gen.next())
                            }
                        })),
                    });
                }
                ReturnKind::Ref => {
                    output_arcs.push(OutputArc {
                        place: val_place,
                        weight: ArcWeight::new(1),
                        color_gen: Some(Box::new({
                            let rt_clone = rt.clone();
                            move |_marking, _stack, value_id_gen| {
                                Color::new(rt_clone.clone(), value_id_gen.next())
                            }
                        })),
                    });
                }
            }
        }

        SignatureTransition {
            id,
            name,
            params,
            return_ty,
            return_kind,
            input_arcs,
            output_arcs,
        }
    }
}

impl Transition for SignatureTransition {
    fn id(&self) -> TransitionId {
        self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn input_arcs(&self) -> &[InputArc] {
        &self.input_arcs
    }

    fn output_arcs(&self) -> &[OutputArc] {
        &self.output_arcs
    }

    fn is_enabled(
        &self,
        config: &Config,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> Option<Choice> {
        // 对于按值传递的参数,如果是 Copy 类型,应该自循环输出
        // 这里简化实现
        let mut choice = Choice::new();

        for arc in &self.input_arcs {
            let multiset = config.marking.get_multiset(arc.place);
            let weight = arc.weight.value();

            let matching_colors: Vec<_> = multiset
                .iter()
                .filter(|(color, count)| {
                    *count >= weight
                        && arc.color_constraint.as_ref().map_or(true, |constraint| {
                            env.unify_types(&color.ty, constraint)
                        })
                })
                .flat_map(|(color, _)| {
                    (0..weight).map(move |_| color.clone())
                })
                .collect();

            if matching_colors.len() < weight {
                return None;
            }

            for color in matching_colors.into_iter().take(weight) {
                choice.add_selection(arc.place, color);
            }
        }

        Some(choice)
    }

    fn fire(
        &self,
        config: &mut Config,
        choice: &Choice,
        env: &dyn Env,
        value_gen: &mut ValueIdGenerator,
    ) -> bool {
        // 对于按值传递的参数
        for (i, arc) in self.input_arcs.iter().enumerate() {
            if let Some(colors) = choice.selections.get(&arc.place) {
                // 检查参数传递方式
                if let Some((_, param_ty, passing)) = self.params.get(i) {
                    match passing {
                        ParamPassing::ByValue => {
                            // 消耗 token
                            for color in colors {
                                config.marking.remove_token(arc.place, color);
                            }
                            // 如果是 Copy 类型,自循环输出
                            if env.is_copy(param_ty) {
                                for color in colors {
                                    config.marking.add_token(arc.place, color.clone());
                                }
                            }
                        }
                        ParamPassing::BySharedRef | ParamPassing::ByMutRef => {
                            // 引用类型: token 不消耗 (已经在输出弧中处理自循环)
                            // 这里不需要额外操作
                        }
                    }
                }
            }
        }

        // 产生输出 token
        for arc in &self.output_arcs {
            if let Some(color_gen) = &arc.color_gen {
                for _ in 0..arc.weight.value() {
                    let color = color_gen(&config.marking, &config.stack, value_gen);
                    config.marking.add_token(arc.place, color);
                }
            }
        }

        true
    }

    fn stack_operation(&self) -> StackOp {
        // 函数调用通常需要 push 作用域
        StackOp::Push(StackFrame::Scope {
            id: self.name.clone(),
        })
    }
}
