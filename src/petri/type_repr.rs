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

    /// 从字符串创建 TypeDescriptor
    /// 自动检测借用类型(&, &mut, *const, *mut)
    pub fn from_string(s: &str) -> Self {
        let s = s.trim();

        let (borrow, type_str) = if let Some(rest) = s.strip_prefix("*mut ") {
            (BorrowKind::RawMutPtr, rest)
        } else if let Some(rest) = s.strip_prefix("*const ") {
            (BorrowKind::RawConstPtr, rest)
        } else if let Some(rest) = s.strip_prefix("&mut ") {
            (BorrowKind::MutRef, rest)
        } else if let Some(rest) = s.strip_prefix('&') {
            // 可能有 lifetime
            let rest = rest.trim_start();
            if rest.starts_with('\'') {
                // 跳过 lifetime
                let mut end = 1;
                while end < rest.len() && (rest.as_bytes()[end] as char).is_alphanumeric() {
                    end += 1;
                }
                let rest = rest[end..].trim_start();
                if let Some(rest) = rest.strip_prefix("mut ") {
                    (BorrowKind::MutRef, rest)
                } else {
                    (BorrowKind::SharedRef, rest)
                }
            } else if let Some(rest) = rest.strip_prefix("mut ") {
                (BorrowKind::MutRef, rest)
            } else {
                (BorrowKind::SharedRef, rest)
            }
        } else {
            (BorrowKind::Owned, s)
        };

        let canonical = Arc::<str>::from(type_str);

        Self {
            display: canonical.clone(),
            canonical,
            borrow,
            lifetimes: Vec::new(), // 从字符串创建时暂时不解析 lifetime
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

    /// 创建一个新的 TypeDescriptor,使用指定的借用类型
    pub fn with_borrow_kind(&self, borrow: BorrowKind) -> Self {
        Self {
            canonical: self.canonical.clone(),
            display: self.display.clone(),
            borrow,
            lifetimes: self.lifetimes.clone(),
        }
    }

    /// `T`、`&T`、`&mut T` 映射到同一个库所
    pub fn normalized(&self) -> Self {
        // 如果已经是 Owned,直接返回
        if matches!(self.borrow, BorrowKind::Owned) {
            return self.clone();
        }

        let strip_ref_prefix = |s: &str| -> String {
            let mut s = s.trim();
            if s.starts_with("*const ") {
                return s[7..].trim().to_string();
            }
            if s.starts_with("*mut ") {
                return s[5..].trim().to_string();
            }
            // 去掉引用前缀:&'lifetime mut 或 &'lifetime 或 &mut 或 &
            if s.starts_with('&') {
                s = &s[1..];
                while s.starts_with('\'') {
                    let mut end = 1;
                    while end < s.len() && (s.as_bytes()[end] as char).is_alphanumeric()
                        || s.as_bytes()[end] == b'_'
                    {
                        end += 1;
                    }
                    s = &s[end..].trim_start();
                }
                // 去掉 mut
                if s.starts_with("mut ") {
                    s = &s[4..];
                } else if s.starts_with("mut")
                    && (s.len() == 3 || !(s.as_bytes()[3] as char).is_alphabetic())
                {
                    s = &s[3..];
                }
                return s.trim().to_string();
            }
            s.to_string()
        };

        let canonical = Arc::<str>::from(strip_ref_prefix(self.canonical()));
        let display = Arc::<str>::from(strip_ref_prefix(self.display()));

        Self {
            canonical,
            display,
            borrow: BorrowKind::Owned,
            lifetimes: self.lifetimes.clone(),
        }
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

    /// 提取类型名称,去除路径信息
    /// 例如:`base64::alphabet::ParseAlphabetError` -> `ParseAlphabetError`
    /// 或者:`std::vec::Vec<i32>` -> `Vec<i32>` (保留泛型参数)
    /// 或者:`std::option::Option<String>` -> `Option<String>` (保留泛型参数)
    pub fn type_name_only(&self) -> Self {
        let extract_name = |s: &str| -> String {
            // 如果包含 "::",提取最后一部分(包括泛型参数)
            // 例如:`base64::alphabet::ParseAlphabetError` -> `ParseAlphabetError`
            // 例如:`std::vec::Vec<i32>` -> `Vec<i32>`
            if let Some((_, name)) = s.rsplit_once("::") {
                name.to_string()
            } else {
                s.to_string()
            }
        };

        Self {
            canonical: Arc::<str>::from(extract_name(self.canonical())),
            display: Arc::<str>::from(extract_name(self.display())),
            borrow: self.borrow,
            lifetimes: self.lifetimes.clone(),
        }
    }

    /// 获取去除泛型后的基础类型名称
    pub fn base_type_name(&self) -> String {
        strip_generics(self.type_name_only().display())
    }

    /// 获取类型使用的泛型参数(按出现顺序)
    pub fn generic_arguments(&self) -> Vec<String> {
        parse_generic_arguments(self.display())
    }

    /// 检查是否为泛型类型(如 T, U 等)
    /// 泛型类型不应该创建 Place,因为它们只是约束占位符
    pub fn is_generic(&self) -> bool {
        // 检查规范化后的类型名是否是单个标识符(可能是泛型参数)
        let normalized = self.normalized();
        let canonical = normalized.canonical();

        // 简单检查:如果规范化后的类型名是单个标识符,且不在已知的基本类型列表中
        // 则可能是泛型参数(但需要更精确的检查)
        // 更好的方法是检查原始的 Type 是否是 Type::Generic
        // 但这里我们只能根据字符串模式来判断

        // 如果包含 "::" 或 "<" 或 ">" 或 "(",则不是简单的泛型参数
        if canonical.contains("::")
            || canonical.contains('<')
            || canonical.contains('>')
            || canonical.contains('(')
        {
            return false;
        }

        // 检查是否是基本类型
        let _is_primitive = matches!(
            canonical.as_ref(),
            "i8" | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "bool"
                | "char"
                | "str"
                | "String"
                | "Vec"
                | "Option"
                | "Result"
                | "Box"
                | "Self"
        );

        // 如果不是基本类型且是单个标识符,可能是泛型参数
        // 但为了安全,我们需要更精确的检查
        // 这里先保守处理:只在明确知道是泛型时才返回 true
        false // 暂时返回 false,让调用方通过其他方式检查
    }
}

fn strip_generics(input: &str) -> String {
    if let Some(idx) = input.find('<') {
        input[..idx].trim().to_string()
    } else {
        input.trim().to_string()
    }
}

fn parse_generic_arguments(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let trimmed = input.trim();
    let start = match trimmed.find('<') {
        Some(idx) => idx + 1,
        None => return args,
    };

    let mut depth = 0;
    let mut current = String::new();
    for ch in trimmed[start..].chars() {
        match ch {
            '<' => {
                depth += 1;
                current.push(ch);
            }
            '>' => {
                if depth == 0 {
                    if !current.trim().is_empty() {
                        args.push(current.trim().to_string());
                    }
                    break;
                } else {
                    depth -= 1;
                    current.push(ch);
                }
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    args.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    args
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
