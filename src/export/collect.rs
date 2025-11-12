use std::borrow::Borrow;

use anyhow::{Context, Result};
use rustdoc_types::{
    AssocItemConstraint, AssocItemConstraintKind, Function, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Item, ItemEnum, Path, PreciseCapturingArg, Term,
    TraitBoundModifier, Type, Visibility, WherePredicate,
};

use crate::{
    adapter::rust_type_name::{function_signature, rust_type_name},
    indexed_crate::ImportablePath,
    IndexedCrate,
};

use super::model::{FunctionInput, FunctionOutput, SourceLocation, SpecGenerics};

#[derive(Debug, Clone, Copy)]
pub struct CollectOptions {
    pub public_only: bool,
}

#[derive(Debug, Clone)]
pub struct CollectedItem {
    pub kind: String,
    pub path: String,
    pub visibility: String,
    pub signature: String,
    pub generics: SpecGenerics,
    pub inputs: Vec<FunctionInput>,
    pub output: FunctionOutput,
    pub docs: Option<String>,
    pub traits_bound: Vec<String>,
    pub source: SourceLocation,
    pub function: Function,
}

pub fn collect_items(indexed: &IndexedCrate<'_>, options: CollectOptions) -> Result<Vec<CollectedItem>> {
    let own_crate_id = indexed
        .inner
        .index
        .get(&indexed.inner.root)
        .map(|root| root.crate_id)
        .context("crate root item missing")?;
    let fn_owner_index = indexed
        .fn_owner_index
        .as_ref()
        .context("fn_owner_index not constructed")?;
    let mut items = Vec::new();
    for item in indexed.inner.index.values() {
        if item.crate_id != own_crate_id {
            continue;
        }
        let ItemEnum::Function(func) = &item.inner else {
            continue;
        };

        let owner = fn_owner_index.get(&item.id).copied();
        if options.public_only && !is_public_export(indexed, item, owner) {
            continue;
        }

        let owner_is_trait = owner.map_or(false, |owner_item| matches!(owner_item.inner, ItemEnum::Trait(_)));
        let kind = classify_kind(func, item, owner, owner_is_trait);
        let path = compute_path(indexed, item, owner);
        let visibility = format_visibility(item.visibility.clone(), owner);
        let signature = function_signature(func, item.name.as_deref().unwrap_or("<unnamed>"));
        let generics = extract_generics(func);
        let traits_bound = extract_trait_bounds(func);
        let inputs = extract_inputs(func);
        let output = extract_output(func);
        let docs = item.docs.clone();
        let source = extract_source(item);

        items.push(CollectedItem {
            kind,
            path,
            visibility,
            signature,
            generics,
            inputs,
            output,
            docs,
            traits_bound,
            source,
            function: func.clone(),
        });
    }

    items.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(items)
}

fn is_public_export<'a>(
    indexed: &IndexedCrate<'a>,
    item: &'a Item,
    owner: Option<&'a Item>,
) -> bool {
    match &item.visibility {
        Visibility::Public => true,
        Visibility::Default => owner
            .map(|owner_item| {
                matches!(owner_item.visibility, Visibility::Public)
                    && has_public_path(indexed, owner_item)
            })
            .unwrap_or(false),
        Visibility::Crate | Visibility::Restricted { .. } => false,
    }
}

fn has_public_path(indexed: &IndexedCrate<'_>, item: &Item) -> bool {
    !indexed.publicly_importable_names(&item.id).is_empty()
}

fn format_visibility(visibility: Visibility, owner: Option<&Item>) -> String {
    match visibility {
        Visibility::Public => "pub".into(),
        Visibility::Crate => "pub(crate)".into(),
        Visibility::Restricted { path, .. } => format!("pub(in {path})"),
        Visibility::Default => owner
            .map(|owner_item| match owner_item.visibility {
                Visibility::Public => "pub".into(),
                Visibility::Crate => "pub(crate)".into(),
                Visibility::Restricted { ref path, .. } => format!("pub(in {path})"),
                Visibility::Default => "private".into(),
            })
            .unwrap_or_else(|| "private".into()),
    }
}

fn compute_path(indexed: &IndexedCrate<'_>, item: &Item, owner: Option<&Item>) -> String {
    match owner {
        None => best_path(indexed, item),
        Some(owner_item) => {
            let owner_path = best_path(indexed, owner_item);
            let name = item.name.as_deref().unwrap_or("<unnamed>");
            if owner_path.is_empty() {
                name.to_string()
            } else {
                format!("{owner_path}::{name}")
            }
        }
    }
}

