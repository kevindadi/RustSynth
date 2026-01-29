//! 类型统一 (Unification) 和补全 (Completion)
//!
//! 实现论文中的泛型处理：先 unification 反推类型参数，
//! 再对未约束的泛型参数用有限地面类型宇宙做 completion。

use std::collections::HashMap;

use crate::types::{RegionLabel, TyGround, TyScheme};

pub type TypeVar = String;
pub type RegionVar = String;

#[derive(Clone, Debug, Default)]
pub struct Substitution {
    pub type_vars: HashMap<TypeVar, TyGround>,
    pub region_vars: HashMap<RegionVar, RegionLabel>,
}

impl Substitution {
    pub fn new() -> Self {
        Substitution {
            type_vars: HashMap::new(),
            region_vars: HashMap::new(),
        }
    }

    pub fn bind_type(&mut self, var: TypeVar, ty: TyGround) {
        self.type_vars.insert(var, ty);
    }

    pub fn bind_region(&mut self, var: RegionVar, label: RegionLabel) {
        self.region_vars.insert(var, label);
    }

    pub fn get_type(&self, var: &str) -> Option<&TyGround> {
        self.type_vars.get(var)
    }

    pub fn get_region(&self, var: &str) -> Option<RegionLabel> {
        self.region_vars.get(var).copied()
    }

    pub fn apply(&self, scheme: &TyScheme) -> Option<TyGround> {
        self.apply_ground(&scheme.base)
    }

    pub fn apply_ground(&self, ty: &TyGround) -> Option<TyGround> {
        match ty {
            TyGround::Primitive(_) | TyGround::Unit => Some(ty.clone()),
            TyGround::Path { name, args } => {
                if args.is_empty() && name.len() == 1 && name.chars().next().unwrap().is_uppercase()
                {
                    if let Some(bound) = self.type_vars.get(name) {
                        return Some(bound.clone());
                    }
                }

                let new_args: Option<Vec<_>> =
                    args.iter().map(|a| self.apply_ground(a)).collect();
                Some(TyGround::Path {
                    name: name.clone(),
                    args: new_args?,
                })
            }
            TyGround::Tuple(elems) => {
                let new_elems: Option<Vec<_>> =
                    elems.iter().map(|e| self.apply_ground(e)).collect();
                // Use TyGround::tuple() to normalize empty tuples to Unit
                Some(TyGround::tuple(new_elems?))
            }
        }
    }

    pub fn unify(scheme_ty: &TyGround, ground_ty: &TyGround) -> Option<Self> {
        let mut subst = Substitution::new();
        if Self::unify_inner(scheme_ty, ground_ty, &mut subst) {
            Some(subst)
        } else {
            None
        }
    }

    fn unify_inner(scheme: &TyGround, ground: &TyGround, subst: &mut Substitution) -> bool {
        match (scheme, ground) {
            (TyGround::Primitive(a), TyGround::Primitive(b)) => a == b,
            (TyGround::Unit, TyGround::Unit) => true,

            (TyGround::Path { name, args }, ground) if Self::is_type_var(name) && args.is_empty() => {
                if let Some(existing) = subst.type_vars.get(name) {
                    existing == ground
                } else {
                    subst.type_vars.insert(name.clone(), ground.clone());
                    true
                }
            }

            (
                TyGround::Path {
                    name: name1,
                    args: args1,
                },
                TyGround::Path {
                    name: name2,
                    args: args2,
                },
            ) => {
                if !Self::names_match(name1, name2) || args1.len() != args2.len() {
                    return false;
                }
                for (a1, a2) in args1.iter().zip(args2.iter()) {
                    if !Self::unify_inner(a1, a2, subst) {
                        return false;
                    }
                }
                true
            }

            (TyGround::Tuple(elems1), TyGround::Tuple(elems2)) => {
                if elems1.len() != elems2.len() {
                    return false;
                }
                for (e1, e2) in elems1.iter().zip(elems2.iter()) {
                    if !Self::unify_inner(e1, e2, subst) {
                        return false;
                    }
                }
                true
            }

            // Note: Unit already matched at the top of this function
            _ => false,
        }
    }

    fn is_type_var(name: &str) -> bool {
        name.len() == 1 && name.chars().next().unwrap().is_uppercase()
    }

    fn names_match(n1: &str, n2: &str) -> bool {
        n1 == n2 || n1.split("::").last() == n2.split("::").last()
    }

    pub fn join(&self, other: &Self) -> Option<Self> {
        let mut result = self.clone();

        for (var, ty) in &other.type_vars {
            if let Some(existing) = result.type_vars.get(var) {
                if existing != ty {
                    return None;
                }
            } else {
                result.type_vars.insert(var.clone(), ty.clone());
            }
        }

        for (var, label) in &other.region_vars {
            if let Some(existing) = result.region_vars.get(var) {
                if existing != label {
                    return None;
                }
            } else {
                result.region_vars.insert(var.clone(), *label);
            }
        }

        Some(result)
    }

    pub fn complete(&mut self, unbound: &[TypeVar], universe: &[TyGround], bounds: &[(String, Vec<String>)]) {
        for var in unbound {
            if self.type_vars.contains_key(var) {
                continue;
            }

            let var_bounds: Vec<&str> = bounds
                .iter()
                .filter(|(v, _)| v == var)
                .flat_map(|(_, bs)| bs.iter().map(|s| s.as_str()))
                .collect();

            for candidate in universe {
                if Self::satisfies_bounds(candidate, &var_bounds) {
                    self.type_vars.insert(var.clone(), candidate.clone());
                    break;
                }
            }
        }
    }

