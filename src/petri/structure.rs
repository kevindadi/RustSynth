// 库所和变迁的结构定义
use rustdoc_types::{Enum, Function, Id, Struct, Type, Union, Variant};

#[derive(Clone, Debug)]
pub struct Place {
    pub id: Id,
    pub name: String,
    pub path: String,
    pub kind: PlaceKind,
}

#[derive(Clone, Debug)]
pub enum PlaceKind {
    Struct(Struct),
    Enum(Enum),
    Union(Union),
    Variant(Variant),
    StructField(Type),
    // 基本类型和复合类型
    Primitive(String),                            // bool, i32, u32, f32, char 等
    Tuple(Vec<Type>),                             // (T1, T2, ...)
    Slice(Box<Type>),                             // [T]
    Array(Box<Type>, String),                     // [T; N]，N 是字符串表达式
    Infer,                                        // _
    RawPointer(Box<Type>, bool),                  // *const T / *mut T，bool 表示 is_mutable
    BorrowedRef(Box<Type>, bool, Option<String>), // &T / &mut T，lifetime
}

impl Place {
    pub fn new(id: Id, name: String, path: String, kind: PlaceKind) -> Self {
        Self {
            id,
            name,
            path,
            kind,
        }
    }

    pub fn get_kind(&self) -> &PlaceKind {
        &self.kind
    }
}

impl PlaceKind {
    pub fn get_struct(&self) -> &Struct {
        match self {
            PlaceKind::Struct(struct_) => struct_,
            _ => panic!("Not a struct"),
        }
    }

    pub fn get_enum(&self) -> &Enum {
        match self {
            PlaceKind::Enum(enum_) => enum_,
            _ => panic!("Not an enum"),
        }
    }

    pub fn get_union(&self) -> &Union {
        match self {
            PlaceKind::Union(union) => union,
            _ => panic!("Not a union"),
        }
    }

    pub fn get_variant(&self) -> &Variant {
        match self {
            PlaceKind::Variant(variant) => variant,
            _ => panic!("Not a variant"),
        }
    }

    pub fn get_struct_field(&self) -> &Type {
        match self {
            PlaceKind::StructField(type_) => type_,
            _ => panic!("Not a struct field"),
        }
    }

    pub fn get_primitive(&self) -> &String {
        match self {
            PlaceKind::Primitive(name) => name,
            _ => panic!("Not a primitive"),
        }
    }

    pub fn get_tuple(&self) -> &Vec<Type> {
        match self {
            PlaceKind::Tuple(types) => types,
            _ => panic!("Not a tuple"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub id: Id,
    pub name: String,
    pub kind: TransitionKind,
}

#[derive(Clone, Debug)]
pub enum TransitionKind {
    Function(Function),
    Hold(Id, Id),
}

impl Transition {
    pub fn new(id: Id, name: String, kind: TransitionKind) -> Self {
        Self { id, name, kind }
    }

    pub fn get_kind(&self) -> &TransitionKind {
        &self.kind
    }
}

impl TransitionKind {
    pub fn get_function(&self) -> &Function {
        match self {
            TransitionKind::Function(function) => function,
            _ => panic!("Not a function"),
        }
    }

    pub fn get_hold(&self) -> (Id, Id) {
        match self {
            TransitionKind::Hold(owner, member) => (*owner, *member),
            _ => panic!("Not a hold"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub weight: u32,
    pub param_type: String,
    pub borrow_kind: BorrowKind,
}

#[derive(Clone, Debug)]
pub enum BorrowKind {
    Owned,
    Borrowed,
    BorrowedMut,
}

impl BorrowKind {
    pub fn get_borrow_kind(&self) -> &BorrowKind {
        &self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    pub item_id: Id,
}
