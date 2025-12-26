//! 代码生成器
//!
//! 从 API 序列生成 Rust 代码

use std::collections::HashMap;

use super::types::{TypeId, RustType};
use super::transition::{TransitionKind, SignatureInfo, ParamPassing, SelfKind};
use super::witness::Witness;
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

        // 处理返回值 - 不标注类型，让编译器推导
        let return_var = if let Some(ret_type_id) = sig.return_type {
            // 使用返回类型名称生成更有意义的变量名
            let type_short = sig.return_type_name.as_ref()
                .map(|n| Self::extract_base_type(n).to_lowercase())
                .unwrap_or_else(|| "result".to_string());
            let var = self.new_var(&type_short);
            stmt.push_str(&format!("let {} = ", var));
            // 注册这个变量到 available_vars
            self.available_vars.entry(ret_type_id).or_default().push(var.clone());
            Some(var)
        } else {
            stmt.push_str("let _ = ");
            None
        };

        // 生成调用表达式
        if let Some(ref self_kind) = sig.self_param {
            // 方法调用（有 self 参数）- 尝试从 available_vars 中找一个
            let self_expr = self.generate_self_expr_with_lookup(self_kind, sig);
            stmt.push_str(&format!("{}.{}(", self_expr, sig.name));
        } else if sig.owner_type.is_some() {
            // 关联函数（没有 self 参数，如 Type::new()）
            let owner = sig.owner_type.as_deref().unwrap();
            stmt.push_str(&format!("{}::{}(", owner, sig.name));
        } else if !sig.path.is_empty() && sig.path != sig.name {
            // 有路径的独立函数（如 crate::module::func）
            // 检查路径是否以模块格式存在
            if sig.path.contains("::") {
                // 完整路径
                stmt.push_str(&format!("{}::{}(", sig.path.rsplit("::").skip(1).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("::"), sig.name));
            } else {
                stmt.push_str(&format!("{}::{}(", sig.path, sig.name));
            }
        } else {
            // 独立函数或无法确定的情况
            stmt.push_str(&format!("{}(", sig.name));
        }

        // 生成参数（排除 self 参数）
        let params: Vec<String> = sig.params.iter()
            .filter(|p| p.name != "self")
            .map(|p| self.generate_param_expr_with_lookup(p))
            .collect();
        stmt.push_str(&params.join(", "));
        stmt.push_str(");");

        // 计算 import（使用 owner_path 或 path）
        let import = sig.owner_path.clone()
            .or_else(|| if !sig.path.is_empty() { Some(sig.path.clone()) } else { None });

        // 如果返回值变量被创建，但类型是 Copy，不要从 available_vars 中移除它
        let _ = return_var; // 目前只是注册，不移除

        (stmt, import)
    }

    /// 生成 self 表达式，优先使用已有变量
    fn generate_self_expr_with_lookup(&mut self, self_kind: &SelfKind, sig: &SignatureInfo) -> String {
        // 尝试从 available_vars 中找到匹配类型的变量
        // 注意：这里简化处理，使用类型名称匹配
        let owner_type = sig.owner_type.as_deref().unwrap_or("Unknown");
        let var_name = owner_type.to_lowercase();
        
        // 检查是否已有这个类型的变量
        for (type_id, vars) in &self.available_vars {
            if let Some(ty) = self.net.types.get(*type_id) {
                let type_short = ty.short_name().to_lowercase();
                if type_short == var_name && !vars.is_empty() {
                    let existing_var = vars.last().unwrap().clone();
                    return match self_kind {
                        SelfKind::Owned => existing_var,
                        SelfKind::Ref => format!("&{}", existing_var),
                        SelfKind::MutRef => format!("&mut {}", existing_var),
                    };
                }
            }
        }
        
        // 没有找到，生成一个 todo!
        format!("todo!(\"need {} instance\")", owner_type)
    }

    /// 生成参数表达式，优先使用已有变量
    fn generate_param_expr_with_lookup(&mut self, param: &super::transition::ParamInfo) -> String {
        // 清理类型名称
        let clean_type = Self::extract_base_type(&param.type_name);
        
        // 如果是外部类型或泛型，生成 todo!() 占位符
        if param.is_external || Self::is_generic_type(&clean_type) {
            let comment = if clean_type.len() <= 3 { 
                format!("/* {} */ ", clean_type) 
            } else { 
                String::new() 
            };
            return format!("{}todo!(\"{}\")", comment, param.name);
        }
        
        // 尝试从 available_vars 中找一个匹配类型的变量
        if let Some(vars) = self.available_vars.get(&param.type_id) {
            if !vars.is_empty() {
                let existing_var = vars.last().unwrap().clone();
                return match param.passing {
                    ParamPassing::ByRef => format!("&{}", existing_var),
                    ParamPassing::ByMutRef => format!("&mut {}", existing_var),
                    ParamPassing::ByValue => existing_var,
                };
            }
        }
        
        // 没有现有变量，生成值
        let base_expr = self.generate_value_for_type(&clean_type);
        
        // 根据传递方式添加引用（但避免双重引用）
        match param.passing {
            ParamPassing::ByRef => {
                if base_expr.starts_with('&') {
                    base_expr
                } else {
                    format!("&{}", base_expr)
                }
            }
            ParamPassing::ByMutRef => {
                if base_expr.starts_with("&mut ") {
                    base_expr
                } else {
                    format!("&mut {}", base_expr)
                }
            }
            ParamPassing::ByValue => base_expr,
        }
    }

    /// 清理类型名称（从 rustdoc 格式转换为 Rust 代码格式）
    fn clean_type_name(type_name: &str) -> String {
        // 处理 rustdoc-types 的调试格式
        let name = type_name.to_string();
        
        // 处理 Primitive("xxx")
        if name.starts_with("Primitive(\"") && name.ends_with("\")") {
            return name[11..name.len()-2].to_string();
        }
        
        // 处理 Generic("xxx")
        if name.starts_with("Generic(\"") && name.ends_with("\")") {
            let generic_name = &name[9..name.len()-2];
            // Self 类型需要特殊处理
            if generic_name == "Self" {
                return "Self".to_string();
            }
            // 其他泛型参数标记为占位符
            return format!("/* {} */ _", generic_name);
        }
        
        // 处理 ResolvedPath(Path { path: "xxx", ... })
        if name.contains("ResolvedPath(Path {") {
            // 提取 path: "xxx" 部分
            if let Some(path_start) = name.find("path: \"") {
                let rest = &name[path_start + 7..];
                if let Some(path_end) = rest.find('"') {
                    let path_name = &rest[..path_end];
                    // 处理 Result 和 Option
                    if path_name == "Result" || path_name == "Option" || path_name == "Vec" {
                        return path_name.to_string();
                    }
                    return path_name.to_string();
                }
            }
        }
        
        // 处理 BorrowedRef { ... }
        if name.starts_with("BorrowedRef {") {
            if name.contains("is_mutable: true") {
                return "&mut _".to_string();
            } else {
                return "&_".to_string();
            }
        }
        
        // 处理 QualifiedPath { name: "xxx", ... }
        if name.contains("QualifiedPath {") {
            if let Some(name_start) = name.find("name: \"") {
                let rest = &name[name_start + 7..];
                if let Some(name_end) = rest.find('"') {
                    return rest[..name_end].to_string();
                }
            }
        }
        
        // 如果是普通的类型名称，简化路径
        let mut result = name.clone();
        
        // 移除生命周期标注
        if let Ok(re) = regex::Regex::new(r"'[a-z_]+\s*") {
            result = re.replace_all(&result, "").to_string();
        }
        
        // 简化常见路径
        result = result.replace("&& ", "&");
        
        // 如果类型名称中有多个 ::，取最后一部分
        if result.contains("::") && !result.starts_with("Result<") && !result.starts_with("Option<") {
            if let Some(generic_start) = result.find('<') {
                let base = &result[..generic_start];
                let generics = &result[generic_start..];
                let short_base = base.rsplit("::").next().unwrap_or(base);
                result = format!("{}{}", short_base, generics);
            } else {
                result = result.rsplit("::").next().unwrap_or(&result).to_string();
            }
        }
        
        result
    }


    /// 提取基础类型名称（处理 rustdoc 格式）
    fn extract_base_type(type_str: &str) -> String {
        let s = type_str.trim();
        
        // 处理 Primitive("xxx")
        if s.starts_with("Primitive(\"") {
            if let Some(end) = s.find("\")") {
                return s[11..end].to_string();
            }
        }
        
        // 处理 Generic("xxx")
        if s.starts_with("Generic(\"") {
            if let Some(end) = s.find("\")") {
                return s[9..end].to_string();
            }
        }
        
        // 处理 ResolvedPath(Path { path: "xxx", ... })
        if s.contains("ResolvedPath(Path {") || s.contains("path: \"") {
            if let Some(path_start) = s.find("path: \"") {
                let rest = &s[path_start + 7..];
                if let Some(path_end) = rest.find('"') {
                    return rest[..path_end].to_string();
                }
            }
        }
        
        // 处理 BorrowedRef { ... type_: ... }
        if s.contains("BorrowedRef {") || s.contains("type_: ") {
            if let Some(type_start) = s.find("type_: ") {
                let rest = &s[type_start + 7..];
                // 递归提取内部类型
                return Self::extract_base_type(rest);
            }
        }
        
        // 处理 Slice(xxx)
        if s.starts_with("Slice(") {
            // 找到匹配的括号
            if let Some(paren_start) = s.find('(') {
                let inner = &s[paren_start + 1..];
                // 移除尾部的括号
                let inner = inner.trim_end_matches(|c| c == ')' || c == ' ');
                let elem_type = Self::extract_base_type(inner);
                return format!("[{}]", elem_type);
            }
        }
        
        // 处理带大括号的复杂类型 - 直接返回简化的占位符
        if s.contains("{") || s.contains("}") {
            // 尝试提取 path 或 name
            if let Some(path_start) = s.find("path: \"") {
                let rest = &s[path_start + 7..];
                if let Some(path_end) = rest.find('"') {
                    return rest[..path_end].to_string();
                }
            }
            if let Some(name_start) = s.find("name: \"") {
                let rest = &s[name_start + 7..];
                if let Some(name_end) = rest.find('"') {
                    return rest[..name_end].to_string();
                }
            }
            return "_".to_string();
        }
        
        s.to_string()
    }

    /// 检查是否是泛型类型
    fn is_generic_type(type_name: &str) -> bool {
        // 单个大写字母通常是泛型
        if type_name.len() == 1 && type_name.chars().all(|c| c.is_uppercase()) {
            return true;
        }
        // Self 也是泛型
        if type_name == "Self" {
            return true;
        }
        false
    }

    /// 为类型生成值
    fn generate_value_for_type(&self, type_name: &str) -> String {
        let clean = type_name.trim();
        
        // 基本类型
        match clean {
            "bool" => return "true".to_string(),
            "char" => return "'a'".to_string(),
            "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => return "0".to_string(),
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => return "0".to_string(),
            "f32" | "f64" => return "0.0".to_string(),
            "()" | "" => return "()".to_string(),
            _ => {}
        }
        
        // &str 和 String
        if clean == "&str" || clean == "str" {
            return "\"test\"".to_string();
        }
        if clean == "String" {
            return "String::new()".to_string();
        }
        
        // 切片类型
        if clean.starts_with("[") && clean.ends_with("]") {
            // [u8] -> &[]
            return "&[]".to_string();
        }
        
        // Vec
        if clean == "Vec" || clean.starts_with("Vec<") {
            return "Vec::new()".to_string();
        }
        
        // Option
        if clean == "Option" || clean.starts_with("Option<") {
            return "None".to_string();
        }
        
        // Result
        if clean == "Result" || clean.starts_with("Result<") {
            return "Ok(())".to_string();
        }
        
        // 常见类型
        match clean {
            "DecodeError" | "EncodeSliceError" | "DecodeSliceError" => {
                return "todo!(\"error value\")".to_string();
            }
            "DecodePaddingMode" => {
                return "DecodePaddingMode::RequireNone".to_string();
            }
            _ => {}
        }
        
        // 默认：尝试使用 Default trait
        format!("{}::default()", clean)
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

