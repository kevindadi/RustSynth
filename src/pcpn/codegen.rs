//! 代码生成器
//!
//! 从 API 序列生成 Rust 代码

use std::collections::HashMap;

use super::types::{TypeId, RustType, TypeRegistry};
use super::transition::{Transition, TransitionKind, SignatureInfo, ParamPassing, SelfKind};
use super::witness::{Witness, WitnessStep};
use super::config::{PcpnConfig, GenerationMode};
use super::net::PcpnNet;

/// 代码生成器
pub struct CodeGenerator<'a> {
    net: &'a PcpnNet,
    config: &'a PcpnConfig,
    /// 变量计数器
    var_counter: usize,
    /// 类型 -> 变量名映射（用于追踪可用变量）
    available_vars: HashMap<TypeId, Vec<String>>,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(net: &'a PcpnNet, config: &'a PcpnConfig) -> Self {
        CodeGenerator {
            net,
            config,
            var_counter: 0,
            available_vars: HashMap::new(),
        }
    }

    /// 生成新的变量名
    fn new_var(&mut self, prefix: &str) -> String {
        let name = format!("{}_{}", prefix, self.var_counter);
        self.var_counter += 1;
        name
    }

    /// 注册可用变量
    fn register_var(&mut self, type_id: TypeId, var_name: String) {
        self.available_vars.entry(type_id).or_default().push(var_name);
    }

    /// 获取可用变量（消耗）
    fn take_var(&mut self, type_id: TypeId) -> Option<String> {
        self.available_vars.get_mut(&type_id)?.pop()
    }

    /// 获取可用变量（借用，不消耗）
    fn borrow_var(&self, type_id: TypeId) -> Option<&String> {
        self.available_vars.get(&type_id)?.last()
    }

    /// 从 witness 生成代码
    pub fn generate(&mut self, witness: &Witness) -> GeneratedCode {
        match self.config.generation_mode {
            GenerationMode::FullSequence => self.generate_full_sequence(witness),
            GenerationMode::TraceOnly => self.generate_trace_only(witness),
            GenerationMode::WithPlaceholders => self.generate_with_placeholders(witness),
            GenerationMode::MinimalHarness => self.generate_minimal_harness(witness),
        }
    }

    /// 生成完整序列代码
    fn generate_full_sequence(&mut self, witness: &Witness) -> GeneratedCode {
        let mut code = String::new();
        let mut imports = Vec::new();

        // 生成测试函数头
        if self.config.generate_test_harness {
            code.push_str("#[test]\n");
        }
        code.push_str("fn generated_api_sequence() {\n");

        // 处理每个 API 调用
        for step in witness.api_calls() {
            if let Some(trans) = self.net.get_transition(step.transition_id) {
                if let TransitionKind::Signature(sig) = &trans.kind {
                    let (stmt, import) = self.generate_api_call(sig);
                    code.push_str(&format!("    {}\n", stmt));
                    if let Some(imp) = import {
                        if !imports.contains(&imp) {
                            imports.push(imp);
                        }
                    }
                }
            }
        }

        code.push_str("}\n");

        // 生成完整的文件
        let mut full_code = String::new();

        // imports
        for imp in &imports {
            full_code.push_str(&format!("use {};\n", imp));
        }
        if !imports.is_empty() {
            full_code.push('\n');
        }

        full_code.push_str(&code);

        GeneratedCode {
            code: full_code,
            imports,
            api_trace: witness.api_calls().iter().map(|s| s.transition_name.clone()).collect(),
            is_complete: true,
            placeholders: Vec::new(),
        }
    }

    /// 生成单个 API 调用
    fn generate_api_call(&mut self, sig: &SignatureInfo) -> (String, Option<String>) {
        let mut stmt = String::new();

        // 处理返回值
        let return_var = if let Some(ret_type) = sig.return_type {
            let var = self.new_var("result");
            if self.config.include_type_annotations {
                if let Some(ty) = self.net.types.get(ret_type) {
                    stmt.push_str(&format!("let {}: {} = ", var, ty.short_name()));
                } else {
                    stmt.push_str(&format!("let {} = ", var));
                }
            } else {
                stmt.push_str(&format!("let {} = ", var));
            }
            self.register_var(ret_type, var.clone());
            Some(var)
        } else {
            None
        };

        // 生成调用表达式
        if sig.is_method {
            // 方法调用
            if let Some(self_kind) = &sig.self_param {
                // 需要一个 self 参数
                let self_expr = self.generate_self_expr(self_kind, sig);
                match self_kind {
                    SelfKind::Owned => {
                        stmt.push_str(&format!("{}.{}(", self_expr, sig.name));
                    }
                    SelfKind::Ref => {
                        stmt.push_str(&format!("{}.{}(", self_expr, sig.name));
                    }
                    SelfKind::MutRef => {
                        stmt.push_str(&format!("{}.{}(", self_expr, sig.name));
                    }
                }
            } else {
                // 关联函数
                stmt.push_str(&format!("{}::{}(", sig.path, sig.name));
            }
        } else {
            // 独立函数
            stmt.push_str(&format!("{}(", sig.path));
        }

        // 生成参数
        let params: Vec<String> = sig.params.iter()
            .map(|p| self.generate_param_expr(p.type_id, p.passing))
            .collect();
        stmt.push_str(&params.join(", "));
        stmt.push_str(");");

        // 计算 import
        let import = if !sig.path.is_empty() {
            Some(sig.path.clone())
        } else {
            None
        };

        (stmt, import)
    }

    /// 生成 self 表达式
    fn generate_self_expr(&mut self, self_kind: &SelfKind, sig: &SignatureInfo) -> String {
        // 尝试从可用变量中获取
        // 简化实现：生成一个占位符
        match self_kind {
            SelfKind::Owned => "value".to_string(),
            SelfKind::Ref => "&value".to_string(),
            SelfKind::MutRef => "&mut value".to_string(),
        }
    }

    /// 生成参数表达式
    fn generate_param_expr(&mut self, type_id: TypeId, passing: ParamPassing) -> String {
        if let Some(ty) = self.net.types.get(type_id) {
            match ty {
                RustType::Primitive(prim) => {
                    // 生成基本类型字面量
                    self.generate_primitive_literal(prim)
                }
                RustType::Named { path, .. } => {
                    // 尝试使用可用变量或生成默认值
                    match passing {
                        ParamPassing::ByRef => format!("&{}", self.generate_default_for_type(ty)),
                        ParamPassing::ByMutRef => format!("&mut {}", self.generate_default_for_type(ty)),
                        ParamPassing::ByValue => self.generate_default_for_type(ty),
                    }
                }
                _ => {
                    match passing {
                        ParamPassing::ByRef => "&todo!()".to_string(),
                        ParamPassing::ByMutRef => "&mut todo!()".to_string(),
                        ParamPassing::ByValue => "todo!()".to_string(),
                    }
                }
            }
        } else {
            "todo!()".to_string()
        }
    }

    /// 生成基本类型字面量
    fn generate_primitive_literal(&self, prim: &super::types::PrimitiveKind) -> String {
        use super::types::PrimitiveKind::*;
        match prim {
            Bool => "true".to_string(),
            Char => "'a'".to_string(),
            Str => "\"test\"".to_string(),
            U8 | U16 | U32 | U64 | U128 | Usize => "0".to_string(),
            I8 | I16 | I32 | I64 | I128 | Isize => "0".to_string(),
            F32 | F64 => "0.0".to_string(),
            Unit => "()".to_string(),
        }
    }

    /// 为类型生成默认值
    fn generate_default_for_type(&self, ty: &RustType) -> String {
        match ty {
            RustType::Primitive(prim) => self.generate_primitive_literal(prim),
            RustType::Named { path, type_args } => {
                let name = path.rsplit("::").next().unwrap_or(path);
                match name {
                    "String" => "String::new()".to_string(),
                    "Vec" => "Vec::new()".to_string(),
                    "Option" => "None".to_string(),
                    "Result" => "Ok(())".to_string(),
                    "HashMap" | "HashSet" => format!("{}::new()", name),
                    _ => format!("{}::default()", name),
                }
            }
            RustType::Tuple(types) if types.is_empty() => "()".to_string(),
            _ => "Default::default()".to_string(),
        }
    }

    /// 生成 trace only（仅 API 名称序列）
    fn generate_trace_only(&self, witness: &Witness) -> GeneratedCode {
        let trace: Vec<String> = witness.api_calls()
            .iter()
            .map(|s| s.transition_name.clone())
            .collect();

        let code = format!(
            "// API Trace ({} calls)\n// {}\n",
            trace.len(),
            trace.join(" -> ")
        );

        GeneratedCode {
            code,
            imports: Vec::new(),
            api_trace: trace,
            is_complete: false,
            placeholders: Vec::new(),
        }
    }

    /// 生成带占位符的代码
    fn generate_with_placeholders(&mut self, witness: &Witness) -> GeneratedCode {
        let mut code = String::new();
        let mut placeholders = Vec::new();

        code.push_str("#[test]\n");
        code.push_str("fn generated_api_sequence() {\n");

        for (i, step) in witness.api_calls().iter().enumerate() {
            if let Some(trans) = self.net.get_transition(step.transition_id) {
                if let TransitionKind::Signature(sig) = &trans.kind {
                    // 生成占位符变量
                    let placeholder = format!("__PLACEHOLDER_{}__", i);
                    placeholders.push(Placeholder {
                        id: placeholder.clone(),
                        expected_type: sig.return_type.and_then(|t| {
                            self.net.types.get(t).map(|ty| ty.short_name())
                        }),
                        context: format!("Return value of {}", sig.name),
                    });

                    code.push_str(&format!(
                        "    let {} = {}::{}({});\n",
                        placeholder,
                        sig.path,
                        sig.name,
                        sig.params.iter()
                            .enumerate()
                            .map(|(j, p)| format!("__PARAM_{}_{}__", i, j))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));

                    // 为每个参数添加占位符
                    for (j, param) in sig.params.iter().enumerate() {
                        placeholders.push(Placeholder {
                            id: format!("__PARAM_{}_{}__", i, j),
                            expected_type: self.net.types.get(param.type_id)
                                .map(|ty| ty.short_name()),
                            context: format!("Parameter '{}' of {}", param.name, sig.name),
                        });
                    }
                }
            }
        }

        code.push_str("}\n");

        GeneratedCode {
            code,
            imports: Vec::new(),
            api_trace: witness.api_calls().iter().map(|s| s.transition_name.clone()).collect(),
            is_complete: false,
            placeholders,
        }
    }

    /// 生成最小化 harness
    fn generate_minimal_harness(&self, witness: &Witness) -> GeneratedCode {
        let trace: Vec<String> = witness.api_calls()
            .iter()
            .map(|s| s.transition_name.clone())
            .collect();

        let code = format!(
            r#"//! Auto-generated test harness
//! 
//! API sequence ({} calls):
//! {}

#[test]
fn generated_test() {{
    // TODO: Implement the following API sequence
    // LLM should complete this implementation
    
    todo!("Complete the API sequence implementation")
}}
"#,
            trace.len(),
            trace.iter()
                .enumerate()
                .map(|(i, s)| format!("//! {}. {}", i + 1, s))
                .collect::<Vec<_>>()
                .join("\n")
        );

        GeneratedCode {
            code,
            imports: Vec::new(),
            api_trace: trace,
            is_complete: false,
            placeholders: Vec::new(),
        }
    }

    /// 生成类型上下文（用于 LLM 提示）
    pub fn generate_type_context(&self, witness: &Witness) -> String {
        let mut context = String::new();
        let mut seen_types = std::collections::HashSet::new();

        for step in witness.api_calls() {
            if let Some(trans) = self.net.get_transition(step.transition_id) {
                if let TransitionKind::Signature(sig) = &trans.kind {
                    // 收集参数类型
                    for param in &sig.params {
                        if seen_types.insert(param.type_id) {
                            if let Some(ty) = self.net.types.get(param.type_id) {
                                context.push_str(&format!("// {}: {}\n", param.name, ty.short_name()));
                            }
                        }
                    }
                    // 收集返回类型
                    if let Some(ret) = sig.return_type {
                        if seen_types.insert(ret) {
                            if let Some(ty) = self.net.types.get(ret) {
                                context.push_str(&format!("// return: {}\n", ty.short_name()));
                            }
                        }
                    }
                }
            }
        }

        context
    }

    /// 生成 LLM 提示词
    pub fn generate_llm_prompt(
        &self,
        witness: &Witness,
        crate_name: &str,
    ) -> String {
        let api_trace: Vec<String> = witness.api_calls()
            .iter()
            .map(|s| s.transition_name.clone())
            .collect();

        let type_context = self.generate_type_context(witness);

        self.config.llm_prompt_template.generate_prompt(
            crate_name,
            &api_trace,
            &type_context,
        )
    }
}