fn best_path(indexed: &IndexedCrate<'_>, item: &Item) -> String {
    let paths = indexed.publicly_importable_names(&item.id);
    if let Some(primary) = paths.first() {
        return join_import_path(primary);
    }

    indexed
        .inner
        .paths
        .get(&item.id)
        .map(|summary| summary.path.join("::"))
        .or_else(|| item.name.clone())
        .unwrap_or_else(|| "unknown".into())
}

fn join_import_path(path: &ImportablePath<'_>) -> String {
    let components: &[&str] = path.path.borrow();
    components.join("::")
}

fn extract_generics(func: &Function) -> SpecGenerics {
    let mut params = Vec::new();
    for param in &func.generics.params {
        if let Some(name) = format_generic_param_name(param) {
            params.push(name);
        }
    }

    let where_clauses = func
        .generics
        .where_predicates
        .iter()
        .map(format_where_predicate)
        .collect();
    SpecGenerics { params, where_clauses }
}

fn extract_trait_bounds(func: &Function) -> Vec<String> {
    let mut result = Vec::new();
    for param in &func.generics.params {
        match &param.kind {
            GenericParamDefKind::Type { bounds, is_synthetic, .. } => {
                if !is_synthetic && !bounds.is_empty() {
                    let bounds_str = bounds.iter().map(format_generic_bound).collect::<Vec<_>>().join(" + ");
                    result.push(format!("{}: {}", param.name, bounds_str));
                }
            }
            GenericParamDefKind::Const { .. } | GenericParamDefKind::Lifetime { .. } => continue,
        }
    }
    result.extend(
        func.generics
            .where_predicates
            .iter()
            .map(format_where_predicate)
            .filter(|clause| !clause.is_empty()),
    );
    result
}

fn extract_inputs(func: &Function) -> Vec<FunctionInput> {
    func.sig
        .inputs
        .iter()
        .map(|(name, ty)| {
            let clean_name = if name.trim().is_empty() { "_".into() } else { name.clone() };
            let type_string = format_type(ty);
            let by_ref = matches!(ty, Type::BorrowedRef { .. });
            let mutable = matches!(ty, Type::BorrowedRef { is_mutable: true, .. } | Type::RawPointer { is_mutable: true, .. });

            FunctionInput {
                name: clean_name,
                type_: type_string,
                by_ref,
                mutable,
            }
        })
        .collect()
}

fn extract_output(func: &Function) -> FunctionOutput {
    let type_ = func
        .sig
        .output
        .as_ref()
        .map(format_type)
        .unwrap_or_else(|| "()".into());
    FunctionOutput { type_ }
}

fn extract_source(item: &Item) -> SourceLocation {
    if let Some(span) = &item.span {
        SourceLocation {
            file: span.filename.to_string_lossy().into_owned(),
            line_span: [span.begin.0, span.end.0],
        }
    } else {
        SourceLocation::default()
    }
}

fn classify_kind(
    func: &Function,
    item: &Item,
    owner: Option<&Item>,
    owner_is_trait: bool,
) -> String {
    if owner.is_none() {
        return "function".into();
    }

    let takes_self = func
        .sig
        .inputs
        .first()
        .map(|(name, ty)| {
            let name = name.trim();
            if name == "self" || name.starts_with("self:") {
                return true;
            }

            matches!(
                ty,
                Type::Generic(s) if s == "Self"
                    || s.starts_with("Self::")
                    || s.starts_with("&Self")
                    || s.starts_with("&mut Self")
            ) || matches!(ty, Type::BorrowedRef { type_, .. } if matches!(**type_, Type::Generic(ref s) if s == "Self"))
        })
        .unwrap_or(false);

    let name = item.name.as_deref().unwrap_or_default();
    if !takes_self && name == "new" && !owner_is_trait {
        return "ctor".into();
    }

    if takes_self {
        "method".into()
    } else {
        "assoc_fn".into()
    }
}

fn format_type(ty: &Type) -> String {
    rust_type_name(ty)
}

fn format_generic_param_name(param: &GenericParamDef) -> Option<String> {
    match &param.kind {
        GenericParamDefKind::Type { is_synthetic, .. } => {
            if *is_synthetic {
                None
            } else {
                Some(param.name.clone())
            }
        }
        GenericParamDefKind::Lifetime { .. } => Some(format!("'{}", param.name)),
        GenericParamDefKind::Const { type_, .. } => Some(format!("const {}: {}", param.name, format_type(type_))),
    }
}

