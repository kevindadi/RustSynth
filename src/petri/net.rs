use std::{fmt::Write, sync::Arc};

use petgraph::{
    Direction,
    algo::{dijkstra, is_cyclic_directed},
    graph::NodeIndex,
    stable_graph::StableGraph,
    visit::EdgeRef,
};
use rustdoc_types::{Crate, Id, ItemEnum, Variant};

/// Token 现在直接使用 Item ID 来表示类型
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// 类型 Item 的 ID
    pub item_id: Id,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceId(pub(crate) NodeIndex);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TransitionId(pub(crate) NodeIndex);

pub type ArcWeight = u32;

#[derive(Clone, Debug)]
pub enum Node {
    Place(Place),
    Transition(Transition),
}

/// 库所(Place)代表一个类型定义
/// 直接使用 rustdoc_types 中的 Item ID 和 ItemEnum
#[derive(Clone, Debug)]
pub enum Place {
    /// Struct、Enum、Union 类型
    Composite {
        /// Item 的 ID
        item_id: Id,
        /// Item 的类型 (Struct, Enum, Union)
        kind: ItemEnum,
        /// Enum 的 Variant 列表(仅当类型为 Enum 时使用)
        variants: Vec<Variant>,
    },
    // 泛型占位符
    Generics {
        // 泛型参数名
        name: Arc<str>,
        // 泛型 info, 保留 rustdoc
        params: rustdoc_types::GenericParamDef,
    },
    // 基本类型
    Primitive {
        name: Arc<str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ArcKind {
    #[default]
    Normal,
    Inhibitor,
    Reset,
}

/// 函数上下文信息
#[derive(Clone, Debug)]
pub enum FunctionContext {
    FreeFunction,
    InherentMethod {
        /// 接收者类型的 Item ID
        receiver_id: Id,
    },
    TraitImplementation {
        /// 接收者类型的 Item ID
        receiver_id: Id,
        /// Trait 的路径
        trait_path: Arc<str>,
    },
}

/// 变迁(Transition)代表一个函数
#[derive(Clone, Debug)]
pub struct Transition {
    /// 函数 Item 的 ID
    pub item_id: Id,
    /// 函数名称
    pub name: Arc<str>,
    /// 函数上下文
    pub context: FunctionContext,
    /// 输入参数的类型 Item ID 列表
    pub input_types: Option<Vec<Id>>,
    /// 返回值的类型 Item ID (如果有)
    pub output_type: Option<Id>,
}

#[derive(Clone, Debug)]
pub struct ArcData {
    pub weight: ArcWeight,
    pub kind: ArcKind,
    /// 参数名称(对于输入弧)
    pub parameter_name: Option<Arc<str>>,
    /// 参数类型的 Item ID (对于输入弧)
    pub input_type_id: Option<Id>,
    /// 输出类型的 Item ID (对于输出弧)
    pub output_type_id: Option<Id>,
}

/// Place 查找键 - 直接使用 Item ID
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PlaceLookupKey {
    /// Item ID
    ItemId(Id),
}

#[derive(Debug)]
pub struct PetriNet {
    graph: StableGraph<Node, ArcData>,
    place_lookup: indexmap::IndexMap<PlaceLookupKey, PlaceId>,
    /// 存储 Item ID 到 Item 的引用(通过 Crate 的 index)
    /// 注意：这里不直接存储 Item，而是存储 ID，使用时从外部 Crate 获取
    _item_ids: std::collections::HashSet<Id>,
}

impl Default for PetriNet {
    fn default() -> Self {
        Self {
            graph: StableGraph::new(),
            place_lookup: indexmap::IndexMap::new(),
            _item_ids: std::collections::HashSet::new(),
        }
    }
}

impl Place {
    /// 获取 Item ID
    pub fn item_id(&self) -> Option<Id> {
        match self {
            Place::Composite { item_id, .. } => Some(*item_id),
            _ => None,
        }
    }

    /// 获取 ItemEnum 类型
    pub fn kind(&self) -> Option<&ItemEnum> {
        match self {
            Place::Composite { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// 获取 Variant 列表(仅 Enum 类型有)
    pub fn variants(&self) -> Option<&[Variant]> {
        match self {
            Place::Composite { variants, .. } => Some(variants),
            _ => None,
        }
    }
}

impl PetriNet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加 Composite 类型(Struct、Enum、Union)的 Place
    pub fn add_composite_place(
        &mut self,
        item_id: Id,
        kind: ItemEnum,
        variants: Vec<Variant>,
    ) -> PlaceId {
        let lookup_key = PlaceLookupKey::ItemId(item_id);

        if let Some(&id) = self.place_lookup.get(&lookup_key) {
            // 如果已存在,更新 variants
            if let Some(place) = self.graph.node_weight_mut(id.0) {
                if let Node::Place(Place::Composite {
                    variants: existing_variants,
                    ..
                }) = place
                {
                    *existing_variants = variants;
                }
            }
            return id;
        }

        let place = Place::Composite {
            item_id,
            kind,
            variants,
        };
        let node_idx = self.graph.add_node(Node::Place(place));
        let id = PlaceId(node_idx);

        self.place_lookup.insert(lookup_key, id);
        self._item_ids.insert(item_id);
        id
    }

    /// 添加变迁(Transition)
    pub fn add_transition(
        &mut self,
        item_id: Id,
        name: Arc<str>,
        context: FunctionContext,
        input_types: Option<Vec<Id>>,
        output_type: Option<Id>,
    ) -> TransitionId {
        let transition = Transition {
            item_id,
            name,
            context,
            input_types: input_types.clone(),
            output_type,
        };
        let node_idx = self.graph.add_node(Node::Transition(transition));
        let id = TransitionId(node_idx);
        self._item_ids.insert(item_id);
        // 同时记录输入类型的 ID

        for input_id in input_types.unwrap() {
            self._item_ids.insert(input_id);
        }
        if let Some(output_id) = output_type {
            self._item_ids.insert(output_id);
        }
        id
    }

    pub fn add_input_arc_from_place(
        &mut self,
        place: PlaceId,
        transition: TransitionId,
        parameter_name: Option<Arc<str>>,
        input_type_id: Id,
    ) {
        let arc = ArcData {
            weight: 1,
            kind: ArcKind::Normal,
            parameter_name,
            input_type_id: Some(input_type_id),
            output_type_id: None,
        };
        self.graph.add_edge(place.0, transition.0, arc);
    }

    pub fn add_output_arc_to_place(
        &mut self,
        transition: TransitionId,
        place: PlaceId,
        output_type_id: Id,
    ) {
        let arc = ArcData {
            weight: 1,
            kind: ArcKind::Normal,
            parameter_name: None,
            input_type_id: None,
            output_type_id: Some(output_type_id),
        };
        self.graph.add_edge(transition.0, place.0, arc);
    }

    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &Place)> {
        self.graph.node_indices().filter_map(|idx| {
            if let Node::Place(place) = &self.graph[idx] {
                Some((PlaceId(idx), place))
            } else {
                None
            }
        })
    }

    pub fn transitions(&self) -> impl Iterator<Item = (TransitionId, &Transition)> {
        self.graph.node_indices().filter_map(|idx| {
            if let Node::Transition(transition) = &self.graph[idx] {
                Some((TransitionId(idx), transition))
            } else {
                None
            }
        })
    }

    pub fn place(&self, id: PlaceId) -> Option<&Place> {
        self.graph.node_weight(id.0).and_then(|node| {
            if let Node::Place(place) = node {
                Some(place)
            } else {
                None
            }
        })
    }

    pub fn transition(&self, id: TransitionId) -> Option<&Transition> {
        self.graph.node_weight(id.0).and_then(|node| {
            if let Node::Transition(transition) = node {
                Some(transition)
            } else {
                None
            }
        })
    }

    /// 通过 Item ID 查找 Place
    pub fn place_id(&self, item_id: Id) -> Option<PlaceId> {
        let lookup_key = PlaceLookupKey::ItemId(item_id);
        self.place_lookup.get(&lookup_key).copied()
    }

    pub fn place_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|idx| matches!(self.graph[*idx], Node::Place(_)))
            .count()
    }

    pub fn transition_count(&self) -> usize {
        self.graph
            .node_indices()
            .filter(|idx| matches!(self.graph[*idx], Node::Transition(_)))
            .count()
    }

    /// 获取 Transition 的所有输入边(从 Place 到 Transition)
    pub fn transition_inputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &ArcData)> {
        self.graph
            .edges_directed(transition.0, Direction::Incoming)
            .filter_map(|edge| {
                let source = edge.source();
                if let Node::Place(_) = &self.graph[source] {
                    Some((PlaceId(source), edge.weight()))
                } else {
                    None
                }
            })
    }

