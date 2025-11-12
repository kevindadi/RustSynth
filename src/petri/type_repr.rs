use std::sync::Arc;

use rustdoc_types::Type;
use serde::{Deserialize, Serialize};

use crate::petri::util::TypeFormatter;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorrowKind {
    Owned,
    SharedRef,
    MutRef,
    RawConstPtr,
    RawMutPtr,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeDescriptor {
    canonical: Arc<str>,
    display: Arc<str>,
    borrow: BorrowKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    lifetimes: Vec<Arc<str>>,
}

impl TypeDescriptor {
    pub fn from_type(ty: &Type) -> Self {
        let borrow = match ty {
            Type::BorrowedRef { is_mutable, .. } => {
                if *is_mutable {
                    BorrowKind::MutRef
                } else {
                    BorrowKind::SharedRef
                }
            }
            Type::RawPointer { is_mutable, .. } => {
                if *is_mutable {
                    BorrowKind::RawMutPtr
                } else {
                    BorrowKind::RawConstPtr
                }
            }
            _ => BorrowKind::Owned,
        };

        let canonical = Arc::<str>::from(TypeFormatter::type_name(ty));
        let lifetimes = TypeFormatter::collect_lifetimes(ty)
            .into_iter()
            .map(|lt| {
                if lt.starts_with('\'') {
                    Arc::<str>::from(lt)
                } else {
                    Arc::<str>::from(format!("'{}", lt))
                }
            })
            .collect::<Vec<_>>();

        Self {
            display: canonical.clone(),
            canonical,
            borrow,
            lifetimes,
        }
    }

    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn borrow_kind(&self) -> BorrowKind {
        self.borrow
    }

    pub fn lifetimes(&self) -> &[Arc<str>] {
        &self.lifetimes
    }

    pub(crate) fn replace_self(&self, replacement: &TypeDescriptor) -> Option<Self> {
        let display = self.display();
        let canonical = self.canonical();

        if !display.contains("Self") && !canonical.contains("Self") {
            return None;
        }

        let new_display = display.replace("Self", replacement.display());
        let new_canonical = canonical.replace("Self", replacement.canonical());

        Some(Self {
            display: Arc::<str>::from(new_display),
            canonical: Arc::<str>::from(new_canonical),
            borrow: self.borrow,
            lifetimes: self.lifetimes.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_self_in_borrowed() {
        let self_type = TypeDescriptor::from_type(&Type::ResolvedPath(rustdoc_types::Path {
            path: "crate::MyType".into(),
            id: rustdoc_types::Id(0),
            args: None,
        }));

        let borrowed_self = TypeDescriptor::from_type(&Type::BorrowedRef {
            lifetime: None,
            is_mutable: false,
            type_: Box::new(Type::Generic("Self".into())),
        });

        let replaced = borrowed_self
            .replace_self(&self_type)
            .expect("should replace Self");
        assert_eq!(replaced.display(), "&crate::MyType");
    }
}

