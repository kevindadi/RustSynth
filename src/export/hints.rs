use std::collections::BTreeMap;

use rustdoc_types::{Enum, GenericArg, GenericArgs, ItemEnum, Type};

use crate::IndexedCrate;

use super::{
    clean::CleanedDocs,
    collect::CollectedItem,
    model::{LlmHints, SpecInvariants, TypeHints},
};

pub struct HintsInput<'a> {
    pub collected: &'a CollectedItem,
    pub cleaned: &'a CleanedDocs,
    pub skip_panics: bool,
}

pub struct HintBundle {
    pub invariants: SpecInvariants,
    pub error_cases: Vec<String>,
    pub may_panic: Vec<String>,
    pub type_hints: TypeHints,
    pub llm_hints: LlmHints,
}

pub fn infer_hints(indexed: &IndexedCrate<'_>, input: &HintsInput<'_>) -> HintBundle {
    let mut invariants = SpecInvariants::default();
    invariants.preconditions = input.cleaned.sections.safety.clone();
    invariants.postconditions = input.cleaned.sections.returns.clone();

    let mut error_cases = input.cleaned.sections.errors.clone();
    let mut type_hints = TypeHints::default();

    if let Some((ty_name, variants)) = resolve_result_error(indexed, &input.collected.function) {
        if !variants.is_empty() {
            error_cases.extend(variants.iter().cloned());
            type_hints.enums.insert(ty_name, variants);
        }
    }

    let mut may_panic = if input.skip_panics {
        Vec::new()
    } else {
        input.cleaned.sections.panics.clone()
    };
    if !input.skip_panics {
        let details = input.cleaned.details.to_lowercase();
        if may_panic.is_empty() && details.contains("panic") {
            may_panic.push("Documentation mentions potential panic behavior".into());
        }
    }

    let mut llm_hints = build_llm_hints(&input.collected);
    merge_value_ranges(&input.collected, &mut type_hints);

    // Deduplicate outputs
    error_cases = dedup(error_cases);
    may_panic = dedup(may_panic);
    llm_hints.equivalence_classes = dedup(llm_hints.equivalence_classes);
    llm_hints.boundary_values = dedup(llm_hints.boundary_values);
    llm_hints.mutation_hotspots = dedup(llm_hints.mutation_hotspots);

    HintBundle {
        invariants,
        error_cases,
        may_panic,
        type_hints,
        llm_hints,
    }
}

fn resolve_result_error(
    indexed: &IndexedCrate<'_>,
    func: &rustdoc_types::Function,
) -> Option<(String, Vec<String>)> {
    let output = func.sig.output.as_ref()?;
    let Type::ResolvedPath(result_path) = output else {
        return None;
    };

    if !result_path.path.ends_with("Result") {
        return None;
    }

    let args = result_path.args.as_ref()?;
    let error_ty = match args.as_ref() {
        GenericArgs::AngleBracketed { args, .. } => args.get(1)?,
        _ => return None,
    };

    let GenericArg::Type(error_ty) = error_ty else {
        return None;
    };

    let Type::ResolvedPath(err_path) = error_ty else {
        return None;
    };

    let enum_item = indexed.inner.index.get(&err_path.id)?;
    let ItemEnum::Enum(Enum { variants, .. }) = &enum_item.inner else {
        return None;
    };

    let mut variant_names = Vec::new();
    for variant_id in variants {
        if let Some(variant) = indexed.inner.index.get(variant_id) {
            if let Some(name) = &variant.name {
                variant_names.push(name.clone());
            }
        }
    }

    let ty_name = err_path.path.clone();
    Some((ty_name, variant_names))
}

fn merge_value_ranges(collected: &CollectedItem, type_hints: &mut TypeHints) {
    let mut ranges = BTreeMap::new();
    for (name, ty) in &collected.function.sig.inputs {
        if let Some(range) = describe_range(ty) {
            ranges.insert(name.clone(), range);
        }
    }

    if let Some(out_ty) = &collected.function.sig.output {
        if let Some(range) = describe_range(out_ty) {
            ranges.insert("return".into(), range);
        }
    }

    if !ranges.is_empty() {
        type_hints.value_ranges.extend(ranges);
    }
}

fn describe_range(ty: &Type) -> Option<String> {
    match ty {
        Type::Primitive(name) => match name.as_str() {
            "u8" => Some("0..=255".into()),
            "u16" => Some("0..=65535".into()),
            "u32" => Some("0..=4_294_967_295".into()),
            "u64" => Some("0..=18_446_744_073_709_551_615".into()),
            "usize" => Some("platform usize range".into()),
            "i8" => Some("-128..=127".into()),
            "i16" => Some("-32768..=32767".into()),
            "i32" => Some("-2_147_483_648..=2_147_483_647".into()),
            "i64" => Some("-9_223_372_036_854_775_808..=9_223_372_036_854_775_807".into()),
            "isize" => Some("platform isize range".into()),
            "f32" => Some("IEEE-754 f32".into()),
            "f64" => Some("IEEE-754 f64".into()),
            _ => None,
        },
        Type::Array { len, .. } => Some(len.clone()),
        Type::BorrowedRef { type_, .. } => describe_range(type_.as_ref()),
        Type::QualifiedPath { self_type, .. } => describe_range(self_type.as_ref()),
        _ => None,
    }
}

fn build_llm_hints(collected: &CollectedItem) -> LlmHints {
    let mut equivalence = Vec::new();
    let mut boundaries = Vec::new();
    let mut hotspots = Vec::new();

    for (name, ty) in &collected.function.sig.inputs {
        hotspots.push(format!("mutate parameter `{}`", name));
        classify_type_for_hints(&mut equivalence, &mut boundaries, name, ty);
    }

    LlmHints {
        equivalence_classes: equivalence,
        boundary_values: boundaries,
        mutation_hotspots: hotspots,
    }
}

fn classify_type_for_hints(
    equivalence: &mut Vec<String>,
    boundaries: &mut Vec<String>,
    name: &str,
    ty: &Type,
) {
    match ty {
        Type::BorrowedRef { type_, .. } => classify_type_for_hints(equivalence, boundaries, name, type_.as_ref()),
        Type::Primitive(prim) if prim == "bool" => {
            equivalence.push(format!("`{name}` ∈ {{true, false}}"));
        }
        Type::Primitive(prim)
            if matches!(prim.as_str(), "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize") =>
        {
            boundaries.push(format!("`{name}` near numeric min / max"));
        }
        Type::Primitive(prim) if prim == "char" => {
            equivalence.push(format!("`{name}` as char: ASCII vs Unicode scalar"));
        }
        Type::Primitive(prim) if prim == "str" => {
            equivalence.push(format!("`{name}` as string: empty vs unicode vs long"));
        }
        Type::ResolvedPath(path) => {
            if path.path.ends_with("Option") {
                equivalence.push(format!("`{name}` option: None vs Some"));
            } else if path.path.ends_with("Result") {
                equivalence.push(format!("`{name}` result: Ok vs Err"));
            } else if path.path.ends_with("Vec") {
                boundaries.push(format!("`{name}` vector length: 0 vs large"));
            }
        }
        Type::Slice(_) | Type::Array { .. } => {
            boundaries.push(format!("`{name}` collection length edge cases"));
        }
        _ => {}
    }
}

fn dedup(mut values: Vec<String>) -> Vec<String> {
    values.sort();
    values.dedup();
    values
}