    /// 获取 Transition 的所有输出边(从 Transition 到 Place)
    pub fn transition_outputs(
        &self,
        transition: TransitionId,
    ) -> impl Iterator<Item = (PlaceId, &ArcData)> {
        self.graph
            .edges_directed(transition.0, Direction::Outgoing)
            .filter_map(|edge| {
                let target = edge.target();
                if let Node::Place(_) = &self.graph[target] {
                    Some((PlaceId(target), edge.weight()))
                } else {
                    None
                }
            })
    }

    pub fn to_dot(&self, crate_: &Crate) -> String {
        let mut dot = String::new();
        dot.push_str("digraph PetriNet {\n");
        dot.push_str("  rankdir=LR;\n");
        dot.push_str("  node [fontname=\"Helvetica\"];\n");

        self.write_places(&mut dot, crate_);
        self.write_transitions(&mut dot, crate_);
        self.write_arcs(&mut dot);

        dot.push_str("}\n");
        dot
    }

    fn write_places(&self, dot: &mut String, crate_: &Crate) {
        let mut enum_places = Vec::new();
        let mut other_places = Vec::new();

        for (id, place) in self.places() {
            let item_id = place.item_id();
            let item_name = crate_
                .index
                .get(&item_id.unwrap())
                .and_then(|item| item.name.as_deref())
                .unwrap_or("Unknown");

            match place.kind().unwrap() {
                ItemEnum::Enum(_) if place.variants().unwrap().is_empty() => {
                    enum_places.push((id, place, item_name));
                }
                _ => {
                    other_places.push((id, place, item_name));
                }
            }
        }

        // 写入 Enum 类型（黄色）
        for (id, _place, name) in enum_places {
            let label = simplify_type_name(name);
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=yellow,label=\"{}\"];",
                id.0.index(),
                label
            );
        }

