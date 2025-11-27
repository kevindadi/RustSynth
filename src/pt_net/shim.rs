/// Shim 填补机制: 为缺失的标准库类型和方法提供抽象接口
///
/// 此模块提供一个可扩展的框架，允许专家知识逐步添加到 Petri Net 中，
/// 填补那些在 rustdoc JSON 中不可见但对 fuzzing 至关重要的操作。
use super::structure::{EdgeData, EdgeKind, PetriNet, TransitionData, TransitionKind};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Shim 特质: 定义如何为特定类型注入变迁
pub trait Shim: Send + Sync {
    /// 获取 Shim 的名称 (用于日志和调试)
    fn name(&self) -> &str;

    /// 检测是否应该对给定的类型名应用此 Shim
    ///
    /// # 参数
    /// - `type_name`: 类型的完整名称，例如 "Vec<u8>" 或 "Option<String>"
    ///
    /// # 返回值
    /// - `Some(inner_types)`: 如果匹配，返回内部类型名列表
    /// - `None`: 如果不匹配
    fn matches(&self, type_name: &str) -> Option<Vec<String>>;

    /// 为匹配的类型注入变迁
    ///
    /// # 参数
    /// - `net`: Petri Net 图
    /// - `type_place`: 该类型对应的 Place 节点索引
    /// - `inner_places`: 内部类型的 Place 节点索引 (与 `matches` 返回的顺序对应)
    fn inject(&self, net: &mut PetriNet, type_place: NodeIndex, inner_places: &[NodeIndex]);
}

/// Shim 注册表: 管理所有可用的 Shim
pub struct ShimRegistry {
    shims: Vec<Box<dyn Shim>>,
}

impl ShimRegistry {
    /// 创建一个新的 Shim 注册表
    pub fn new() -> Self {
        Self { shims: Vec::new() }
    }

    /// 创建包含默认 Shim 的注册表
    pub fn with_default_shims() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(VecShim));
        registry.register(Box::new(OptionShim));
        registry.register(Box::new(ResultShim));
        registry.register(Box::new(BoxShim));
        registry
    }

    /// 注册一个新的 Shim
    pub fn register(&mut self, shim: Box<dyn Shim>) {
        log::info!("注册 Shim: {}", shim.name());
        self.shims.push(shim);
    }

    /// 对 Petri Net 应用所有 Shim
    pub fn apply_all(&self, net: &mut PetriNet) {
        log::info!("开始应用 {} 个 Shim", self.shims.len());

        // 收集所有 Place 信息
        let places: Vec<(NodeIndex, String)> = net
            .graph
            .node_indices()
            .filter_map(|idx| {
                net.graph[idx]
                    .as_place()
                    .map(|p| (idx, p.type_name.clone()))
            })
            .collect();

        // 为每个 Place 尝试应用 Shim
        for (place_idx, type_name) in places {
            for shim in &self.shims {
                if let Some(inner_type_names) = shim.matches(&type_name) {
                    log::debug!(
                        "Shim '{}' 匹配类型 '{}', 内部类型: {:?}",
                        shim.name(),
                        type_name,
                        inner_type_names
                    );

                    // 查找内部类型的 Place 索引
                    let inner_places: Vec<NodeIndex> = inner_type_names
                        .iter()
                        .filter_map(|inner_name| find_place_by_name(net, inner_name))
                        .collect();

                    if inner_places.len() == inner_type_names.len() {
                        shim.inject(net, place_idx, &inner_places);
                    } else {
                        log::warn!(
                            "Shim '{}' 无法找到所有内部类型的 Place: 需要 {:?}, 找到 {} 个",
                            shim.name(),
                            inner_type_names,
                            inner_places.len()
                        );
                    }
                }
            }
        }

        log::info!("Shim 应用完成");
    }
}

impl Default for ShimRegistry {
    fn default() -> Self {
        Self::with_default_shims()
    }
}

/// 辅助函数: 在 Petri Net 中查找指定名称的 Place
fn find_place_by_name(net: &PetriNet, name: &str) -> Option<NodeIndex> {
    net.graph.node_indices().find(|&idx| {
        if let Some(p) = net.graph[idx].as_place() {
            p.type_name == name
        } else {
            false
        }
    })
}

fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

fn add_transition(
    net: &mut PetriNet,
    func_name: &str,
    kind: TransitionKind,
    inputs: Vec<(NodeIndex, EdgeKind, usize)>,
    outputs: Vec<(NodeIndex, EdgeKind, usize)>,
) {
    let trans_id = hash_string(&format!("shim_{}", func_name));

    let trans_data = TransitionData {
        id: trans_id,
        func_name: func_name.to_string(),
        kind,
        generic_map: HashMap::new(),
    };

    let trans_idx = net.add_transition(trans_data);

    // 连接输入边
    for (place_idx, edge_kind, index) in inputs {
        net.connect(
            place_idx,
            trans_idx,
            EdgeData {
                kind: edge_kind,
                index,
                is_raw_ptr: false,
            },
        );
    }

    // 连接输出边
    for (place_idx, edge_kind, index) in outputs {
        net.connect(
            trans_idx,
            place_idx,
            EdgeData {
                kind: edge_kind,
                index,
                is_raw_ptr: false,
            },
        );
    }
}

/// Vec<T> Shim
///
/// 注入的方法:
/// - `Vec::push(&mut Vec<T>, T)`
/// - `Vec::pop(&mut Vec<T>) -> T`
/// - `Vec::new() -> Vec<T>`
struct VecShim;

impl Shim for VecShim {
    fn name(&self) -> &str {
        "Vec<T>"
    }

    fn matches(&self, type_name: &str) -> Option<Vec<String>> {
        // 匹配 "Vec<...>" 或 "std::vec::Vec<...>"
        if (type_name.starts_with("Vec<") || type_name.contains("::Vec<"))
            && type_name.ends_with('>')
        {
            // 提取内部类型
            let start = type_name.find('<')? + 1;
            let end = type_name.len() - 1;
            let inner = type_name[start..end].trim();
            Some(vec![inner.to_string()])
        } else {
            None
        }
    }

    fn inject(&self, net: &mut PetriNet, vec_place: NodeIndex, inner_places: &[NodeIndex]) {
        let t_place = inner_places[0];

        // Vec::new() -> Vec<T>
        add_transition(
            net,
            "Vec::new",
            TransitionKind::AssocFn,
            vec![], // 无输入
            vec![(vec_place, EdgeKind::Move, 0)],
        );

        // Vec::push(&mut Vec<T>, T)
        add_transition(
            net,
            "Vec::push",
            TransitionKind::MethodCall,
            vec![
                (vec_place, EdgeKind::MutRef, 0), // &mut self
                (t_place, EdgeKind::Move, 1),     // val: T
            ],
            vec![], // 无返回值
        );

        // Vec::pop(&mut Vec<T>) -> T
        add_transition(
            net,
            "Vec::pop",
            TransitionKind::MethodCall,
            vec![(vec_place, EdgeKind::MutRef, 0)], // &mut self
            vec![(t_place, EdgeKind::Move, 0)],     // -> T
        );

        log::debug!("注入 Vec<T> 的 3 个方法");
    }
}

/// Option<T> Shim
///
/// 注入的方法:
/// - `Option::Some(T) -> Option<T>`
/// - `Option::None() -> Option<T>`
/// - `Option::unwrap(Option<T>) -> T`
/// - `Option::is_some(&Option<T>) -> bool`
struct OptionShim;

impl Shim for OptionShim {
    fn name(&self) -> &str {
        "Option<T>"
    }

    fn matches(&self, type_name: &str) -> Option<Vec<String>> {
        if (type_name.starts_with("Option<") || type_name.contains("::Option<"))
            && type_name.ends_with('>')
        {
            let start = type_name.find('<')? + 1;
            let end = type_name.len() - 1;
            let inner = type_name[start..end].trim();
            Some(vec![inner.to_string()])
        } else {
            None
        }
    }

