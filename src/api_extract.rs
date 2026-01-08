//! API 提取：从 rustdoc Crate 中提取可调用的函数/方法签名

use anyhow::{Context, Result};
use rustdoc_types::{Crate, Item, ItemEnum};

use crate::model::{Capability, TypeKey};
use crate::type_norm::TypeContext;

/// 参数模式 (函数参数的类型+借用模式)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamMode {
    /// 按值传递 (需要 owned token)
    ByValue(TypeKey, bool), // (type, is_copy)
    /// 共享引用 (需要 shr token 或从 own 临时借用)
    SharedRef(TypeKey),
    /// 可变引用 (需要 mut token 或从 own 临时可变借用)
    MutRef(TypeKey),
}

impl ParamMode {
    /// 获取参数的 base TypeKey
    pub fn type_key(&self) -> &TypeKey {
        match self {
            ParamMode::ByValue(ty, _) => ty,
            ParamMode::SharedRef(ty) => ty,
            ParamMode::MutRef(ty) => ty,
        }
    }

    /// 获取所需的 capability
    pub fn required_capability(&self) -> Capability {
        match self {
            ParamMode::ByValue(_, _) => Capability::Own,
            ParamMode::SharedRef(_) => Capability::Shr,
            ParamMode::MutRef(_) => Capability::Mut,
        }
    }

    /// 是否允许从 owned 适配
    pub fn can_adapt_from_owned(&self) -> bool {
        matches!(self, ParamMode::SharedRef(_) | ParamMode::MutRef(_))
    }
}

/// 返回值模式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnMode {
    /// 返回 owned value
    OwnedValue(TypeKey, bool), // (type, is_copy)
    /// 返回共享引用 (必须绑定到某个输入的 origin)
    SharedRef(TypeKey),
    /// 返回可变引用
    MutRef(TypeKey),
    /// 无返回值 (unit)
    Unit,
}

/// API 签名 (函数/方法的抽象表示)
#[derive(Debug, Clone)]
pub struct ApiSignature {
    /// API 全称路径 (例如: "mycrate::module::function")
    pub full_path: String,
    /// 是否是方法 (有 self 参数)
    pub is_method: bool,
    /// self 参数模式 (如果是方法)
    pub self_mode: Option<ParamMode>,
    /// 其他参数
    pub params: Vec<ParamMode>,
    /// 返回值
    pub return_mode: ReturnMode,
    /// 是否是 unsafe
    pub is_unsafe: bool,
}

impl ApiSignature {
    /// 获取所有参数 (包括 self)
    pub fn all_params(&self) -> Vec<ParamMode> {
        let mut all = Vec::new();
        if let Some(self_mode) = &self.self_mode {
            all.push(self_mode.clone());
        }
        all.extend(self.params.clone());
        all
    }
}