        // 写入其他类型（Struct、Union）
        for (id, _place, name) in other_places {
            let label = simplify_type_name(name);
            let _ = writeln!(
                dot,
                "  p{} [shape=circle,style=filled,fillcolor=lightblue,label=\"{}\"];",
                id.0.index(),
                label
            );
        }
    }

    fn write_transitions(&self, dot: &mut String, crate_: &Crate) {
        for (id, transition) in self.transitions() {
            let item = crate_
                .index
                .get(&transition.item_id)
                .map(|item| item.name.as_deref().unwrap_or("Unknown"))
                .unwrap_or("Unknown");

            // 构建简化的函数签名：类型名 + 函数名
            let sig = format!("{}::{}", item, transition.name.as_ref());
            let simplified_sig = simplify_signature(&sig);

            let _ = writeln!(
                dot,
                "  t{} [shape=box,style=rounded,label=\"{}\"];",
                id.0.index(),
                simplified_sig
            );
        }
    }

    fn write_arcs(&self, dot: &mut String) {
        for (transition_id, _transition) in self.transitions() {
            // 输入弧:Place -> Transition
            for (place_id, arc_data) in self.transition_inputs(transition_id) {
                self.write_input_arc(dot, place_id, transition_id, arc_data);
            }

            // 输出弧:Transition -> Place
            for (place_id, arc_data) in self.transition_outputs(transition_id) {
                self.write_output_arc(dot, transition_id, place_id, arc_data);
            }
        }
    }

    fn write_input_arc(
        &self,
        dot: &mut String,
        place_id: PlaceId,
        transition_id: TransitionId,
        arc: &ArcData,
    ) {
        // 显示参数名称（如果有）
        let label = arc.parameter_name.as_deref().map(|s| s.to_string());
        let attr = edge_attr(arc.kind, label);
        let _ = writeln!(
            dot,
            "  p{} -> t{}{};",
            place_id.0.index(),
            transition_id.0.index(),
            attr
        );
    }

    fn write_output_arc(
        &self,
        dot: &mut String,
        transition_id: TransitionId,
        place_id: PlaceId,
        arc: &ArcData,
    ) {
        let attr = edge_attr(arc.kind, None);
        let _ = writeln!(
            dot,
            "  t{} -> p{}{};",
            transition_id.0.index(),
            place_id.0.index(),
            attr
        );
    }

    /// 检查图中是否存在环路(用于检测类型依赖循环)
    pub fn has_cycles(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// 计算从一个 Place 到另一个 Place 的最短路径
    /// 返回路径上经过的 transitions 数量,如果不可达则返回 None
    pub fn shortest_path_length(&self, from: PlaceId, to: PlaceId) -> Option<usize> {
        let distances = dijkstra(
            &self.graph,
            from.0,
            Some(to.0),
            |_| 1, // 统一权重为 1
        );

        distances.get(&to.0).copied()
    }

    /// 查找从 source Place 可以通过一次转换到达的所有 target Places
    /// 返回 (target_place, transition, arc_data) 的列表
    pub fn reachable_in_one_step(&self, source: PlaceId) -> Vec<(PlaceId, TransitionId, &ArcData)> {
        let mut reachable = Vec::new();

        // 找到所有从 source place 出发的边(到 transition)
        for edge_ref in self.graph.edges_directed(source.0, Direction::Outgoing) {
            let transition_node = edge_ref.target();

            // 检查这个节点是否是 transition
            if let Node::Transition(_) = &self.graph[transition_node] {
                let transition_id = TransitionId(transition_node);

                // 找到这个 transition 的所有输出
                for output_edge in self
                    .graph
                    .edges_directed(transition_node, Direction::Outgoing)
                {
                    let target_node = output_edge.target();
                    if let Node::Place(_) = &self.graph[target_node] {
                        reachable.push((PlaceId(target_node), transition_id, output_edge.weight()));
                    }
                }
            }
        }

        reachable
    }

    pub fn statistics(&self) -> PetriNetStatistics {
        let mut stats = PetriNetStatistics {
            place_count: 0,
            transition_count: 0,
            arc_count: self.graph.edge_count(),
            has_cycles: self.has_cycles(),
            max_place_in_degree: 0,
            max_place_out_degree: 0,
            max_transition_in_degree: 0,
            max_transition_out_degree: 0,
        };

        for node_idx in self.graph.node_indices() {
            match &self.graph[node_idx] {
                Node::Place(_) => {
                    stats.place_count += 1;
                    let in_deg = self
                        .graph
                        .edges_directed(node_idx, Direction::Incoming)
                        .count();
                    let out_deg = self
                        .graph
                        .edges_directed(node_idx, Direction::Outgoing)
                        .count();
                    stats.max_place_in_degree = stats.max_place_in_degree.max(in_deg);
                    stats.max_place_out_degree = stats.max_place_out_degree.max(out_deg);
                }
                Node::Transition(_) => {
                    stats.transition_count += 1;
                    let in_deg = self
                        .graph
                        .edges_directed(node_idx, Direction::Incoming)
                        .count();
                    let out_deg = self
                        .graph
                        .edges_directed(node_idx, Direction::Outgoing)
                        .count();
                    stats.max_transition_in_degree = stats.max_transition_in_degree.max(in_deg);
                    stats.max_transition_out_degree = stats.max_transition_out_degree.max(out_deg);
                }
            }
        }

        stats
    }

    pub fn graph(&self) -> &StableGraph<Node, ArcData> {
        &self.graph
    }

    /// 查找类型转换链:从 source 类型到 target 类型的所有可能路径
    /// 返回路径列表,每条路径是一系列 transition IDs
    pub fn find_type_conversion_paths(
        &self,
        source: PlaceId,
        target: PlaceId,
        max_depth: usize,
    ) -> Vec<Vec<TransitionId>> {
        let mut paths = Vec::new();
        let mut current_path = Vec::new();
        let mut visited = std::collections::HashSet::new();

        self.dfs_find_paths(
            source,
            target,
            &mut current_path,
            &mut visited,
            &mut paths,
            max_depth,
        );

        paths
    }

    /// 深度优先搜索辅助函数
    fn dfs_find_paths(
        &self,
        current: PlaceId,
        target: PlaceId,
        current_path: &mut Vec<TransitionId>,
        visited: &mut std::collections::HashSet<PlaceId>,
        paths: &mut Vec<Vec<TransitionId>>,
        max_depth: usize,
    ) {
        if current == target {
            paths.push(current_path.clone());
            return;
        }

        if current_path.len() >= max_depth {
            return;
        }

        visited.insert(current);

        for (next_place, transition_id, _arc) in self.reachable_in_one_step(current) {
            if !visited.contains(&next_place) {
                current_path.push(transition_id);
                self.dfs_find_paths(next_place, target, current_path, visited, paths, max_depth);
                current_path.pop();
            }
        }

        visited.remove(&current);
    }
}