    fn inject(&self, net: &mut PetriNet, opt_place: NodeIndex, inner_places: &[NodeIndex]) {
        let t_place = inner_places[0];

        // Option::Some(T) -> Option<T>
        add_transition(
            net,
            "Option::Some",
            TransitionKind::VariantCtor,
            vec![(t_place, EdgeKind::Move, 0)],
            vec![(opt_place, EdgeKind::Move, 0)],
        );

        // Option::None() -> Option<T>
        add_transition(
            net,
            "Option::None",
            TransitionKind::VariantCtor,
            vec![],
            vec![(opt_place, EdgeKind::Move, 0)],
        );

        // Option::unwrap(Option<T>) -> T
        add_transition(
            net,
            "Option::unwrap",
            TransitionKind::MethodCall,
            vec![(opt_place, EdgeKind::Move, 0)],
            vec![(t_place, EdgeKind::Move, 0)],
        );

        log::debug!("注入 Option<T> 的 3 个方法");
    }
}

/// Result<T, E> Shim
///
/// 注入的方法:
/// - `Result::Ok(T) -> Result<T, E>`
/// - `Result::Err(E) -> Result<T, E>`
/// - `Result::unwrap(Result<T, E>) -> T`
struct ResultShim;

impl Shim for ResultShim {
    fn name(&self) -> &str {
        "Result<T, E>"
    }

    fn matches(&self, type_name: &str) -> Option<Vec<String>> {
        if (type_name.starts_with("Result<") || type_name.contains("::Result<"))
            && type_name.ends_with('>')
        {
            let start = type_name.find('<')? + 1;
            let end = type_name.len() - 1;
            let inner = type_name[start..end].trim();

            // 分割 T 和 E
            let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
            if parts.len() >= 2 {
                Some(vec![parts[0].to_string(), parts[1].to_string()])
            } else {
                None
            }
        } else {
            None
        }
    }

    fn inject(&self, net: &mut PetriNet, result_place: NodeIndex, inner_places: &[NodeIndex]) {
        if inner_places.len() < 2 {
            log::warn!(
                "Result Shim 需要 2 个内部类型，但只找到 {}",
                inner_places.len()
            );
            return;
        }

        let t_place = inner_places[0];
        let e_place = inner_places[1];

        // Result::Ok(T) -> Result<T, E>
        add_transition(
            net,
            "Result::Ok",
            TransitionKind::VariantCtor,
            vec![(t_place, EdgeKind::Move, 0)],
            vec![(result_place, EdgeKind::Move, 0)],
        );

        // Result::Err(E) -> Result<T, E>
        add_transition(
            net,
            "Result::Err",
            TransitionKind::VariantCtor,
            vec![(e_place, EdgeKind::Move, 0)],
            vec![(result_place, EdgeKind::Move, 0)],
        );

        // Result::unwrap(Result<T, E>) -> T
        add_transition(
            net,
            "Result::unwrap",
            TransitionKind::MethodCall,
            vec![(result_place, EdgeKind::Move, 0)],
            vec![(t_place, EdgeKind::Move, 0)],
        );

        log::debug!("注入 Result<T, E> 的 3 个方法");
    }
}

/// Box<T> Shim
///
/// 注入的方法:
/// - `Box::new(T) -> Box<T>`
/// - `Box::into_inner(Box<T>) -> T` (模拟解引用)
struct BoxShim;

impl Shim for BoxShim {
    fn name(&self) -> &str {
        "Box<T>"
    }

    fn matches(&self, type_name: &str) -> Option<Vec<String>> {
        if (type_name.starts_with("Box<") || type_name.contains("::Box<"))
            && type_name.ends_with('>')
        {
            let start = type_name.find('<')? + 1;
            let end = type_name.len() - 1;
            let inner = type_name[start..end].trim();
            Some(vec![inner.to_string()])
        } else {
            None
        }
    }

    fn inject(&self, net: &mut PetriNet, box_place: NodeIndex, inner_places: &[NodeIndex]) {
        let t_place = inner_places[0];

        // Box::new(T) -> Box<T>
        add_transition(
            net,
            "Box::new",
            TransitionKind::AssocFn,
            vec![(t_place, EdgeKind::Move, 0)],
            vec![(box_place, EdgeKind::Move, 0)],
        );

        // *box (模拟解引用为 Move)
        add_transition(
            net,
            "Box::into_inner",
            TransitionKind::MethodCall,
            vec![(box_place, EdgeKind::Move, 0)],
            vec![(t_place, EdgeKind::Move, 0)],
        );

        log::debug!("注入 Box<T> 的 2 个方法");
    }
}
