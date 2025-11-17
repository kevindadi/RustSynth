use std::collections::BTreeSet;
use std::fmt::Write;

use rustdoc_types::{
    AssocItemConstraint, AssocItemConstraintKind, DynTrait, Function, FunctionPointer, GenericArg,
    GenericArgs, GenericBound, GenericParamDef, GenericParamDefKind, Path, Term, Type,
    WherePredicate,
};

pub(crate) struct TypeFormatter;

impl TypeFormatter {
    /// 将 `rustdoc_types::Type` 转换为用户可读且稳定的字符串.
    pub(crate) fn type_name(ty: &Type) -> String {
        crate::adapter::rust_type_name::rust_type_name(ty)
    }

    pub(crate) fn function_signature(func: &Function, name: &str) -> String {
        crate::adapter::rust_type_name::function_signature(func, name)
    }

    /// 收集类型中出现的全部生命周期标识符.
    pub(crate) fn collect_lifetimes(ty: &Type) -> Vec<String> {
        let mut lifetimes = BTreeSet::new();
        Self::collect_lifetimes_inner(ty, &mut lifetimes);
        lifetimes.into_iter().collect()
    }

    pub(crate) fn format_generic_param(param: &GenericParamDef) -> String {
        match &param.kind {
            GenericParamDefKind::Lifetime { outlives } => {
                if outlives.is_empty() {
                    format!("'{}", param.name)
                } else {
                    format!(
                        "'{}: {}",
                        param.name,
                        outlives
                            .iter()
                            .map(|s| format!("'{}", s))
                            .collect::<Vec<_>>()
                            .join(" + ")
                    )
                }
            }
            GenericParamDefKind::Type {
                bounds,
                default,
                is_synthetic,
            } => {
                let mut buf = param.name.clone();
                if !bounds.is_empty() {
                    let bounds_str = bounds
                        .iter()
                        .map(Self::format_generic_bound)
                        .collect::<Vec<_>>()
                        .join(" + ");
                    buf.push_str(": ");
                    buf.push_str(&bounds_str);
                }
                if let Some(default) = default {
                    if !*is_synthetic {
                        buf.push_str(" = ");
                        buf.push_str(&Self::type_name(default));
                    }
                }
                buf
            }
            GenericParamDefKind::Const { type_, default } => {
                let mut buf = format!("const {}: {}", param.name, Self::type_name(type_));
                if let Some(default) = default {
                    buf.push_str(" = ");
                    buf.push_str(default);
                }
                buf
            }
        }
    }

    pub(crate) fn format_generic_params(params: &[GenericParamDef]) -> Vec<String> {
        params
            .iter()
            .filter(|param| {
                !matches!(
                    param.kind,
                    GenericParamDefKind::Type {
                        is_synthetic: true,
                        ..
                    }
                )
            })
            .map(Self::format_generic_param)
            .collect()
    }

    pub(crate) fn format_where_predicate(predicate: &WherePredicate) -> String {
        match predicate {
            WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } => {
                let mut buf = String::new();
                if !generic_params.is_empty() {
                    buf.push_str("for<");
                    buf.push_str(
                        &generic_params
                            .iter()
                            .map(Self::format_generic_param)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    buf.push_str("> ");
                }
                buf.push_str(&Self::type_name(type_));
                if !bounds.is_empty() {
                    buf.push_str(": ");
                    buf.push_str(
                        &bounds
                            .iter()
                            .map(Self::format_generic_bound)
                            .collect::<Vec<_>>()
                            .join(" + "),
                    );
                }
                buf
            }
            WherePredicate::LifetimePredicate { lifetime, outlives } => {
                if outlives.is_empty() {
                    format!("'{}", lifetime)
                } else {
                    format!(
                        "'{}: {}",
                        lifetime,
                        outlives
                            .iter()
                            .map(|s| format!("'{}", s))
                            .collect::<Vec<_>>()
                            .join(" + ")
                    )
                }
            }
            WherePredicate::EqPredicate { lhs, rhs } => {
                format!("{} = {}", Self::type_name(lhs), Self::format_term(rhs))
            }
        }
    }

    pub(crate) fn format_where_predicates(predicates: &[WherePredicate]) -> Vec<String> {
        predicates
            .iter()
            .map(Self::format_where_predicate)
            .collect()
    }