/// Petri 网的统计信息
#[derive(Debug, Clone)]
pub struct PetriNetStatistics {
    pub place_count: usize,
    pub transition_count: usize,
    pub arc_count: usize,
    pub has_cycles: bool,
    pub max_place_in_degree: usize,
    pub max_place_out_degree: usize,
    pub max_transition_in_degree: usize,
    pub max_transition_out_degree: usize,
}

impl std::fmt::Display for PetriNetStatistics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Petri 网统计信息:")?;
        writeln!(f, "  Places: {}", self.place_count)?;
        writeln!(f, "  Transitions: {}", self.transition_count)?;
        writeln!(f, "  Arcs: {}", self.arc_count)?;
        writeln!(f, "  Has Cycles: {}", self.has_cycles)?;
        writeln!(f, "  Max Place In-Degree: {}", self.max_place_in_degree)?;
        writeln!(f, "  Max Place Out-Degree: {}", self.max_place_out_degree)?;
        writeln!(
            f,
            "  Max Transition In-Degree: {}",
            self.max_transition_in_degree
        )?;
        writeln!(
            f,
            "  Max Transition Out-Degree: {}",
            self.max_transition_out_degree
        )?;
        Ok(())
    }
}

/// 清理类型标签，去掉特殊符号但保留路径信息和泛型参数
fn clean_type_label(label: &str) -> String {
    let mut result = label.to_string();

    // 去掉特殊符号：( ) [ ] { } * & mut
    // 但保留 :: 路径分隔符和 <> 泛型参数符号
    result = result
        .replace('(', "")
        .replace(')', "")
        .replace('[', "")
        .replace(']', "")
        .replace('{', "")
        .replace('}', "")
        .replace('*', "")
        .replace("&mut ", "")
        .replace("&", "")
        .replace("mut ", "");

    // 清理多余的空格和逗号
    result = result
        .replace(", ", ",")
        .replace(" ,", ",")
        .replace("  ", " ")
        .trim()
        .to_string();

    result
}