/// 生成的代码
#[derive(Debug, Clone)]
pub struct GeneratedCode {
    /// 生成的代码
    pub code: String,
    /// 需要的 imports
    pub imports: Vec<String>,
    /// API 调用序列
    pub api_trace: Vec<String>,
    /// 代码是否完整（可编译）
    pub is_complete: bool,
    /// 占位符列表
    pub placeholders: Vec<Placeholder>,
}

/// 占位符
#[derive(Debug, Clone)]
pub struct Placeholder {
    /// 占位符 ID
    pub id: String,
    /// 期望的类型
    pub expected_type: Option<String>,
    /// 上下文描述
    pub context: String,
}

impl GeneratedCode {
    /// 转换为 LLM 补全的格式
    pub fn to_llm_format(&self) -> String {
        let mut output = String::new();

        output.push_str("# Generated API Sequence\n\n");
        output.push_str("## API Trace\n\n");
        for (i, api) in self.api_trace.iter().enumerate() {
            output.push_str(&format!("{}. `{}`\n", i + 1, api));
        }
        output.push('\n');

        if !self.placeholders.is_empty() {
            output.push_str("## Placeholders to Fill\n\n");
            for placeholder in &self.placeholders {
                output.push_str(&format!(
                    "- `{}`: {} (type: {})\n",
                    placeholder.id,
                    placeholder.context,
                    placeholder.expected_type.as_deref().unwrap_or("unknown")
                ));
            }
            output.push('\n');
        }

        output.push_str("## Generated Code\n\n```rust\n");
        output.push_str(&self.code);
        output.push_str("```\n");

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_generator() {
        // 简单测试
        let config = PcpnConfig::default();
        let net = PcpnNet::new();
        let mut genr = CodeGenerator::new(&net, &config);

        let witness = Witness::empty();
        let code = genr.generate(&witness);

        assert!(code.code.contains("fn generated_api_sequence"));
    }
}

