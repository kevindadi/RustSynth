use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::fs;

use crate::petri::net::{ArcData, FunctionContext, FunctionSummary, PetriNet, PlaceId};

/// RepairPetriNet - 与 JSON Schema 对应的 Petri 网顶层结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonPetriNet {
    pub places: Vec<JsonPlace>,
    #[serde(default)]
    pub tokens: Vec<JsonToken>,
    pub transitions: Vec<JsonTransition>,
    #[serde(default)]
    pub guards: Vec<JsonGuard>,
    #[serde(default)]
    pub metadata: JsonMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JsonMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustdoc_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonPlace {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<JsonField>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonField {
    pub name: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutability: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonToken {
    pub id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub var_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub borrow_of: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub origin: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTransition {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<JsonEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<JsonEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generic_params: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impl_of: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guard_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub origin: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attributes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonEdge {
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonGuard {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<JsonGuardCondition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonGuardCondition {
    pub lhs: String,
    pub op: String,
    pub rhs: serde_json::Value,
    #[serde(default)]
    pub negate: bool,
}

impl From<&PetriNet> for JsonPetriNet {
    fn from(net: &PetriNet) -> Self {
        let mut place_id_map: HashMap<PlaceId, String> = HashMap::new();
        let mut places = Vec::new();

        for (idx, (place_id, place)) in net.places().enumerate() {
            let id = format!("P{}", idx);
            place_id_map.insert(place_id, id.clone());
            let mut attributes = HashMap::new();
            if !place.implemented_traits.is_empty() {
                attributes.insert(
                    "implemented_traits".to_string(),
                    serde_json::to_value(&place.implemented_traits).unwrap_or_default(),
                );
            }
            if !place.required_trait_bounds.is_empty() {
                attributes.insert(
                    "required_trait_bounds".to_string(),
                    serde_json::to_value(&place.required_trait_bounds).unwrap_or_default(),
                );
            }
            if place.is_generic_parameter {
                attributes.insert("is_generic_parameter".to_string(), serde_json::Value::Bool(true));
            }

            places.push(JsonPlace {
                id,
                name: place.descriptor.display().to_string(),
                kind: None,
                generics: Vec::new(),
                fields: Vec::new(),
                attributes,
            });
        }

        let mut transitions = Vec::new();
        for (idx, (transition_id, transition)) in net.transitions().enumerate() {
            let id = format!("T{}", idx);
            let summary: &FunctionSummary = &transition.summary;

            let kind = Some(match &summary.context {
                FunctionContext::FreeFunction => "function".to_string(),
                FunctionContext::InherentMethod { .. } => "method".to_string(),
                FunctionContext::TraitImplementation { .. } => "method".to_string(),
            });

            let inputs = collect_edges_for_transition(
                net,
                transition_id,
                &place_id_map,
                true,
            );
            let outputs = collect_edges_for_transition(
                net,
                transition_id,
                &place_id_map,
                false,
            );

            let mut origin = HashMap::new();
            if let Some(path) = &summary.qualified_path {
                origin.insert(
                    "def_location".to_string(),
                    serde_json::Value::String(path.to_string()),
                );
            }

            transitions.push(JsonTransition {
                id,
                name: summary.name.to_string(),
                kind,
                inputs,
                outputs,
                generic_params: summary.generics.iter().map(|g| g.to_string()).collect(),
                impl_of: None,
                guard_refs: Vec::new(),
                origin,
                attributes: HashMap::new(),
            });
        }

        JsonPetriNet {
            places,
            tokens: Vec::new(),
            transitions,
            guards: Vec::new(),
            metadata: JsonMetadata {
                crate_name: None,
                rustdoc_version: None,
                source_file: None,
                timestamp: None,
            },
        }
    }
}

fn collect_edges_for_transition(
    net: &PetriNet,
    transition_id: crate::petri::net::TransitionId,
    _place_id_map: &HashMap<PlaceId, String>,
    incoming: bool,
) -> Vec<JsonEdge> {
    let iter: Box<dyn Iterator<Item = (PlaceId, &ArcData)>> = if incoming {
        Box::new(net.transition_inputs(transition_id))
    } else {
        Box::new(net.transition_outputs(transition_id))
    };

    iter.filter_map(|(place_id, arc)| {
        let type_name = if let Some(param) = &arc.parameter {
            param.descriptor.display().to_string()
        } else if let Some(place) = net.place(place_id) {
            place.descriptor.display().to_string()
        } else {
            return None;
        };

        let name = arc
            .parameter
            .as_ref()
            .and_then(|p| p.name.as_ref())
            .map(|n| n.to_string());

        let mode = arc.borrow_kind.map(|bk| match bk {
            crate::petri::type_repr::BorrowKind::Owned => "value".to_string(),
            crate::petri::type_repr::BorrowKind::SharedRef => "&".to_string(),
            crate::petri::type_repr::BorrowKind::MutRef => "&mut".to_string(),
            crate::petri::type_repr::BorrowKind::RawConstPtr => "*const".to_string(),
            crate::petri::type_repr::BorrowKind::RawMutPtr => "*mut".to_string(),
        });

        Some(JsonEdge {
            r#type: type_name,
            name,
            mode,
            mutability: None,
            position: None,
            description: None,
        })
    })
    .collect()
}

impl JsonPetriNet {
    /// 从 JSON 文件加载 Petri 网
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let petri_net = serde_json::from_str(&content)?;
        Ok(petri_net)
    }

    /// 将 Petri 网保存为 JSON 文件
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// 从 JSON 字符串解析
    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// 转换为 JSON 字符串
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 转换为内存中的 PetriNet 结构(用于执行和分析)
    pub fn to_petri_net(&self) -> Result<PetriNet, String> {
        let mut net = PetriNet::new();
        let mut place_map: HashMap<String, PlaceId> = HashMap::new();

        // 1. 创建所有的 Place
        for json_place in &self.places {
            let descriptor = super::type_repr::TypeDescriptor::from_string(&json_place.name);
            
            let place_id = if json_place.attributes.get("is_generic_parameter")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                // 泛型参数 Place
                let bounds = json_place.attributes.get("required_trait_bounds")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| std::sync::Arc::<str>::from(s))
                    .collect();
                net.add_generic_parameter_place(descriptor, bounds)
            } else {
                // 普通或基本类型 Place
                let traits = json_place.attributes.get("implemented_traits")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| std::sync::Arc::<str>::from(s))
                    .collect();
                net.add_primitive_place(descriptor, traits)
            };

            place_map.insert(json_place.id.clone(), place_id);
        }

        // 2. 创建所有的 Transition
        for json_transition in &self.transitions {
            let context = match json_transition.kind.as_deref() {
                Some("method") => {
                    // 尝试从第一个输入推断 receiver
                    let receiver = if let Some(input) = json_transition.inputs.first() {
                        super::type_repr::TypeDescriptor::from_string(&input.r#type)
                    } else {
                        super::type_repr::TypeDescriptor::from_string("Self")
                    };
                    FunctionContext::InherentMethod { receiver }
                }
                _ => FunctionContext::FreeFunction,
            };

            let inputs: Vec<super::net::ParameterSummary> = json_transition.inputs.iter()
                .map(|edge| super::net::ParameterSummary {
                    name: edge.name.clone().map(|s| std::sync::Arc::<str>::from(s)),
                    descriptor: super::type_repr::TypeDescriptor::from_string(&edge.r#type),
                })
                .collect();

            let output = json_transition.outputs.first()
                .map(|edge| super::type_repr::TypeDescriptor::from_string(&edge.r#type));

            let summary = FunctionSummary {
                item_id: rustdoc_types::Id(0), // 从 JSON 加载时使用虚拟 ID
                name: std::sync::Arc::<str>::from(json_transition.name.clone()),
                qualified_path: json_transition.origin.get("def_location")
                    .and_then(|v| v.as_str())
                    .map(|s| std::sync::Arc::<str>::from(s)),
                signature: std::sync::Arc::<str>::from(
                    format!("{}(...)", json_transition.name)
                ),
                generics: json_transition.generic_params.iter()
                    .map(|s| std::sync::Arc::<str>::from(s.clone()))
                    .collect(),
                where_clauses: Vec::new(),
                trait_bounds: Vec::new(),
                context,
                inputs,
                output,
            };

            let transition_id = net.add_transition(summary);

            // 3. 添加输入弧 (Place -> Transition)
            for input_edge in &json_transition.inputs {
                if let Some(&place_id) = place_map.get(&input_edge.r#type) {
                    let borrow_kind = parse_borrow_kind(input_edge.mode.as_deref());
                    net.add_input_arc_from_place(
                        place_id,
                        transition_id,
                        ArcData {
                            weight: 1,
                            kind: super::net::ArcKind::Normal,
                            parameter: Some(super::net::ParameterSummary {
                                name: input_edge.name.clone()
                                    .map(|s| std::sync::Arc::<str>::from(s)),
                                descriptor: super::type_repr::TypeDescriptor::from_string(&input_edge.r#type),
                            }),
                            descriptor: None,
                            borrow_kind: Some(borrow_kind),
                        },
                    );
                }
            }

            // 4. 添加输出弧 (Transition -> Place)
            for output_edge in &json_transition.outputs {
                if let Some(&place_id) = place_map.get(&output_edge.r#type) {
                    let borrow_kind = parse_borrow_kind(output_edge.mode.as_deref());
                    net.add_output_arc_to_place(
                        transition_id,
                        place_id,
                        ArcData {
                            weight: 1,
                            kind: super::net::ArcKind::Normal,
                            parameter: None,
                            descriptor: Some(super::type_repr::TypeDescriptor::from_string(&output_edge.r#type)),
                            borrow_kind: Some(borrow_kind),
                        },
                    );
                }
            }
        }

        Ok(net)
    }

    /// 验证 Petri 网的结构是否有效
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        // 检查 place IDs 是否唯一
        let mut place_ids = std::collections::HashSet::new();
        for place in &self.places {
            if !place_ids.insert(&place.id) {
                errors.push(format!("重复的 Place ID: {}", place.id));
            }
        }

        // 检查 transition IDs 是否唯一
        let mut transition_ids = std::collections::HashSet::new();
        for transition in &self.transitions {
            if !transition_ids.insert(&transition.id) {
                errors.push(format!("重复的 Transition ID: {}", transition.id));
            }
        }

        // 检查 token 引用的类型是否存在
        for token in &self.tokens {
            if !place_ids.contains(&token.r#type) {
                errors.push(format!("Token {} 引用了不存在的类型: {}", token.id, token.r#type));
            }
        }

        // 检查 guard IDs 是否唯一
        let mut guard_ids = std::collections::HashSet::new();
        for guard in &self.guards {
            if !guard_ids.insert(&guard.id) {
                errors.push(format!("重复的 Guard ID: {}", guard.id));
            }
        }

        // 检查 transition 引用的 guard 是否存在
        for transition in &self.transitions {
            for guard_ref in &transition.guard_refs {
                if !guard_ids.contains(guard_ref) {
                    errors.push(format!(
                        "Transition {} 引用了不存在的 Guard: {}",
                        transition.id, guard_ref
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// 添加一个新的 Place
    pub fn add_place(&mut self, place: JsonPlace) {
        self.places.push(place);
    }

    /// 添加一个新的 Token
    pub fn add_token(&mut self, token: JsonToken) {
        self.tokens.push(token);
    }

    /// 添加一个新的 Transition
    pub fn add_transition(&mut self, transition: JsonTransition) {
        self.transitions.push(transition);
    }

    /// 添加一个新的 Guard
    pub fn add_guard(&mut self, guard: JsonGuard) {
        self.guards.push(guard);
    }

    /// 根据 ID 查找 Place
    pub fn find_place(&self, id: &str) -> Option<&JsonPlace> {
        self.places.iter().find(|p| p.id == id)
    }

    /// 根据 ID 查找 Transition
    pub fn find_transition(&self, id: &str) -> Option<&JsonTransition> {
        self.transitions.iter().find(|t| t.id == id)
    }

    /// 根据 ID 查找 Guard
    pub fn find_guard(&self, id: &str) -> Option<&JsonGuard> {
        self.guards.iter().find(|g| g.id == id)
    }
}

/// 从字符串解析借用类型
fn parse_borrow_kind(mode: Option<&str>) -> super::type_repr::BorrowKind {
    match mode {
        Some("&") => super::type_repr::BorrowKind::SharedRef,
        Some("&mut") => super::type_repr::BorrowKind::MutRef,
        Some("*const") => super::type_repr::BorrowKind::RawConstPtr,
        Some("*mut") => super::type_repr::BorrowKind::RawMutPtr,
        Some("value") | None => super::type_repr::BorrowKind::Owned,
        _ => super::type_repr::BorrowKind::Owned,
    }
}