fn simplify_type_name(type_name: &str) -> String {
    // 简化类型名称：去掉路径前缀，只保留最后一部分
    if let Some(last_colon) = type_name.rfind("::") {
        type_name[last_colon + 2..].to_string()
    } else {
        type_name.to_string()
    }
}

/// 移除生命周期参数
/// Base64Display<'a, 'e, E> -> Base64Display<E>
/// Option<&(dyn Error + 'static)> -> Option<&(dyn Error)>
fn remove_lifetimes(type_name: &str) -> String {
    let mut result = String::with_capacity(type_name.len());
    let mut chars = type_name.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\'' {
            // 跳过生命周期名称
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    chars.next();
                } else {
                    break;
                }
            }

            // 跳过 'static 后的空格和 +
            while let Some(&' ') = chars.peek() {
                chars.next();
            }

            // 如果后面是 + 号,也要跳过(因为这是 trait bound 的一部分)
            if let Some(&'+') = chars.peek() {
                chars.next();
                // 跳过 + 后的空格
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }

                // 如果 + 后面没有其他内容了(只有括号或 >),需要移除前面的空格和 +
                // 这个会在后面的 replace 中处理
            }

            // 如果是逗号,也跳过它和后续空格
            if let Some(&',') = chars.peek() {
                chars.next();
                while let Some(&' ') = chars.peek() {
                    chars.next();
                }
            }
        } else {
            result.push(ch);
        }
    }

    // 清理可能的多余字符
    result = result.replace("<, ", "<");
    result = result.replace(", >", ">");
    result = result.replace("< ", "<");
    result = result.replace(" >", ">");
    result = result.replace("<>", "");
    result = result.replace("  ", " ");
    result = result.replace(" +)", ")"); // 移除 trait bounds 结尾的 +
    result = result.replace("+ )", ")");
    result = result.replace("+)", ")");
    result = result.replace(" )", ")"); // 移除括号前的空格
    result = result.replace("( ", "("); // 移除括号后的空格
    result = result.replace("dyn  ", "dyn ");

    result
}

