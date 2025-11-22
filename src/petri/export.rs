use rustdoc_types::{Crate, ItemEnum};
use serde_json::json;

use super::net::PetriNet;
use super::structure::{PlaceKind, TransitionKind};

/// 将 Petri 网导出为 DOT 格式（Graphviz）
pub fn to_dot(net: &PetriNet, crate_data: &Crate) -> String {
    let mut dot = String::new();
    dot.push_str("digraph PetriNet {\n");
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=circle];\n\n");

    // 添加所有 Place 节点
    dot.push_str("  // Places\n");
    for (place_id, place) in net.places() {
        let shape = "circle";
        let color = match &place.kind {
            PlaceKind::Struct(_) => "lightblue",
            PlaceKind::Enum(_) => "lightgreen",
            PlaceKind::Union(_) => "lightyellow",
            PlaceKind::Trait(_) => "mediumpurple", // Trait 用紫色
            PlaceKind::Variant(_) => "lightcyan",
            PlaceKind::StructField(_) => "wheat",
            PlaceKind::Primitive(_) => "pink",
            PlaceKind::Tuple(_) => "lavender",
            PlaceKind::Slice(_) => "peachpuff",
            PlaceKind::Array(_, _) => "lightgoldenrod",
            PlaceKind::Infer => "lightgray",
            PlaceKind::RawPointer(_, _) => "plum",
            PlaceKind::BorrowedRef(_, _, _) => "lightsteelblue",
            PlaceKind::GenericParam(_, _) => "lightsalmon", // 泛型参数用浅橙色
            PlaceKind::Result(_, _) => "coral",             // Result 类型用珊瑚色
            PlaceKind::Option(_) => "khaki",                // Option 类型用卡其色
            PlaceKind::AssocType(_, _, _) => "lightpink",   // 关联类型用浅粉色
        };

        // 对于泛型参数，添加所属类型标注
        let label = match &place.kind {
            PlaceKind::GenericParam(owner_id, generic_name) => {
                let owner_info = get_owner_type_info(crate_data, owner_id);
                if let Some((kind, name)) = owner_info {
                    format!("{} ({kind}: {name})", generic_name)
                } else {
                    generic_name.clone()
                }
            }
            _ => place.name.clone(),
        };
        let label = escape_dot_label(&label);
        dot.push_str(&format!(
            "  p{} [label=\"{}\", shape={}, style=filled, fillcolor={}];\n",
            place_id.0.index(),
            label,
            shape,
            color
        ));
    }

    // 添加所有 Transition 节点
    dot.push_str("\n  // Transitions\n");
    for (trans_id, trans) in net.transitions() {
        let shape = "box";
        let label = match &trans.kind {
            TransitionKind::Function(_) => escape_dot_label(&trans.name),
            TransitionKind::Hold(_, _) => "holds".to_string(),
            TransitionKind::Unwrap => "unwrap".to_string(),
            TransitionKind::Ok => "ok".to_string(),
            TransitionKind::Impls(_, _) => "impls".to_string(),
            TransitionKind::AliasType(_, _) => "alias_type".to_string(),
        };

        dot.push_str(&format!(
            "  t{} [label=\"{}\", shape={}, style=filled, fillcolor=lightgray];\n",
            trans_id.0.index(),
            label,
            shape
        ));
    }

    // 添加所有边
    dot.push_str("\n  // Flows\n");
    for (trans_id, _trans) in net.transitions() {
        // 输入边: Place -> Transition
        for (place_id, flow) in net.transition_inputs(trans_id) {
            dot.push_str(&format!(
                "  p{} -> t{} [label=\"{}\"];\n",
                place_id.0.index(),
                trans_id.0.index(),
                escape_dot_label(&flow.param_type)
            ));
        }

        // 输出边: Transition -> Place
        for (place_id, flow) in net.transition_outputs(trans_id) {
            dot.push_str(&format!(
                "  t{} -> p{} [label=\"{}\"];\n",
                trans_id.0.index(),
                place_id.0.index(),
                escape_dot_label(&flow.param_type)
            ));
        }
    }

    dot.push_str("}\n");
    dot
}

/// 将 Petri 网导出为 JSON 格式
pub fn to_json(net: &PetriNet, _crate_data: &Crate) -> String {
    let mut places = Vec::new();
    for (place_id, place) in net.places() {
        places.push(json!({
            "id": format!("p{}", place_id.0.index()),
            "name": place.name,
            "path": place.path,
            "kind": format!("{:?}", place.kind),
        }));
    }

    let mut transitions = Vec::new();
    for (trans_id, trans) in net.transitions() {
        transitions.push(json!({
            "id": format!("t{}", trans_id.0.index()),
            "name": trans.name,
            "kind": format!("{:?}", trans.kind),
        }));
    }

    let mut flows = Vec::new();
    for (trans_id, _trans) in net.transitions() {
        // 输入边
        for (place_id, flow) in net.transition_inputs(trans_id) {
            flows.push(json!({
                "from": format!("p{}", place_id.0.index()),
                "to": format!("t{}", trans_id.0.index()),
                "weight": flow.weight,
                "param_type": flow.param_type,
                "borrow_kind": format!("{:?}", flow.borrow_kind),
            }));
        }

        // 输出边
        for (place_id, flow) in net.transition_outputs(trans_id) {
            flows.push(json!({
                "from": format!("t{}", trans_id.0.index()),
                "to": format!("p{}", place_id.0.index()),
                "weight": flow.weight,
                "param_type": flow.param_type,
                "borrow_kind": format!("{:?}", flow.borrow_kind),
            }));
        }
    }

    let result = json!({
        "places": places,
        "transitions": transitions,
        "flows": flows,
    });

    serde_json::to_string_pretty(&result).unwrap()
}

/// 获取泛型参数所属类型的信息
/// 返回 (类型种类, 类型名称)，例如 ("struct", "MyStruct") 或 ("fn", "my_function")
fn get_owner_type_info(crate_data: &Crate, owner_id: &rustdoc_types::Id) -> Option<(String, String)> {
    if let Some(item) = crate_data.index.get(owner_id) {
        let name = item.name.as_deref().unwrap_or("(匿名)").to_string();
        match &item.inner {
            ItemEnum::Struct(_) => Some(("struct".to_string(), name)),
            ItemEnum::Enum(_) => Some(("enum".to_string(), name)),
            ItemEnum::Union(_) => Some(("union".to_string(), name)),
            ItemEnum::Trait(_) => Some(("trait".to_string(), name)),
            ItemEnum::Function(_) => Some(("fn".to_string(), name)),
            _ => None,
        }
    } else {
        // 可能是函数或 impl 块的泛型，尝试在 index 中查找
        if let Some(item) = crate_data.index.values().find(|i| i.id == *owner_id) {
            let name = item.name.as_deref().unwrap_or("(匿名)").to_string();
            match &item.inner {
                ItemEnum::Struct(_) => Some(("struct".to_string(), name)),
                ItemEnum::Enum(_) => Some(("enum".to_string(), name)),
                ItemEnum::Union(_) => Some(("union".to_string(), name)),
                ItemEnum::Trait(_) => Some(("trait".to_string(), name)),
                ItemEnum::Function(_) => Some(("fn".to_string(), name)),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// 转义 DOT 标签中的特殊字符
fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
