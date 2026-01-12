//! Rust Code Emitter - 从 Firing 序列生成可编译的 Rust 代码
//!
//! 生成规则：
//! - 变量命名: v{vid} (owned), r{vid} (reference)
//! - 显式 drop
//! - 不依赖 NLL
//! - 不用 unsafe
//!
//! ## 典型映射
//! | Transition | Rust Code |
//! |------------|-----------|
//! | CreateConst | `let v0: i32 = Default::default();` |
//! | BorrowShrFirst | `let r1 = &v0;` |
//! | BorrowShrNext | `let r2 = &v0;` |
//! | EndShr | `drop(r1);` |
//! | BorrowMut | `let r1 = &mut v0;` |
//! | EndMut | `drop(r1);` |
//! | MakeMutByMove | `let mut v1 = v0;` |
//! | MakeMutByCopy | `let mut v1 = v0;` (Copy) |
//! | Drop | `drop(v0);` |
//! | ApiCall | `let v1: T = func(v0);` |

use std::collections::HashMap;

use crate::simulator::{Firing, FiringKind, Token, VarId};
use crate::type_model::TypeKey;

/// 代码生成器
pub struct CodeEmitter {
    /// 生成的代码行
    lines: Vec<String>,
    /// 缩进级别
    indent: usize,
    /// 变量类型信息
    var_types: HashMap<VarId, TypeKey>,
    /// 变量是否是引用
    var_is_ref: HashMap<VarId, bool>,
    /// 引用的 owner
    ref_to_owner: HashMap<VarId, VarId>,
}

impl CodeEmitter {
    /// 创建新的代码生成器
    pub fn new() -> Self {
        CodeEmitter {
            lines: Vec::new(),
            indent: 1, // 函数体内部
            var_types: HashMap::new(),
            var_is_ref: HashMap::new(),
            ref_to_owner: HashMap::new(),
        }
    }

    /// 从 Firing 序列生成 Rust 代码
    pub fn emit(&mut self, trace: &[Firing]) -> String {
        self.lines.clear();
        self.var_types.clear();
        self.var_is_ref.clear();
        self.ref_to_owner.clear();

        // 文件头
        self.emit_header();

        // 主函数开始
        self.push_line("fn main() {");

        // 处理每个 firing
        for firing in trace {
            self.emit_firing(firing);
        }

        // 主函数结束
        self.push_line("}");

        self.lines.join("\n")
    }

    /// 生成文件头
    fn emit_header(&mut self) {
        self.push_line("//! 由 SyPetype PCPN Simulator 自动生成");
        self.push_line("//! ");
        self.push_line("//! 此代码展示了一条通过 Rust 借用检查的 API 调用序列。");
        self.push_line("//! 所有借用都是显式的，使用 drop() 显式结束借用。");
        self.push_line("");
        self.push_line("#![allow(unused_variables)]");
        self.push_line("#![allow(unused_mut)]");
        self.push_line("#![allow(dead_code)]");
        self.push_line("");
    }

    /// 处理单个 firing
    fn emit_firing(&mut self, firing: &Firing) {
        match &firing.kind {
            FiringKind::CreateConst { type_key } => {
                self.emit_create_const(firing, type_key);
            }
            FiringKind::ApiCall { fn_path } => {
                self.emit_api_call(firing, fn_path);
            }
            FiringKind::BorrowShrFirst { base_type } => {
                self.emit_borrow_shr_first(firing, base_type);
            }
            FiringKind::BorrowShrNext { base_type } => {
                self.emit_borrow_shr_next(firing, base_type);
            }
            FiringKind::EndShrKeepFrz { .. } | FiringKind::EndShrUnfreeze { .. } => {
                self.emit_end_shr(firing);
            }
            FiringKind::BorrowMut { base_type } => {
                self.emit_borrow_mut(firing, base_type);
            }
            FiringKind::EndMut { .. } => {
                self.emit_end_mut(firing);
            }
            FiringKind::MakeMutByMove { type_key } => {
                self.emit_make_mut_by_move(firing, type_key);
            }
            FiringKind::MakeMutByCopy { type_key } => {
                self.emit_make_mut_by_copy(firing, type_key);
            }
            FiringKind::Drop { type_key } => {
                self.emit_drop(firing, type_key);
            }
            FiringKind::CopyUse { type_key } => {
                self.emit_copy_use(firing, type_key);
            }
        }
    }

