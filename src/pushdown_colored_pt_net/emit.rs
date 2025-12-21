//! Rust 代码生成器
//!
//! 从 witness trace 生成 Rust 测试代码

use crate::pushdown_colored_pt_net::runtime::*;
use crate::pushdown_colored_pt_net::search::WitnessStep;
use crate::pushdown_colored_pt_net::build::StructuralTransitionKind;
use crate::pushdown_colored_pt_net::types::*;
use std::collections::HashMap;

/// 代码生成器
pub struct CodeEmitter {
    /// 变量名映射: ValueId -> 变量名
    variables: HashMap<ValueId, String>,
    /// 下一个变量索引
    next_var_index: usize,
    /// 生成的代码行
    statements: Vec<String>,
}

impl CodeEmitter {
    /// 创建新的代码生成器
    pub fn new() -> Self {
        CodeEmitter {
            variables: HashMap::new(),
            next_var_index: 0,
            statements: Vec::new(),
        }
    }

    /// 生成变量名
    fn var_name(&mut self, value_id: ValueId) -> String {
        if let Some(name) = self.variables.get(&value_id) {
            name.clone()
        } else {
            let name = format!("v{}", self.next_var_index);
            self.next_var_index += 1;
            self.variables.insert(value_id, name.clone());
            name
        }
    }

    /// 从 witness 生成代码
    pub fn emit_witness(&mut self, witness: &[WitnessStep], transitions: &HashMap<TransitionId, TransitionInfo>) -> String {
        for step in witness {
            if let Some(info) = transitions.get(&step.transition_id) {
                self.emit_transition(info, step);
            }
        }

        self.statements.join("\n")
    }

