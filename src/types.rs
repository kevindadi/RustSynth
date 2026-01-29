//! 9-Place 类型系统定义
//!
//! 论文模型：对每个基础类型 T，区分 T、&T、&mut T 三种形态，
//! 再乘以 capability (own/frz/blk) 得到 9 个 place。

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

pub type PlaceId = usize;
pub type TransitionId = usize;
pub type VarId = u32;
pub type RegionLabel = u32;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeForm {
    Value,
    RefShr,
    RefMut,
}

impl fmt::Display for TypeForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeForm::Value => write!(f, "T"),
            TypeForm::RefShr => write!(f, "&T"),
            TypeForm::RefMut => write!(f, "&mut T"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    Own,
    Frz,
    Blk,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::Own => write!(f, "own"),
            Capability::Frz => write!(f, "frz"),
            Capability::Blk => write!(f, "blk"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TyGround {
    Primitive(String),
    Path { name: String, args: Vec<TyGround> },
    Tuple(Vec<TyGround>),
    Unit,
}

impl TyGround {
    pub fn primitive(name: &str) -> Self {
        TyGround::Primitive(name.to_string())
    }

    /// Create a tuple type, automatically normalizing empty tuples to Unit
    pub fn tuple(elems: Vec<TyGround>) -> Self {
        if elems.is_empty() {
            TyGround::Unit
        } else {
            TyGround::Tuple(elems)
        }
    }

    pub fn path(name: &str) -> Self {
        TyGround::Path {
            name: name.to_string(),
            args: vec![],
        }
    }

    pub fn path_with_args(name: &str, args: Vec<TyGround>) -> Self {
        TyGround::Path {
            name: name.to_string(),
            args,
        }
    }

    pub fn short_name(&self) -> String {
        match self {
            TyGround::Primitive(s) => s.clone(),
            TyGround::Path { name, args } => {
                let base = name.split("::").last().unwrap_or(name);
                if args.is_empty() {
                    base.to_string()
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.short_name()).collect();
                    format!("{}<{}>", base, args_str.join(", "))
                }
            }
            TyGround::Tuple(elems) => {
                let elems_str: Vec<_> = elems.iter().map(|e| e.short_name()).collect();
                format!("({})", elems_str.join(", "))
            }
            TyGround::Unit => "()".to_string(),
        }
    }

    pub fn full_name(&self) -> String {
        match self {
            TyGround::Primitive(s) => s.clone(),
            TyGround::Path { name, args } => {
                if args.is_empty() {
                    name.clone()
                } else {
                    let args_str: Vec<_> = args.iter().map(|a| a.full_name()).collect();
                    format!("{}<{}>", name, args_str.join(", "))
                }
            }
            TyGround::Tuple(elems) => {
                let elems_str: Vec<_> = elems.iter().map(|e| e.full_name()).collect();
                format!("({})", elems_str.join(", "))
            }
            TyGround::Unit => "()".to_string(),
        }
    }

    pub fn is_primitive(&self) -> bool {
        match self {
            TyGround::Primitive(name) => matches!(
                name.as_str(),
                "bool"
                    | "char"
                    | "str"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "f32"
                    | "f64"
            ),
            TyGround::Unit => true,
            _ => false,
        }
    }

    pub fn is_copy(&self) -> bool {
        match self {
            TyGround::Primitive(_) => true,
            TyGround::Unit => true,
            TyGround::Tuple(elems) => elems.iter().all(|e| e.is_copy()),
            _ => false,
        }
    }
}

impl fmt::Display for TyGround {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short_name())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TyScheme {
    pub base: TyGround,
    pub type_vars: Vec<String>,
    pub bounds: Vec<(String, Vec<String>)>,
}

impl TyScheme {
    pub fn ground(ty: TyGround) -> Self {
        TyScheme {
            base: ty,
            type_vars: vec![],
            bounds: vec![],
        }
    }

    pub fn is_ground(&self) -> bool {
        self.type_vars.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaceKey {
    pub base_type: TyGround,
    pub form: TypeForm,
    pub cap: Capability,
}

impl PlaceKey {
    pub fn new(base_type: TyGround, form: TypeForm, cap: Capability) -> Self {
        PlaceKey {
            base_type,
            form,
            cap,
        }
    }

    pub fn display_name(&self) -> String {
        let form_str = match self.form {
            TypeForm::Value => self.base_type.short_name(),
            TypeForm::RefShr => format!("&{}", self.base_type.short_name()),
            TypeForm::RefMut => format!("&mut {}", self.base_type.short_name()),
        };
        format!("{}_{}", self.cap, form_str)
    }
}

impl fmt::Display for PlaceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    pub id: PlaceId,
    pub base_type: TyGround,
    pub form: TypeForm,
    pub cap: Capability,
    pub budget: usize,
}

impl Place {
    pub fn key(&self) -> PlaceKey {
        PlaceKey::new(self.base_type.clone(), self.form.clone(), self.cap)
    }

    pub fn display_name(&self) -> String {
        self.key().display_name()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Token {
    pub vid: VarId,
    pub ty: TyGround,
    pub form: TypeForm,
    pub regions: SmallVec<[RegionLabel; 2]>,
    pub borrowed_from: Option<VarId>,
}

impl Token {
    pub fn new_owned(vid: VarId, ty: TyGround) -> Self {
        Token {
            vid,
            ty,
            form: TypeForm::Value,
            regions: SmallVec::new(),
            borrowed_from: None,
        }
    }

    pub fn new_ref_shr(vid: VarId, ty: TyGround, region: RegionLabel, from: VarId) -> Self {
        let mut regions = SmallVec::new();
        regions.push(region);
        Token {
            vid,
            ty,
            form: TypeForm::RefShr,
            regions,
            borrowed_from: Some(from),
        }
    }

    pub fn new_ref_mut(vid: VarId, ty: TyGround, region: RegionLabel, from: VarId) -> Self {
        let mut regions = SmallVec::new();
        regions.push(region);
        Token {
            vid,
            ty,
            form: TypeForm::RefMut,
            regions,
            borrowed_from: Some(from),
        }
    }

    pub fn is_ref(&self) -> bool {
        matches!(self.form, TypeForm::RefShr | TypeForm::RefMut)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackFrame {
    Freeze { owner_vid: VarId },
    Shr {
        owner_vid: VarId,
        ref_vid: VarId,
        region: RegionLabel,
    },
    Mut {
        owner_vid: VarId,
        ref_vid: VarId,
        region: RegionLabel,
    },
}

impl StackFrame {
    pub fn owner_vid(&self) -> VarId {
        match self {
            StackFrame::Freeze { owner_vid } => *owner_vid,
            StackFrame::Shr { owner_vid, .. } => *owner_vid,
            StackFrame::Mut { owner_vid, .. } => *owner_vid,
        }
    }

    pub fn ref_vid(&self) -> Option<VarId> {
        match self {
            StackFrame::Freeze { .. } => None,
            StackFrame::Shr { ref_vid, .. } => Some(*ref_vid),
            StackFrame::Mut { ref_vid, .. } => Some(*ref_vid),
        }
    }

    pub fn region(&self) -> Option<RegionLabel> {
        match self {
            StackFrame::Freeze { .. } => None,
            StackFrame::Shr { region, .. } => Some(*region),
            StackFrame::Mut { region, .. } => Some(*region),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BorrowStack {
    pub frames: Vec<StackFrame>,
}

impl BorrowStack {
    pub fn new() -> Self {
        BorrowStack { frames: Vec::new() }
    }

    pub fn push(&mut self, frame: StackFrame) {
        self.frames.push(frame);
    }

    pub fn pop(&mut self) -> Option<StackFrame> {
        self.frames.pop()
    }

    pub fn top(&self) -> Option<&StackFrame> {
        self.frames.last()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_blocked(&self, vid: VarId) -> bool {
        self.frames.iter().any(|f| f.owner_vid() == vid)
    }

    pub fn find_ref(&self, ref_vid: VarId) -> Option<usize> {
        self.frames
            .iter()
            .position(|f| f.ref_vid() == Some(ref_vid))
    }

    pub fn count_shr_for_owner(&self, owner_vid: VarId) -> usize {
        self.frames
            .iter()
            .filter(|f| matches!(f, StackFrame::Shr { owner_vid: o, .. } if *o == owner_vid))
            .count()
    }

    pub fn has_freeze_for_owner(&self, owner_vid: VarId) -> bool {
        self.frames
            .iter()
            .any(|f| matches!(f, StackFrame::Freeze { owner_vid: o } if *o == owner_vid))
    }
}

#[derive(Clone, Debug)]
pub struct Marking {
    pub tokens: HashMap<PlaceId, Vec<Token>>,
}

impl Marking {
    pub fn new() -> Self {
        Marking {
            tokens: HashMap::new(),
        }
    }

    pub fn add(&mut self, place: PlaceId, token: Token) {
        self.tokens.entry(place).or_default().push(token);
    }

    pub fn remove(&mut self, place: PlaceId) -> Option<Token> {
        self.tokens.get_mut(&place).and_then(|ts| {
            if ts.is_empty() {
                None
            } else {
                Some(ts.remove(0))
            }
        })
    }

    pub fn remove_by_vid(&mut self, place: PlaceId, vid: VarId) -> Option<Token> {
        self.tokens.get_mut(&place).and_then(|ts| {
            ts.iter().position(|t| t.vid == vid).map(|i| ts.remove(i))
        })
    }

    pub fn count(&self, place: PlaceId) -> usize {
        self.tokens.get(&place).map(|ts| ts.len()).unwrap_or(0)
    }

    pub fn get(&self, place: PlaceId) -> Option<&Vec<Token>> {
        self.tokens.get(&place)
    }

    pub fn find_token(&self, place: PlaceId, vid: VarId) -> Option<&Token> {
        self.tokens
            .get(&place)
            .and_then(|ts| ts.iter().find(|t| t.vid == vid))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&PlaceId, &Vec<Token>)> {
        self.tokens.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.tokens.values().all(|ts| ts.is_empty())
    }

    pub fn total_tokens(&self) -> usize {
        self.tokens.values().map(|ts| ts.len()).sum()
    }
}

impl Default for Marking {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonToken {
    pub vid: VarId,
    pub ty: TyGround,
    pub form: TypeForm,
    pub regions: SmallVec<[RegionLabel; 2]>,
    pub borrowed_from: Option<VarId>,
}

impl From<&Token> for CanonToken {
    fn from(t: &Token) -> Self {
        CanonToken {
            vid: t.vid,
            ty: t.ty.clone(),
            form: t.form.clone(),
            regions: t.regions.clone(),
            borrowed_from: t.borrowed_from,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CanonFrame {
    pub kind: CanonFrameKind,
    pub owner_vid: VarId,
    pub ref_vid: Option<VarId>,
    pub region: Option<RegionLabel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CanonFrameKind {
    Freeze,
    Shr,
    Mut,
}

impl From<&StackFrame> for CanonFrame {
    fn from(f: &StackFrame) -> Self {
        match f {
            StackFrame::Freeze { owner_vid } => CanonFrame {
                kind: CanonFrameKind::Freeze,
                owner_vid: *owner_vid,
                ref_vid: None,
                region: None,
            },
            StackFrame::Shr {
                owner_vid,
                ref_vid,
                region,
            } => CanonFrame {
                kind: CanonFrameKind::Shr,
                owner_vid: *owner_vid,
                ref_vid: Some(*ref_vid),
                region: Some(*region),
            },
            StackFrame::Mut {
                owner_vid,
                ref_vid,
                region,
            } => CanonFrame {
                kind: CanonFrameKind::Mut,
                owner_vid: *owner_vid,
                ref_vid: Some(*ref_vid),
                region: Some(*region),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_key_display() {
        let key = PlaceKey::new(TyGround::path("Counter"), TypeForm::Value, Capability::Own);
        assert_eq!(key.display_name(), "own_Counter");

        let key2 = PlaceKey::new(TyGround::primitive("i32"), TypeForm::RefShr, Capability::Frz);
        assert_eq!(key2.display_name(), "frz_&i32");
    }

    #[test]
    fn test_token_creation() {
        let t = Token::new_owned(0, TyGround::path("Counter"));
        assert!(!t.is_ref());
        assert_eq!(t.form, TypeForm::Value);

        let r = Token::new_ref_shr(1, TyGround::path("Counter"), 0, 0);
        assert!(r.is_ref());
        assert_eq!(r.borrowed_from, Some(0));
    }

    #[test]
    fn test_borrow_stack() {
        let mut stack = BorrowStack::new();
        stack.push(StackFrame::Freeze { owner_vid: 0 });
        stack.push(StackFrame::Shr {
            owner_vid: 0,
            ref_vid: 1,
            region: 0,
        });

        assert!(stack.is_blocked(0));
        assert!(!stack.is_blocked(1));
        assert_eq!(stack.count_shr_for_owner(0), 1);
        assert!(stack.has_freeze_for_owner(0));
    }

    #[test]
    fn test_marking() {
        let mut marking = Marking::new();
        let t1 = Token::new_owned(0, TyGround::path("Counter"));
        let t2 = Token::new_owned(1, TyGround::path("Counter"));

        marking.add(0, t1);
        marking.add(0, t2);

        assert_eq!(marking.count(0), 2);
        assert_eq!(marking.total_tokens(), 2);

        let removed = marking.remove(0);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().vid, 0);
        assert_eq!(marking.count(0), 1);
    }

    #[test]
    fn test_unit_normalization() {
        // Test that TyGround::tuple() normalizes empty tuples to Unit
        let unit = TyGround::Unit;
        let via_tuple = TyGround::tuple(vec![]);

        // Both represent "()" in Rust
        assert_eq!(unit.short_name(), "()");
        assert_eq!(via_tuple.short_name(), "()");

        // After normalization, they should be equal
        assert_eq!(unit, via_tuple, "TyGround::tuple(vec![]) should normalize to Unit");
        
        // HashSet should only have one entry
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(unit.clone());
        set.insert(via_tuple.clone());
        assert_eq!(set.len(), 1, "Unit and normalized empty tuple should be the same in HashSet");

        // Non-empty tuples should remain as Tuple
        let pair = TyGround::tuple(vec![TyGround::Unit, TyGround::Unit]);
        assert!(matches!(pair, TyGround::Tuple(_)), "Non-empty tuple should be Tuple variant");
    }
}
