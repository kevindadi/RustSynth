//! 单元测试
//!
//! 测试 PCPN 运行时系统的核心功能

#[cfg(test)]
mod tests {
    use crate::pushdown_colored_pt_net::types::*;
    use crate::pushdown_colored_pt_net::runtime::*;
    use crate::pushdown_colored_pt_net::env::MockEnv;
    use crate::pushdown_colored_pt_net::build::*;

    /// 测试共享借用嵌套: BorrowShrOwn -> BorrowShrFrz -> EndShrKeep -> EndShrLast
    #[test]
    fn test_shared_borrow_nesting() {
        let mut env = MockEnv::new();
        let mut value_gen = ValueIdGenerator::new();

        // 创建库所
        let own_place = Place::new(0);
        let shr_place = Place::new(1);

        // 创建类型
        let ty = TypeExpr::Primitive("u8".to_string());

        // 创建初始配置
        let mut config = Config::new();
        let color = Color::new(ty.clone(), value_gen.next());
        config.marking.add_token(own_place, color);

        // 创建 BorrowShrOwn 变迁
        let trans1 = StructuralTransition::new_borrow_shr_own(
            TransitionId::new(0),
            own_place,
            shr_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice1 = trans1.is_enabled(&config, &env, &mut value_gen);
        assert!(choice1.is_some(), "BorrowShrOwn should be enabled");

        // 触发 BorrowShrOwn
        let choice1 = choice1.unwrap();
        assert!(trans1.fire(&mut config, &choice1, &env, &mut value_gen));
        
        // 检查栈
        assert_eq!(config.stack.depth(), 1, "Stack should have one frame");

        // 创建 BorrowShrFrz 变迁
        let trans2 = StructuralTransition::new_borrow_shr_frz(
            TransitionId::new(1),
            shr_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice2 = trans2.is_enabled(&config, &env, &mut value_gen);
        assert!(choice2.is_some(), "BorrowShrFrz should be enabled");

        // 触发 BorrowShrFrz
        let choice2 = choice2.unwrap();
        assert!(trans2.fire(&mut config, &choice2, &env, &mut value_gen));

        // 创建 EndShrKeep 变迁
        let trans3 = StructuralTransition::new_end_shr_keep(
            TransitionId::new(2),
            shr_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice3 = trans3.is_enabled(&config, &env, &mut value_gen);
        assert!(choice3.is_some(), "EndShrKeep should be enabled");

        // 触发 EndShrKeep
        let choice3 = choice3.unwrap();
        assert!(trans3.fire(&mut config, &choice3, &env, &mut value_gen));
        
        // 检查栈深度应该减少
        assert_eq!(config.stack.depth(), 1, "Stack should still have one frame");

        // 创建 EndShrLast 变迁
        let trans4 = StructuralTransition::new_end_shr_last(
            TransitionId::new(3),
            shr_place,
            own_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice4 = trans4.is_enabled(&config, &env, &mut value_gen);
        assert!(choice4.is_some(), "EndShrLast should be enabled");

        // 触发 EndShrLast
        let choice4 = choice4.unwrap();
        assert!(trans4.fire(&mut config, &choice4, &env, &mut value_gen));
        
        // 检查栈应该为空
        assert!(config.stack.is_empty(), "Stack should be empty");
        
        // 检查所有者 token 应该恢复
        assert!(config.marking.contains(own_place, &Color::new(ty.clone(), ValueId::new(0))));
    }

    /// 测试可变借用: BorrowMut -> (some call self-loop) -> EndMut
    #[test]
    fn test_mutable_borrow() {
        let mut env = MockEnv::new();
        let mut value_gen = ValueIdGenerator::new();

        // 创建库所
        let own_place = Place::new(0);
        let mut_place = Place::new(1);

        // 创建类型
        let ty = TypeExpr::Primitive("u8".to_string());

        // 创建初始配置
        let mut config = Config::new();
        let color = Color::new(ty.clone(), value_gen.next());
        config.marking.add_token(own_place, color);

        // 创建 BorrowMut 变迁
        let trans1 = StructuralTransition::new_borrow_mut(
            TransitionId::new(0),
            own_place,
            mut_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice1 = trans1.is_enabled(&config, &env, &mut value_gen);
        assert!(choice1.is_some(), "BorrowMut should be enabled");

        // 触发 BorrowMut
        let choice1 = choice1.unwrap();
        assert!(trans1.fire(&mut config, &choice1, &env, &mut value_gen));
        
        // 检查栈
        assert_eq!(config.stack.depth(), 1, "Stack should have one frame");

        // 创建 EndMut 变迁
        let trans2 = StructuralTransition::new_end_mut(
            TransitionId::new(1),
            mut_place,
            own_place,
            ty.clone(),
        );

        // 检查是否可以触发
        let choice2 = trans2.is_enabled(&config, &env, &mut value_gen);
        assert!(choice2.is_some(), "EndMut should be enabled");

        // 触发 EndMut
        let choice2 = choice2.unwrap();
        assert!(trans2.fire(&mut config, &choice2, &env, &mut value_gen));
        
        // 检查栈应该为空
        assert!(config.stack.is_empty(), "Stack should be empty");
        
        // 检查所有者 token 应该恢复
        assert!(config.marking.contains(own_place, &Color::new(ty.clone(), ValueId::new(0))));
    }

    /// 测试可变字段重借用: ProjMut -> EndProjMut (栈正确性)
    #[test]
    fn test_mutable_field_reborrow() {
        let mut env = MockEnv::new();
        let mut value_gen = ValueIdGenerator::new();

        // 创建库所
        let struct_place = Place::new(0);
        let field_place = Place::new(1);

        // 创建类型
        let struct_ty = TypeExpr::Composite {
            name: "Struct".to_string(),
            type_args: Vec::new(),
        };
        let field_ty = TypeExpr::Primitive("u8".to_string());
        let field_name = "field".to_string();

        // 创建初始配置 (可变引用)
        let mut config = Config::new();
        let struct_ref_ty = TypeExpr::mut_ref(struct_ty.clone());
        let struct_color = Color::new(struct_ref_ty.clone(), value_gen.next());
        config.marking.add_token(struct_place, struct_color);

        // 创建 ProjMut 变迁
        let trans1 = StructuralTransition::new_proj_mut(
            TransitionId::new(0),
            struct_place,
            field_place,
            struct_ty.clone(),
            field_ty.clone(),
            field_name.clone(),
        );

        // 检查是否可以触发
        let choice1 = trans1.is_enabled(&config, &env, &mut value_gen);
        assert!(choice1.is_some(), "ProjMut should be enabled");

        // 触发 ProjMut
        let choice1 = choice1.unwrap();
        assert!(trans1.fire(&mut config, &choice1, &env, &mut value_gen));
        
        // 检查栈
        assert_eq!(config.stack.depth(), 1, "Stack should have one field projection frame");

        // 创建 EndProjMut 变迁
        let trans2 = StructuralTransition::new_end_proj_mut(
            TransitionId::new(1),
            field_place,
            struct_place,
            struct_ty.clone(),
            field_ty.clone(),
            field_name.clone(),
        );

        // 检查是否可以触发
        let choice2 = trans2.is_enabled(&config, &env, &mut value_gen);
        assert!(choice2.is_some(), "EndProjMut should be enabled");

        // 触发 EndProjMut
        let choice2 = choice2.unwrap();
        assert!(trans2.fire(&mut config, &choice2, &env, &mut value_gen));
        
        // 检查栈应该为空
        assert!(config.stack.is_empty(), "Stack should be empty");
        
        // 检查结构体引用 token 应该恢复
        let struct_ref_color = Color::new(struct_ref_ty, ValueId::new(0));
        assert!(config.marking.contains(struct_place, &struct_ref_color));
    }
}