    /// 生成创建常量
    fn emit_create_const(&mut self, firing: &Firing, type_key: &TypeKey) {
        if let Some((_, token)) = firing.output_bindings.first() {
            let var_name = self.var_name(token.vid, false);
            let type_name = type_key.rust_type_name();
            let default_value = self.default_value(type_key);

            self.var_types.insert(token.vid, type_key.clone());
            self.var_is_ref.insert(token.vid, false);

            let comment = format!("// {}", firing.name);
            self.emit_line(&format!(
                "let {}: {} = {}; {}",
                var_name, type_name, default_value, comment
            ));
        }
    }

    /// 生成 API 调用
    fn emit_api_call(&mut self, firing: &Firing, fn_path: &str) {
        let mut args = Vec::new();
        let mut is_method = false;
        let mut receiver = String::new();

        for (i, (_, token)) in firing.input_bindings.iter().enumerate() {
            let is_ref = token.is_ref();
            let var_name = self.var_name(token.vid, is_ref);

            if i == 0 && fn_path.contains("::") {
                // 可能是方法调用
                let parts: Vec<_> = fn_path.split("::").collect();
                if parts.len() >= 2 {
                    // 检查是否是 self 参数
                    let type_name = parts[..parts.len() - 1].join("::");
                    if let Some(var_type) = self.var_types.get(&self.get_owner(token)) {
                        if var_type.short_name() == type_name || 
                           var_type.rust_type_name().contains(&type_name) {
                            is_method = true;
                            receiver = var_name.clone();
                            continue;
                        }
                    }
                }
            }
            args.push(var_name);
        }

        // 确定函数名
        let func_name = if is_method {
            fn_path.split("::").last().unwrap_or(fn_path).to_string()
        } else {
            fn_path.to_string()
        };

        // 生成调用代码
        let call_expr = if is_method {
            format!("{}.{}({})", receiver, func_name, args.join(", "))
        } else {
            format!("{}({})", func_name, args.join(", "))
        };

        // 处理返回值
        if let Some((_, output_token)) = firing.output_bindings.first() {
            let is_ref = output_token.is_ref();
            let var_name = self.var_name(output_token.vid, is_ref);
            
            self.var_is_ref.insert(output_token.vid, is_ref);

            // 根据 Token 的 bind_mut 属性决定是否输出 let mut
            let let_keyword = if output_token.bind_mut && !is_ref {
                "let mut"
            } else {
                "let"
            };

            let comment = format!("// {}", firing.name);
            self.emit_line(&format!("{} {} = {}; {}", let_keyword, var_name, call_expr, comment));
        } else {
            let comment = format!("// {}", firing.name);
            self.emit_line(&format!("{}; {}", call_expr, comment));
        }
    }

    /// 生成首次共享借用
    fn emit_borrow_shr_first(&mut self, firing: &Firing, base_type: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.len() < 2 {
            return;
        }

        let owner_token = &firing.input_bindings[0].1;
        let ref_token = &firing.output_bindings[1].1;

        let owner_var = self.var_name(owner_token.vid, false);
        let ref_var = self.var_name(ref_token.vid, true);

        self.var_types.insert(ref_token.vid, TypeKey::ref_shr(base_type.clone()));
        self.var_is_ref.insert(ref_token.vid, true);
        self.ref_to_owner.insert(ref_token.vid, owner_token.vid);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = &{}; {}", ref_var, owner_var, comment));
    }

    /// 生成后续共享借用
    fn emit_borrow_shr_next(&mut self, firing: &Firing, base_type: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.is_empty() {
            return;
        }

        let owner_token = &firing.input_bindings[0].1;
        let ref_token = &firing.output_bindings[0].1;

        let owner_var = self.var_name(owner_token.vid, false);
        let ref_var = self.var_name(ref_token.vid, true);

        self.var_types.insert(ref_token.vid, TypeKey::ref_shr(base_type.clone()));
        self.var_is_ref.insert(ref_token.vid, true);
        self.ref_to_owner.insert(ref_token.vid, owner_token.vid);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = &{}; {}", ref_var, owner_var, comment));
    }