    pub(crate) fn format_generic_bound(bound: &GenericBound) -> String {
        match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                modifier,
            } => {
                let mut buf = String::new();
                if !generic_params.is_empty() {
                    buf.push_str("for<");
                    buf.push_str(
                        &generic_params
                            .iter()
                            .map(Self::format_generic_param)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    buf.push_str("> ");
                }
                match modifier {
                    rustdoc_types::TraitBoundModifier::None => {}
                    rustdoc_types::TraitBoundModifier::Maybe => buf.push('?'),
                    rustdoc_types::TraitBoundModifier::MaybeConst => buf.push_str("~const "),
                }
                buf.push_str(&Self::path_to_string(trait_));
                buf
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
                        rustdoc_types::PreciseCapturingArg::Lifetime(name) => format!("'{}", name),
                        rustdoc_types::PreciseCapturingArg::Param(name) => name.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("use<{joined}>")
            }
        }
    }

    pub(crate) fn path_to_string(path: &Path) -> String {
        let mut buf = path.path.clone();
        if let Some(args) = &path.args {
            buf.push_str(&Self::generic_args_to_string(args));
        }
        buf
    }

    fn generic_args_to_string(args: &GenericArgs) -> String {
        match args {
            GenericArgs::AngleBracketed { args, constraints } => {
                let mut buf = String::new();
                buf.push('<');
                let mut first = true;
                for arg in args {
                    if !first {
                        buf.push_str(", ");
                    }
                    first = false;
                    buf.push_str(&Self::generic_arg_to_string(arg));
                }
                for constraint in constraints {
                    if !first {
                        buf.push_str(", ");
                    }
                    first = false;
                    buf.push_str(&Self::assoc_constraint_to_string(constraint));
                }
                buf.push('>');
                buf
            }
            GenericArgs::Parenthesized { inputs, output } => {
                let mut buf = String::new();
                buf.push('(');
                for (idx, ty) in inputs.iter().enumerate() {
                    if idx > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(&Self::type_name(ty));
                }
                buf.push(')');
                if let Some(output) = output {
                    buf.push_str(" -> ");
                    buf.push_str(&Self::type_name(output));
                }
                buf
            }
            GenericArgs::ReturnTypeNotation => String::from("()"),
        }
    }

    fn assoc_constraint_to_string(constraint: &AssocItemConstraint) -> String {
        let mut buf = String::new();
        write!(&mut buf, "{}", constraint.name).expect("write! to String never fails");
        if let Some(args) = &constraint.args {
            buf.push_str(&Self::generic_args_to_string(args));
        }
        match &constraint.binding {
            AssocItemConstraintKind::Equality(term) => {
                buf.push_str(" = ");
                buf.push_str(&Self::format_term(term));
            }
            AssocItemConstraintKind::Constraint(bounds) => {
                buf.push_str(": ");
                buf.push_str(
                    &bounds
                        .iter()
                        .map(Self::format_generic_bound)
                        .collect::<Vec<_>>()
                        .join(" + "),
                );
            }
        }
        buf
    }

    fn generic_arg_to_string(arg: &GenericArg) -> String {
        match arg {
            GenericArg::Lifetime(name) => format!("'{}", name),
            GenericArg::Type(ty) => Self::type_name(ty),
            GenericArg::Const(constant) => constant.expr.clone(),
            GenericArg::Infer => "_".into(),
        }
    }

    fn format_term(term: &Term) -> String {
        match term {
            Term::Type(ty) => Self::type_name(ty),
            Term::Constant(constant) => constant.expr.clone(),
        }
    }

    fn collect_lifetimes_inner(ty: &Type, lifetimes: &mut BTreeSet<String>) {
        match ty {
            Type::BorrowedRef {
                lifetime, type_, ..
            } => {
                if let Some(lifetime) = lifetime {
                    lifetimes.insert(lifetime.clone());
                }
                Self::collect_lifetimes_inner(type_, lifetimes);
            }
            Type::RawPointer { type_, .. } => {
                Self::collect_lifetimes_inner(type_, lifetimes);
            }
            Type::Slice(inner) | Type::Pat { type_: inner, .. } => {
                Self::collect_lifetimes_inner(inner, lifetimes);
            }
            Type::Array { type_, .. } => {
                Self::collect_lifetimes_inner(type_, lifetimes);
            }
            Type::Tuple(elements) => {
                for element in elements {
                    Self::collect_lifetimes_inner(element, lifetimes);
                }
            }
            Type::DynTrait(DynTrait { traits, lifetime }) => {
                if let Some(lifetime) = lifetime {
                    lifetimes.insert(lifetime.clone());
                }
                for poly in traits {
                    for gp in &poly.generic_params {
                        Self::collect_lifetimes_from_generic_param(gp, lifetimes);
                    }
                    if let Some(args) = &poly.trait_.args {
                        Self::collect_lifetimes_from_generic_args(args, lifetimes);
                    }
                }
            }
            Type::ImplTrait(bounds) => {
                for bound in bounds {
                    Self::collect_lifetimes_from_bound(bound, lifetimes);
                }
            }
            Type::FunctionPointer(pointer) => {
                let FunctionPointer {
                    sig,
                    generic_params,
                    ..
                } = pointer.as_ref();
                for gp in generic_params {
                    Self::collect_lifetimes_from_generic_param(gp, lifetimes);
                }
                for (_, ty) in &sig.inputs {
                    Self::collect_lifetimes_inner(ty, lifetimes);
                }
                if let Some(output) = &sig.output {
                    Self::collect_lifetimes_inner(output, lifetimes);
                }
            }
            Type::ResolvedPath(path) => {
                if let Some(args) = &path.args {
                    Self::collect_lifetimes_from_generic_args(args, lifetimes);
                }
            }
            Type::QualifiedPath {
                args,
                self_type,
                trait_,
                ..
            } => {
                Self::collect_lifetimes_inner(self_type, lifetimes);
                if let Some(args) = args {
                    Self::collect_lifetimes_from_generic_args(args, lifetimes);
                }
                if let Some(trait_) = trait_ {
                    if let Some(args) = &trait_.args {
                        Self::collect_lifetimes_from_generic_args(args, lifetimes);
                    }
                }
            }
            Type::Generic(_) | Type::Primitive(_) | Type::Infer => {}
        }
    }

    fn collect_lifetimes_from_generic_args(args: &GenericArgs, lifetimes: &mut BTreeSet<String>) {
        match args {
            GenericArgs::AngleBracketed { args, constraints } => {
                for arg in args {
                    match arg {
                        GenericArg::Lifetime(name) => {
                            lifetimes.insert(name.clone());
                        }
                        GenericArg::Type(ty) => Self::collect_lifetimes_inner(ty, lifetimes),
                        GenericArg::Const(_) | GenericArg::Infer => {}
                    }
                }
                for constraint in constraints {
                    if let Some(args) = &constraint.args {
                        Self::collect_lifetimes_from_generic_args(args, lifetimes);
                    }
                    match &constraint.binding {
                        AssocItemConstraintKind::Equality(term) => {
                            if let Term::Type(ty) = term {
                                Self::collect_lifetimes_inner(ty, lifetimes);
                            }
                        }
                        AssocItemConstraintKind::Constraint(bounds) => {
                            for bound in bounds {
                                Self::collect_lifetimes_from_bound(bound, lifetimes);
                            }
                        }
                    }
                }
            }
            GenericArgs::Parenthesized { inputs, output } => {
                for input in inputs {
                    Self::collect_lifetimes_inner(input, lifetimes);
                }
                if let Some(output) = output {
                    Self::collect_lifetimes_inner(output, lifetimes);
                }
            }
            GenericArgs::ReturnTypeNotation => {}
        }
    }

    fn collect_lifetimes_from_bound(bound: &GenericBound, lifetimes: &mut BTreeSet<String>) {
        match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                ..
            } => {
                if let Some(args) = &trait_.args {
                    Self::collect_lifetimes_from_generic_args(args, lifetimes);
                }
                for gp in generic_params {
                    Self::collect_lifetimes_from_generic_param(gp, lifetimes);
                }
            }
            GenericBound::Outlives(lifetime) => {
                lifetimes.insert(lifetime.clone());
            }
            GenericBound::Use(args) => {
                for arg in args {
                    if let rustdoc_types::PreciseCapturingArg::Lifetime(name) = arg {
                        lifetimes.insert(name.clone());
                    }
                }
            }
        }
    }

    fn collect_lifetimes_from_generic_param(
        param: &GenericParamDef,
        lifetimes: &mut BTreeSet<String>,
    ) {
        match &param.kind {
            GenericParamDefKind::Lifetime { outlives } => {
                lifetimes.extend(outlives.iter().cloned());
            }
            GenericParamDefKind::Type {
                bounds, default, ..
            } => {
                for bound in bounds {
                    Self::collect_lifetimes_from_bound(bound, lifetimes);
                }
                if let Some(default) = default {
                    Self::collect_lifetimes_inner(default, lifetimes);
                }
            }
            GenericParamDefKind::Const { type_, .. } => {
                Self::collect_lifetimes_inner(type_, lifetimes);
            }
        }
    }
}