    /// 生成单个变迁的代码
    fn emit_transition(&mut self, info: &TransitionInfo, step: &WitnessStep) {
        match &info.kind {
            TransitionKind::Structural(kind) => {
                match kind {
                    StructuralTransitionKind::Move => {
                        // Move 通常不需要显式代码
                    }
                    StructuralTransitionKind::CopyUse => {
                        // CopyUse 通常是隐式的
                    }
                    StructuralTransitionKind::DropOwn => {
                        // DropOwn: drop(variable)
                        if let Some(var) = self.get_var_from_choice(&step.choice) {
                            self.statements.push(format!("drop({});", var));
                        }
                    }
                    StructuralTransitionKind::BorrowShrOwn => {
                        // BorrowShrOwn: let r = &owner;
                        if let Some((owner, borrow)) = self.get_borrow_vars(&step.choice) {
                            self.statements.push(format!("let {} = &{};", borrow, owner));
                        }
                    }
                    StructuralTransitionKind::BorrowShrFrz => {
                        // BorrowShrFrz: let r2 = &*r1; (重新借用)
                        if let Some((source, target)) = self.get_borrow_vars(&step.choice) {
                            self.statements.push(format!("let {} = &*{};", target, source));
                        }
                    }
                    StructuralTransitionKind::BorrowMut => {
                        // BorrowMut: let r = &mut owner;
                        if let Some((owner, borrow)) = self.get_borrow_vars(&step.choice) {
                            self.statements.push(format!("let {} = &mut {};", borrow, owner));
                        }
                    }
                    StructuralTransitionKind::EndMut => {
                        // EndMut: drop(r);
                        if let Some(var) = self.get_var_from_choice(&step.choice) {
                            self.statements.push(format!("drop({});", var));
                        }
                    }
                    StructuralTransitionKind::EndShrKeep => {
                        // EndShrKeep: drop(r);
                        if let Some(var) = self.get_var_from_choice(&step.choice) {
                            self.statements.push(format!("drop({});", var));
                        }
                    }
                    StructuralTransitionKind::EndShrLast => {
                        // EndShrLast: drop(r);
                        if let Some(var) = self.get_var_from_choice(&step.choice) {
                            self.statements.push(format!("drop({});", var));
                        }
                    }
                    StructuralTransitionKind::ProjMove => {
                        // ProjMove: let f = owner.field;
                        if let Some((owner, field, field_var)) = self.get_field_proj_vars(&step.choice, info) {
                            self.statements.push(format!("let {} = {}.{};", field_var, owner, field));
                        }
                    }
                    StructuralTransitionKind::ProjShr => {
                        // ProjShr: let rf = &parent.field; 或 let rf = &(*parent).field;
                        if let Some((parent, field, field_ref)) = self.get_field_proj_vars(&step.choice, info) {
                            // 检查 parent 是否是引用
                            if parent.starts_with("&") {
                                self.statements.push(format!("let {} = &(*{}).{};", field_ref, parent, field));
                            } else {
                                self.statements.push(format!("let {} = &{}.{};", field_ref, parent, field));
                            }
                        }
                    }
                    StructuralTransitionKind::ProjMut => {
                        // ProjMut: let rf = &mut parent.field; 或 let rf = &mut (*parent).field;
                        if let Some((parent, field, field_ref)) = self.get_field_proj_vars(&step.choice, info) {
                            if parent.starts_with("&") {
                                self.statements.push(format!("let {} = &mut (*{}).{};", field_ref, parent, field));
                            } else {
                                self.statements.push(format!("let {} = &mut {}.{};", field_ref, parent, field));
                            }
                        }
                    }
                    StructuralTransitionKind::EndProjMut => {
                        // EndProjMut: drop(rf);
                        if let Some(var) = self.get_var_from_choice(&step.choice) {
                            self.statements.push(format!("drop({});", var));
                        }
                    }
                    StructuralTransitionKind::ImplWitness => {
                        // ImplWitness: 通常不需要显式代码
                    }
                    StructuralTransitionKind::AssocCast => {
                        // AssocCast: 类型转换
                        if let Some((from, to)) = self.get_cast_vars(&step.choice) {
                            self.statements.push(format!("let {} = {};", to, from));
                        }
                    }
                    StructuralTransitionKind::DupCopy => {
                        // DupCopy: let v2 = v1; (对于 Copy 类型)
                        if let Some((source, target)) = self.get_copy_vars(&step.choice) {
                            self.statements.push(format!("let {} = {};", target, source));
                        }
                    }
                }
            }
            TransitionKind::Signature { name, params, return_ty } => {
                // 签名诱导变迁: 生成函数调用
                self.emit_function_call(name, params, return_ty, &step.choice);
            }
        }
    }

    /// 从选择中获取变量名
    fn get_var_from_choice(&mut self, choice: &Choice) -> Option<String> {
        // 从 choice 中提取第一个值的 value_id
        for colors in choice.selections.values() {
            if let Some(color) = colors.first() {
                return Some(self.var_name(color.value_id));
            }
        }
        None
    }

    /// 获取借用变量对
    fn get_borrow_vars(&mut self, choice: &Choice) -> Option<(String, String)> {
        // 简化实现: 假设第一个是 owner,第二个是 borrow
        let mut vars: Vec<String> = choice.selections
            .values()
            .flatten()
            .map(|color| self.var_name(color.value_id))
            .collect();
        
        if vars.len() >= 2 {
            Some((vars[0].clone(), vars[1].clone()))
        } else if vars.len() == 1 {
            // 只有 borrow,需要生成 owner 变量名
            let borrow = vars[0].clone();
            let owner = format!("owner_{}", self.next_var_index);
            self.next_var_index += 1;
            Some((owner, borrow))
        } else {
            None
        }
    }