    fn satisfies_bounds(ty: &TyGround, bounds: &[&str]) -> bool {
        for bound in bounds {
            let satisfied = match *bound {
                "Copy" => ty.is_copy(),
                "Clone" => true,
                "Default" => ty.is_primitive(),
                "Send" | "Sync" => true,
                _ => true,
            };
            if !satisfied {
                return false;
            }
        }
        true
    }

    pub fn is_complete(&self, type_vars: &[String]) -> bool {
        type_vars.iter().all(|v| self.type_vars.contains_key(v))
    }
}

#[derive(Clone, Debug)]
pub struct TypeUniverse {
    pub types: Vec<TyGround>,
}

impl TypeUniverse {
    pub fn new() -> Self {
        TypeUniverse { types: Vec::new() }
    }

    pub fn with_primitives() -> Self {
        let mut universe = TypeUniverse::new();
        for prim in &["i32", "u32", "i64", "u64", "bool", "usize"] {
            universe.types.push(TyGround::primitive(prim));
        }
        universe
    }

    pub fn add(&mut self, ty: TyGround) {
        if !self.types.contains(&ty) {
            self.types.push(ty);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &TyGround> {
        self.types.iter()
    }

    pub fn candidates_for_bounds(&self, bounds: &[String]) -> Vec<&TyGround> {
        let bound_strs: Vec<&str> = bounds.iter().map(|s| s.as_str()).collect();
        self.types
            .iter()
            .filter(|ty| Substitution::satisfies_bounds(ty, &bound_strs))
            .collect()
    }
}

impl Default for TypeUniverse {
    fn default() -> Self {
        Self::with_primitives()
    }
}

pub fn enumerate_instantiations(
    type_vars: &[(String, Vec<String>)],
    universe: &TypeUniverse,
) -> Vec<Substitution> {
    if type_vars.is_empty() {
        return vec![Substitution::new()];
    }

    let mut results = Vec::new();
    enumerate_helper(type_vars, 0, Substitution::new(), universe, &mut results);
    results
}

fn enumerate_helper(
    type_vars: &[(String, Vec<String>)],
    idx: usize,
    current: Substitution,
    universe: &TypeUniverse,
    results: &mut Vec<Substitution>,
) {
    if idx >= type_vars.len() {
        results.push(current);
        return;
    }

    let (var_name, bounds) = &type_vars[idx];
    let candidates = universe.candidates_for_bounds(bounds);

    if candidates.is_empty() {
        return;
    }

    for candidate in candidates {
        let mut new_subst = current.clone();
        new_subst.type_vars.insert(var_name.clone(), candidate.clone());
        enumerate_helper(type_vars, idx + 1, new_subst, universe, results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unify_simple() {
        let scheme = TyGround::path("T");
        let ground = TyGround::primitive("i32");

        let subst = Substitution::unify(&scheme, &ground).unwrap();
        assert_eq!(subst.type_vars.get("T"), Some(&TyGround::primitive("i32")));
    }

    #[test]
    fn test_unify_generic_path() {
        let scheme = TyGround::path_with_args("Vec", vec![TyGround::path("T")]);
        let ground = TyGround::path_with_args("Vec", vec![TyGround::primitive("u8")]);

        let subst = Substitution::unify(&scheme, &ground).unwrap();
        assert_eq!(subst.type_vars.get("T"), Some(&TyGround::primitive("u8")));
    }

    #[test]
    fn test_unify_mismatch() {
        let scheme = TyGround::primitive("i32");
        let ground = TyGround::primitive("u32");

        assert!(Substitution::unify(&scheme, &ground).is_none());
    }

    #[test]
    fn test_join_substitutions() {
        let mut s1 = Substitution::new();
        s1.bind_type("T".to_string(), TyGround::primitive("i32"));

        let mut s2 = Substitution::new();
        s2.bind_type("U".to_string(), TyGround::primitive("u32"));

        let joined = s1.join(&s2).unwrap();
        assert_eq!(joined.type_vars.len(), 2);
    }

    #[test]
    fn test_join_conflict() {
        let mut s1 = Substitution::new();
        s1.bind_type("T".to_string(), TyGround::primitive("i32"));

        let mut s2 = Substitution::new();
        s2.bind_type("T".to_string(), TyGround::primitive("u32"));

        assert!(s1.join(&s2).is_none());
    }

    #[test]
    fn test_complete() {
        let mut subst = Substitution::new();
        let universe = TypeUniverse::with_primitives();
        let bounds = vec![("T".to_string(), vec!["Copy".to_string()])];

        subst.complete(&["T".to_string()], &universe.types, &bounds);

        assert!(subst.type_vars.contains_key("T"));
        assert!(subst.type_vars.get("T").unwrap().is_copy());
    }

    #[test]
    fn test_enumerate_instantiations() {
        let universe = TypeUniverse::with_primitives();
        let type_vars = vec![("T".to_string(), vec!["Copy".to_string()])];

        let results = enumerate_instantiations(&type_vars, &universe);
        assert!(!results.is_empty());

        for subst in &results {
            assert!(subst.type_vars.contains_key("T"));
        }
    }

    #[test]
    fn test_apply_substitution() {
        let mut subst = Substitution::new();
        subst.bind_type("T".to_string(), TyGround::primitive("i32"));

        let scheme = TyScheme::ground(TyGround::path_with_args(
            "Vec",
            vec![TyGround::path("T")],
        ));

        let applied = subst.apply(&scheme).unwrap();
        match applied {
            TyGround::Path { args, .. } => {
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], TyGround::primitive("i32"));
            }
            _ => panic!("Expected Path"),
        }
    }
}