    /// 生成结束共享借用
    fn emit_end_shr(&mut self, firing: &Firing) {
        if firing.input_bindings.len() < 2 {
            return;
        }

        let ref_token = &firing.input_bindings[1].1;
        let ref_var = self.var_name(ref_token.vid, true);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("drop({}); {}", ref_var, comment));
    }

    /// 生成可变借用
    fn emit_borrow_mut(&mut self, firing: &Firing, base_type: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.len() < 2 {
            return;
        }

        let owner_token = &firing.input_bindings[0].1;
        let ref_token = &firing.output_bindings[1].1;

        let owner_var = self.var_name(owner_token.vid, false);
        let ref_var = self.var_name(ref_token.vid, true);

        self.var_types.insert(ref_token.vid, TypeKey::ref_mut(base_type.clone()));
        self.var_is_ref.insert(ref_token.vid, true);
        self.ref_to_owner.insert(ref_token.vid, owner_token.vid);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = &mut {}; {}", ref_var, owner_var, comment));
    }

    /// 生成结束可变借用
    fn emit_end_mut(&mut self, firing: &Firing) {
        if firing.input_bindings.len() < 2 {
            return;
        }

        let ref_token = &firing.input_bindings[1].1;
        let ref_var = self.var_name(ref_token.vid, true);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("drop({}); {}", ref_var, comment));
    }

    /// 生成 MakeMutByMove
    fn emit_make_mut_by_move(&mut self, firing: &Firing, type_key: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.is_empty() {
            return;
        }

        let old_token = &firing.input_bindings[0].1;
        let new_token = &firing.output_bindings[0].1;

        let old_var = self.var_name(old_token.vid, false);
        let new_var = format!("mut v{}", new_token.vid);

        self.var_types.insert(new_token.vid, type_key.clone());
        self.var_is_ref.insert(new_token.vid, false);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = {}; {}", new_var, old_var, comment));
    }

    /// 生成 MakeMutByCopy
    fn emit_make_mut_by_copy(&mut self, firing: &Firing, type_key: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.is_empty() {
            return;
        }

        let src_token = &firing.input_bindings[0].1;
        let dst_token = &firing.output_bindings[0].1;

        let src_var = self.var_name(src_token.vid, false);
        let dst_var = format!("mut v{}", dst_token.vid);

        self.var_types.insert(dst_token.vid, type_key.clone());
        self.var_is_ref.insert(dst_token.vid, false);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = {}; {}", dst_var, src_var, comment));
    }

    /// 生成 Drop
    fn emit_drop(&mut self, firing: &Firing, _type_key: &TypeKey) {
        if firing.input_bindings.is_empty() {
            return;
        }

        let token = &firing.input_bindings[0].1;
        let var_name = self.var_name(token.vid, false);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("drop({}); {}", var_name, comment));
    }

    /// 生成 CopyUse
    fn emit_copy_use(&mut self, firing: &Firing, type_key: &TypeKey) {
        if firing.input_bindings.is_empty() || firing.output_bindings.is_empty() {
            return;
        }

        let src_token = &firing.input_bindings[0].1;
        let dst_token = &firing.output_bindings[0].1;

        let src_var = self.var_name(src_token.vid, false);
        let dst_var = self.var_name(dst_token.vid, false);

        self.var_types.insert(dst_token.vid, type_key.clone());
        self.var_is_ref.insert(dst_token.vid, false);

        let comment = format!("// {}", firing.name);
        self.emit_line(&format!("let {} = {}; {}", dst_var, src_var, comment));
    }

    /// 生成变量名
    fn var_name(&self, vid: VarId, is_ref: bool) -> String {
        if is_ref {
            format!("r{}", vid)
        } else {
            format!("v{}", vid)
        }
    }

    /// 获取引用的 owner vid
    fn get_owner(&self, token: &Token) -> VarId {
        if token.is_ref() {
            self.ref_to_owner.get(&token.vid).copied().unwrap_or(token.vid)
        } else {
            token.vid
        }
    }

    /// 获取类型的默认值
    fn default_value(&self, type_key: &TypeKey) -> String {
        match type_key {
            TypeKey::Primitive(name) => match name.as_str() {
                "bool" => "false".to_string(),
                "char" => "'\\0'".to_string(),
                "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => "0".to_string(),
                "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => "0".to_string(),
                "f32" | "f64" => "0.0".to_string(),
                "()" => "()".to_string(),
                _ => "Default::default()".to_string(),
            },
            _ => "Default::default()".to_string(),
        }
    }

    /// 添加一行代码（带缩进）
    fn emit_line(&mut self, line: &str) {
        let indent_str = "    ".repeat(self.indent);
        self.lines.push(format!("{}{}", indent_str, line));
    }

    /// 添加一行代码（无缩进）
    fn push_line(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }
}

impl Default for CodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}

/// 生成可编译的 Rust 代码
pub fn emit_rust_code(trace: &[Firing]) -> String {
    let mut emitter = CodeEmitter::new();
    emitter.emit(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_var_name() {
        let emitter = CodeEmitter::new();
        assert_eq!(emitter.var_name(0, false), "v0");
        assert_eq!(emitter.var_name(1, true), "r1");
    }

    #[test]
    fn test_default_value() {
        let emitter = CodeEmitter::new();
        assert_eq!(emitter.default_value(&TypeKey::primitive("i32")), "0");
        assert_eq!(emitter.default_value(&TypeKey::primitive("bool")), "false");
        assert_eq!(emitter.default_value(&TypeKey::path("Counter")), "Default::default()");
    }
}