fn format_where_predicate(predicate: &WherePredicate) -> String {
    match predicate {
        WherePredicate::BoundPredicate {
            type_,
            bounds,
            generic_params,
        } => {
            let prefix = if generic_params.is_empty() {
                String::new()
            } else {
                format!(
                    "for<{}> ",
                    generic_params
                        .iter()
                        .filter_map(format_generic_param_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let bounds_str = bounds.iter().map(format_generic_bound).collect::<Vec<_>>().join(" + ");
            if bounds_str.is_empty() {
                format!("{prefix}{}", format_type(type_))
            } else {
                format!("{prefix}{}: {bounds_str}", format_type(type_))
            }
        }
        WherePredicate::LifetimePredicate { lifetime, outlives } => {
            if outlives.is_empty() {
                format!("'{}", lifetime)
            } else {
                let joined = outlives
                    .iter()
                    .map(|lt| format!("'{}", lt.trim_start_matches('\'')))
                    .collect::<Vec<_>>()
                    .join(" + ");
                format!("'{}: {joined}", lifetime.trim_start_matches('\''))
            }
        }
        WherePredicate::EqPredicate { lhs, rhs } => format!("{} = {}", format_type(lhs), format_term(rhs)),
    }
}

fn format_generic_bound(bound: &GenericBound) -> String {
    match bound {
        GenericBound::TraitBound {
            trait_,
            generic_params,
            modifier,
        } => {
            let prefix = if generic_params.is_empty() {
                String::new()
            } else {
                format!(
                    "for<{}> ",
                    generic_params
                        .iter()
                        .filter_map(format_generic_param_name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let modifier = match modifier {
                TraitBoundModifier::None => "",
                TraitBoundModifier::Maybe => "?",
                TraitBoundModifier::MaybeConst => "~const ",
            };
            format!("{prefix}{modifier}{}", format_path(trait_))
        }
        GenericBound::Outlives(lt) => {
            if lt.starts_with('\'') {
                lt.clone()
            } else {
                format!("'{}", lt)
            }
        }
        GenericBound::Use(args) => {
            let joined = args
                .iter()
                .map(|arg| match arg {
                    PreciseCapturingArg::Lifetime(name) => format!("'{}", name),
                    PreciseCapturingArg::Param(name) => name.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("use<{joined}>")
        }
    }
}

fn format_term(term: &Term) -> String {
    match term {
        Term::Type(ty) => format_type(ty),
        Term::Constant(constant) => constant.expr.clone(),
    }
}

fn format_path(path: &Path) -> String {
    let mut out = path.path.clone();
    if let Some(args) = &path.args {
        out.push_str(&format_generic_args(args));
    }
    out
}

fn format_generic_args(args: &GenericArgs) -> String {
    match args {
        GenericArgs::AngleBracketed { args, constraints } => {
            let mut parts: Vec<String> = args
                .iter()
                .map(|arg| match arg {
                    GenericArg::Type(ty) => format_type(ty),
                    GenericArg::Lifetime(lt) => format!("'{}", lt.trim_start_matches('\'')),
                    GenericArg::Const(c) => c.expr.clone(),
                    GenericArg::Infer => "_".into(),
                })
                .collect();
            parts.extend(constraints.iter().map(format_assoc_constraint));
            format!("<{}>", parts.join(", "))
        }
        GenericArgs::Parenthesized { inputs, output } => {
            let mut buf = String::new();
            buf.push('(');
            buf.push_str(
                &inputs
                    .iter()
                    .map(format_type)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            buf.push(')');
            if let Some(output_ty) = output {
                buf.push_str(" -> ");
                buf.push_str(&format_type(output_ty));
            }
            buf
        }
        GenericArgs::ReturnTypeNotation => String::new(),
    }
}

fn format_assoc_constraint(constraint: &AssocItemConstraint) -> String {
    let args = constraint
        .args
        .as_ref()
        .map(|args| format_generic_args(args))
        .unwrap_or_default();
    let base = if args.is_empty() {
        constraint.name.clone()
    } else {
        format!("{}{}", constraint.name, args)
    };
    match &constraint.binding {
        AssocItemConstraintKind::Equality(term) => format!("{base} = {}", format_term(term)),
        AssocItemConstraintKind::Constraint(bounds) => {
            let bounds_text = bounds.iter().map(format_generic_bound).collect::<Vec<_>>().join(" + ");
            if bounds_text.is_empty() {
                base
            } else {
                format!("{base}: {bounds_text}")
            }
        }
    }
}