/// 简化函数签名显示
/// 移除路径前缀、生命周期、简化泛型约束
/// 例如: fn encode<T: AsRef<[u8]>>(self: &Self, input: T) -> String
///   -> fn encode(self: &Self, input: T) -> String
fn simplify_signature(sig: &str) -> String {
    let sig = sig.trim();

    // 移除 const、unsafe 等修饰符(保留位置但简化)
    let mut result = sig.to_string();

    // 移除泛型约束(保留泛型参数但移除约束)
    // fn foo<T: Trait>(x: T) -> fn foo<T>(x: T)
    result = simplify_generic_bounds(&result);

    // 移除生命周期
    result = remove_lifetimes(&result);

    // 移除路径前缀 (std::string::String -> String)
    result = remove_type_paths(&result);

    // 限制长度
    if result.len() > 80 {
        if let Some(arrow_pos) = result[..80].rfind("->") {
            format!("{} -> ...", &result[..arrow_pos].trim())
        } else if let Some(paren_pos) = result[..80].rfind(')') {
            format!("{})", &result[..paren_pos])
        } else {
            format!("{}...", &result[..77])
        }
    } else {
        result
    }
}

/// 简化泛型约束
/// fn foo<T: Clone + Debug, U: Display>(x: T) -> fn foo<T, U>(x: T)
fn simplify_generic_bounds(sig: &str) -> String {
    let mut result = String::new();
    let mut chars = sig.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '<' if result.ends_with("fn ")
                || result.chars().rev().take(10).any(|c| c.is_alphanumeric()) =>
            {
                // 可能是泛型参数开始
                result.push(ch);

                // 跳过约束部分
                let mut generic_content = String::new();
                let mut bracket_level = 1;

                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    if next_ch == '<' {
                        bracket_level += 1;
                        generic_content.push(next_ch);
                    } else if next_ch == '>' {
                        bracket_level -= 1;
                        if bracket_level == 0 {
                            // 处理泛型内容,移除约束
                            let params: Vec<&str> = generic_content.split(',').collect();
                            let simplified_params: Vec<String> = params
                                .iter()
                                .map(|p| {
                                    // 提取参数名(在 : 之前的部分)
                                    if let Some(colon_pos) = p.find(':') {
                                        p[..colon_pos].trim().to_string()
                                    } else {
                                        p.trim().to_string()
                                    }
                                })
                                .collect();
                            result.push_str(&simplified_params.join(", "));
                            result.push('>');
                            break;
                        }
                        generic_content.push(next_ch);
                    } else {
                        generic_content.push(next_ch);
                    }
                }
            }
            _ => {
                result.push(ch);
            }
        }
    }

    result
}