/// 提取所有公开 API
pub fn extract_apis(
    krate: &Crate,
    type_ctx: &TypeContext,
    module_filter: &[String],
) -> Result<Vec<ApiSignature>> {
    let mut apis = Vec::new();
    
    // 首先收集所有 impl 块中的方法 ID，避免重复提取
    let mut impl_method_ids = std::collections::HashSet::new();
    for item in krate.index.values() {
        if let ItemEnum::Impl(impl_) = &item.inner {
            impl_method_ids.extend(impl_.items.iter().cloned());
        }
    }

    // 遍历所有 items
    for (id, item) in &krate.index {
        // Impl 块的可见性通常是 "default"，我们需要检查其中的方法是否 public
        let should_process = is_public(item) || matches!(item.inner, ItemEnum::Impl(_));
        
        if !should_process {
            continue;
        }

        // 模块过滤
        if !module_filter.is_empty() {
            let path = type_ctx.id_to_path.get(id).map(|s| s.as_str()).unwrap_or("");
            if !module_filter.iter().any(|m| path.starts_with(m)) {
                continue;
            }
        }

        // 提取函数/方法
        match &item.inner {
            ItemEnum::Function(func) => {
                // 跳过 impl 块中的方法（它们会在处理 Impl 时被提取）
                if impl_method_ids.contains(id) {
                    continue;
                }
                
                if is_public(item) {  // Function 必须是 public
                    if let Ok(sig) = extract_function_signature(item, &func.sig, type_ctx, false, None) {
                        apis.push(sig);
                    }
                }
            }
            ItemEnum::Impl(impl_) => {
                // 获取 impl 的 Self 类型
                tracing::debug!("Processing impl block, impl_.for_ = {:?}", impl_.for_);
                let self_type = type_ctx.normalize_type(&impl_.for_)
                    .ok()
                    .map(|(ty, _, _)| ty);
                
                tracing::debug!("Impl block: self_type = {:?}", self_type);
                
                // 提取 impl 块中的方法
                for item_id in &impl_.items {
                    if let Some(method_item) = type_ctx.get_item(item_id) {
                        if let ItemEnum::Function(func) = &method_item.inner {
                            if is_public(method_item) {
                                if let Ok(sig) = extract_function_signature(
                                    method_item,
                                    &func.sig,
                                    type_ctx,
                                    true,
                                    self_type.clone(),
                                ) {
                                    apis.push(sig);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(apis)
}

/// 检查 Item 是否公开
fn is_public(item: &Item) -> bool {
    use rustdoc_types::Visibility;
    matches!(item.visibility, Visibility::Public)
}

/// 提取函数签名
fn extract_function_signature(
    item: &Item,
    sig: &rustdoc_types::FunctionSignature,
    type_ctx: &TypeContext,
    is_method: bool,
    impl_self_type: Option<TypeKey>,
) -> Result<ApiSignature> {
    let full_path = item
        .name
        .clone()
        .context("函数缺少名称")?;

    // 解析参数
    let mut self_mode = None;
    let mut params = Vec::new();

    for (name, ty) in &sig.inputs {
        // 检查是否是 self 参数
        if name == "self" && is_method {
            self_mode = Some(parse_param_mode(ty, type_ctx, impl_self_type.as_ref())?);
        } else {
            params.push(parse_param_mode(ty, type_ctx, impl_self_type.as_ref())?);
        }
    }

    // 解析返回值
    let return_mode = if let Some(output) = &sig.output {
        parse_return_mode(output, type_ctx, impl_self_type.as_ref())?
    } else {
        ReturnMode::Unit
    };

    Ok(ApiSignature {
        full_path,
        is_method,
        self_mode,
        params,
        return_mode,
        is_unsafe: false,
    })
}

/// 解析参数模式
fn parse_param_mode(
    ty: &rustdoc_types::Type, 
    type_ctx: &TypeContext,
    impl_self_type: Option<&TypeKey>,
) -> Result<ParamMode> {
    let (type_key, cap, is_copy) = type_ctx.normalize_type_with_context(ty, impl_self_type)?;

    match cap {
        Capability::Own => Ok(ParamMode::ByValue(type_key, is_copy)),
        Capability::Shr => Ok(ParamMode::SharedRef(type_key)),
        Capability::Mut => Ok(ParamMode::MutRef(type_key)),
    }
}

/// 解析返回值模式
fn parse_return_mode(
    ty: &rustdoc_types::Type, 
    type_ctx: &TypeContext,
    impl_self_type: Option<&TypeKey>,
) -> Result<ReturnMode> {
    let (type_key, cap, is_copy) = type_ctx.normalize_type_with_context(ty, impl_self_type)?;

    match cap {
        Capability::Own => {
            if type_key == "()" {
                Ok(ReturnMode::Unit)
            } else {
                Ok(ReturnMode::OwnedValue(type_key, is_copy))
            }
        }
        Capability::Shr => Ok(ReturnMode::SharedRef(type_key)),
        Capability::Mut => Ok(ReturnMode::MutRef(type_key)),
    }
}