    /// 获取字段投影变量
    fn get_field_proj_vars(&mut self, choice: &Choice, info: &TransitionInfo) -> Option<(String, String, String)> {
        // 简化实现
        let mut vars: Vec<String> = choice.selections
            .values()
            .flatten()
            .map(|color| self.var_name(color.value_id))
            .collect();
        
        if let Some(field_name) = &info.field_name {
            if vars.len() >= 1 {
                let parent = vars[0].clone();
                let field_var = if vars.len() >= 2 {
                    vars[1].clone()
                } else {
                    format!("field_{}", self.next_var_index)
                };
                Some((parent, field_name.clone(), field_var))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 获取类型转换变量
    fn get_cast_vars(&mut self, choice: &Choice) -> Option<(String, String)> {
        let mut vars: Vec<String> = choice.selections
            .values()
            .flatten()
            .map(|color| self.var_name(color.value_id))
            .collect();
        
        if vars.len() >= 2 {
            Some((vars[0].clone(), vars[1].clone()))
        } else {
            None
        }
    }

    /// 获取复制变量
    fn get_copy_vars(&mut self, choice: &Choice) -> Option<(String, String)> {
        let mut vars: Vec<String> = choice.selections
            .values()
            .flatten()
            .map(|color| self.var_name(color.value_id))
            .collect();
        
        if vars.len() >= 2 {
            Some((vars[0].clone(), vars[1].clone()))
        } else {
            None
        }
    }

    /// 生成函数调用代码
    fn emit_function_call(
        &mut self,
        name: &str,
        params: &[(String, TypeExpr, crate::pushdown_colored_pt_net::build::ParamPassing)],
        return_ty: &Option<TypeExpr>,
        choice: &Choice,
    ) {
        // 从 choice 中提取参数变量
        let mut arg_vars: Vec<String> = choice.selections
            .values()
            .flatten()
            .map(|color| self.var_name(color.value_id))
            .collect();

        // 构建参数列表
        let args: Vec<String> = params.iter()
            .enumerate()
            .map(|(i, (param_name, _ty, passing))| {
                let var = if i < arg_vars.len() {
                    arg_vars[i].clone()
                } else {
                    format!("arg_{}", i)
                };
                
                match passing {
                    crate::pushdown_colored_pt_net::build::ParamPassing::ByValue => var,
                    crate::pushdown_colored_pt_net::build::ParamPassing::BySharedRef => {
                        if !var.starts_with("&") {
                            format!("&{}", var)
                        } else {
                            var
                        }
                    }
                    crate::pushdown_colored_pt_net::build::ParamPassing::ByMutRef => {
                        if !var.starts_with("&mut") {
                            format!("&mut {}", var)
                        } else {
                            var
                        }
                    }
                }
            })
            .collect();

        let args_str = args.join(", ");

        // 如果有返回值,生成赋值语句
        if return_ty.is_some() {
            let return_var = format!("result_{}", self.next_var_index);
            self.next_var_index += 1;
            self.statements.push(format!("let {} = {}({});", return_var, name, args_str));
        } else {
            self.statements.push(format!("{}({});", name, args_str));
        }
    }

    /// 生成完整的测试函数
    pub fn emit_test_function(&self, function_name: &str, body: &str) -> String {
        format!(
            "#[test]\nfn {}() {{\n    {}\n}}",
            function_name,
            body.split('\n').collect::<Vec<_>>().join("\n    ")
        )
    }
}

impl Default for CodeEmitter {
    fn default() -> Self {
        Self::new()
    }
}

/// 变迁信息 (用于代码生成)
#[derive(Debug, Clone)]
pub struct TransitionInfo {
    pub kind: TransitionKind,
    pub field_name: Option<String>,
}

/// 变迁种类
#[derive(Debug, Clone)]
pub enum TransitionKind {
    Structural(StructuralTransitionKind),
    Signature {
        name: String,
        params: Vec<(String, TypeExpr, crate::pushdown_colored_pt_net::build::ParamPassing)>,
        return_ty: Option<TypeExpr>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_emitter() {
        let mut emitter = CodeEmitter::new();
        let _var_name = emitter.var_name(ValueId::new(1));
        assert_eq!(_var_name, "v0");
    }
}