/// 移除类型路径前缀
/// std::string::String -> String
/// <U as TryFrom<T>>::Error -> Error (不带尖括号)
fn remove_type_paths(sig: &str) -> String {
    let mut result = String::new();
    let mut current_word = String::new();
    let mut in_angle_brackets: i32 = 0;
    let mut bracket_start = 0;

    for ch in sig.chars() {
        match ch {
            '<' => {
                if in_angle_brackets == 0 {
                    bracket_start = result.len();
                }
                in_angle_brackets += 1;

                // 先处理当前的word
                if !current_word.is_empty() {
                    if let Some(last_colon) = current_word.rfind("::") {
                        result.push_str(&current_word[last_colon + 2..]);
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
            '>' => {
                in_angle_brackets = in_angle_brackets.saturating_sub(1);

                // 先处理当前的word
                if !current_word.is_empty() {
                    // 检查是否是 qualified path (e.g. <T as Trait>::Type)
                    // 如果在尖括号内且有 ::,这可能是 associated type
                    if let Some(last_colon) = current_word.rfind("::") {
                        let type_name = &current_word[last_colon + 2..];
                        // 如果这是 qualified path 的最后一部分,移除前面的尖括号
                        if in_angle_brackets == 0 && result[bracket_start..].starts_with('<') {
                            // 这是类似 <T as Trait>::Error 的情况
                            // 移除整个 qualified path 的尖括号部分
                            result.truncate(bracket_start);
                            result.push_str(type_name);
                            current_word.clear();
                            // 不添加 >
                            continue;
                        } else {
                            result.push_str(type_name);
                        }
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
            _ if ch.is_alphanumeric() || ch == '_' || ch == ':' => {
                current_word.push(ch);
            }
            _ => {
                if !current_word.is_empty() {
                    if let Some(last_colon) = current_word.rfind("::") {
                        result.push_str(&current_word[last_colon + 2..]);
                    } else {
                        result.push_str(&current_word);
                    }
                    current_word.clear();
                }
                result.push(ch);
            }
        }
    }

    if !current_word.is_empty() {
        if let Some(last_colon) = current_word.rfind("::") {
            result.push_str(&current_word[last_colon + 2..]);
        } else {
            result.push_str(&current_word);
        }
    }

    result
}

fn edge_attr(kind: ArcKind, label: Option<String>) -> String {
    let mut parts = Vec::new();

    if let Some(label) = label {
        parts.push(format!("label=\"{}\"", label));
    }

    match kind {
        ArcKind::Normal => {}
        ArcKind::Inhibitor => {
            parts.push("style=dashed".into());
            parts.push("arrowhead=dot".into());
        }
        ArcKind::Reset => {
            parts.push("color=\"firebrick\"".into());
            parts.push("arrowhead=tee".into());
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        format!(" [{}]", parts.join(","))
    }
}
