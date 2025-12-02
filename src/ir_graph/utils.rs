use rustdoc_types::{GenericArg, GenericArgs, Path, Type};

/// 从路径字符串中提取最后一段作为类型名
///
/// 例如: "std::vec::Vec" -> "Vec"
pub fn extract_type_name_from_path(path: &str) -> String {
    path.split("::").last().unwrap_or(path).to_string()
}

/// 从 Path 结构体中提取类型名
pub fn extract_type_name(path: &Path) -> String {
    extract_type_name_from_path(&path.path)
}

/// 提取 Result<T, E> 的类型参数
///
/// 返回 (Ok 类型, Err 类型)
pub fn extract_result_types(ty: &Type) -> Option<(Type, Type)> {
    if let Type::ResolvedPath(path) = ty {
        // 通过 path 识别 Result(可能是 std::result::Result, io::Result 等)
        let is_result = path.path.ends_with("Result")
            || path.path == "Result"
            || path.path.contains("::Result");

        if is_result {
            if let Some(args) = &path.args {
                if let GenericArgs::AngleBracketed { args, .. } = &**args {
                    if args.len() >= 2 {
                        // 标准 Result<T, E>
                        if let (GenericArg::Type(ok_type), GenericArg::Type(err_type)) =
                            (&args[0], &args[1])
                        {
                            return Some((ok_type.clone(), err_type.clone()));
                        }
                    } else if args.len() == 1 {
                        // TypeAlias 形式的 Result(如 io::Result<T>),第二个参数被固定
                        // 提取 T,并创建一个通用的 Error 类型
                        if let GenericArg::Type(ok_type) = &args[0] {
                            // 根据 path 推断 Error 类型
                            let error_type = if path.path.contains("io::") {
                                // io::Result -> io::Error
                                Type::ResolvedPath(Path {
                                    path: "io::Error".to_string(),
                                    id: path.id, // 使用相同的 id(外部类型)
                                    args: None,
                                })
                            } else {
                                // 其他情况,使用通用 Error
                                Type::Generic("Error".to_string())
                            };
                            return Some((ok_type.clone(), error_type));
                        }
                    }
                }
            }
        }
    }
    None
}

/// 提取 Option<T> 的类型参数
pub fn extract_option_type(ty: &Type) -> Option<Type> {
    if let Type::ResolvedPath(path) = ty {
        // 通过 path 识别 Option(可能是 std::option::Option 或 Option)
        let is_option = path.path.ends_with("Option")
            || path.path == "Option"
            || path.path.contains("::Option");

        if is_option {
            if let Some(args) = &path.args {
                if let GenericArgs::AngleBracketed { args, .. } = &**args {
                    if !args.is_empty() {
                        if let GenericArg::Type(some_type) = &args[0] {
                            return Some(some_type.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

/// 格式化类型标签(用于显示)
///
/// 将 Type 转换为可读的字符串表示
pub fn format_type_label(ty: &Type, context: &str) -> String {
    match ty {
        Type::Primitive(name) => name.clone(),
        Type::ResolvedPath(path) => extract_type_name(path),
        Type::Generic(name) => format!("{}:{}", context, name),
        Type::BorrowedRef {
            type_, is_mutable, ..
        } => {
            let inner = format_type_label(type_, context);
            if *is_mutable {
                format!("&mut {}", inner)
            } else {
                format!("&{}", inner)
            }
        }
        Type::Slice(inner) => format!("[{}]", format_type_label(inner, context)),
        Type::Array { type_, len } => {
            format!("[{}; {}]", format_type_label(type_, context), len)
        }
        Type::Tuple(elems) => {
            let elem_strs: Vec<_> = elems
                .iter()
                .map(|e| format_type_label(e, context))
                .collect();
            format!("({})", elem_strs.join(", "))
        }
        _ => format!("{:?}", ty),
    }
}

/// 清理类型名称(用于代码生成)
///
/// 将类型名转换为适合作为 Rust 标识符的格式
pub fn sanitize_type_name(type_name: &str) -> String {
    let mut result = type_name.to_string();

    // 处理元组类型 (T1, T2, ...) -> tuple_T1_T2_...
    if result.starts_with('(') && result.ends_with(')') {
        result = result
            .trim_start_matches('(')
            .trim_end_matches(')')
            .to_string();
        result = format!("tuple_{}", result);
    }

    // 处理切片类型 [T] -> array_T
    if result.starts_with('[') && result.ends_with(']') && !result.contains(';') {
        result = result
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        result = format!("array_{}", result);
    }

    // 处理固定大小数组 [T; N] -> array_T_N
    if result.starts_with('[') && result.contains(';') && result.ends_with(']') {
        result = result
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        result = format!("array_{}", result);
    }

    // 通用的符号替换
    result
        .replace("::", "_")
        .replace("<", "_")
        .replace(">", "")
        .replace(" ", "")
        .replace(",", "_")
        .replace("&", "")
        .replace("mut", "")
        .replace("[", "")
        .replace("]", "")
        .replace("(", "")
        .replace(")", "")
        .replace(";", "_")
}

/// 清理函数名称(用于代码生成)
///
/// 将函数路径转换为 PascalCase 格式
pub fn sanitize_func_name(func_name: &str) -> String {
    // 将 std::fs::File::open 转换为 StdFsFileOpen (PascalCase)
    // 简单的实现:按 :: 分割,首字母大写
    func_name
        .split("::")
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}
