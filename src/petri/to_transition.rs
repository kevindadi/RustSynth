//! 变迁(Transition)创建相关的函数

use log::debug;
use rustdoc_types::Id;

use crate::petri::{
    PetriNetBuilder,
    net::PlaceId,
    structure::{BorrowKind, Flow, Transition, TransitionKind},
};

impl<'a> PetriNetBuilder<'a> {
    /// 创建 holds transition 连接 owner 和 member
    pub(super) fn create_holds_transition(
        &mut self,
        owner_id: Id,
        member_id: Id,
        owner_place_id: PlaceId,
        member_place_id: PlaceId,
    ) {
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            format!("holds"),
            TransitionKind::Hold(owner_id, member_id),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:owner -> transition -> member
        self.net.add_flow(
            owner_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "owner".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            member_place_id,
            Flow {
                weight: 1,
                param_type: "member".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
    }

    /// 创建 impls transition 连接实现类型和 trait
    pub(super) fn create_impls_transition(
        &mut self,
        impl_type_id: Id,
        trait_id: Id,
        impl_place_id: PlaceId,
        trait_place_id: PlaceId,
    ) {
        let transition_id_val = self.generate_temp_id();
        let transition = Transition::new(
            transition_id_val,
            format!("impls"),
            TransitionKind::Impls(impl_type_id, trait_id),
        );
        let transition_id = self.net.add_transition_and_get_id(transition);

        // 添加边:impl_type -> transition -> trait
        self.net.add_flow(
            impl_place_id,
            transition_id,
            Flow {
                weight: 1,
                param_type: "impl_type".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );
        self.net.add_flow_from_transition(
            transition_id,
            trait_place_id,
            Flow {
                weight: 1,
                param_type: "trait".to_string(),
                borrow_kind: BorrowKind::Owned,
            },
        );

        debug!(
            "✨ 创建 impls 关系: 类型 {:?} 实现 trait {:?}",
            impl_type_id, trait_id
        );
    }
}
