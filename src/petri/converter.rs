//! rustdoc.json 到 RepairPetriNet JSON 的转换器
//! 
//! 这个模块提供了将 Rust API 文档 (rustdoc.json) 转换为
//! RepairPetriNet JSON 格式的核心功能。

use std::fs;
use std::path::Path;
use rustdoc_types::Crate;

use super::{PetriNetBuilder, JsonPetriNet};

/// 将 rustdoc.json 转换为 RepairPetriNet JSON 格式
/// 
/// # 参数
/// 
/// * `rustdoc_json_str` - rustdoc.json 的 JSON 字符串内容
/// 
/// # 返回
/// 
/// 成功时返回 RepairPetriNet JSON 字符串，失败时返回错误信息
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use trustfall_rustdoc_adapter::petri::convert_rustdoc_to_petri;
/// 
/// let rustdoc_json = std::fs::read_to_string("target/doc/my_crate.json")?;
/// let petri_net_json = convert_rustdoc_to_petri(&rustdoc_json)?;
/// std::fs::write("petri_net.json", petri_net_json)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_rustdoc_to_petri(rustdoc_json_str: &str) -> Result<String, Box<dyn std::error::Error>> {
    // 1. 解析 rustdoc.json
    let crate_data: Crate = serde_json::from_str(rustdoc_json_str)?;
    
    // 2. 构建 Petri 网
    let petri_net = PetriNetBuilder::from_crate(&crate_data);
    
    // 3. 转换为 JSON 表示
    let mut json_net = JsonPetriNet::from(&petri_net);
    
    // 4. 填充元数据
    json_net.metadata.crate_name = crate_data.index.get(&crate_data.root)
        .map(|item| item.name.as_ref().map(|n| n.to_string()))
        .flatten();
    
    json_net.metadata.rustdoc_version = Some(format!("{}.{}.{}", 
        crate_data.format_version / 100,
        (crate_data.format_version / 10) % 10,
        crate_data.format_version % 10
    ));
    
    json_net.metadata.timestamp = Some(chrono::Utc::now().to_rfc3339());
    
    // 5. 序列化为 JSON 字符串
    let json_string = json_net.to_json_string()?;
    
    Ok(json_string)
}

/// 从文件读取 rustdoc.json 并转换为 RepairPetriNet JSON
/// 
/// # 参数
/// 
/// * `rustdoc_json_path` - rustdoc.json 文件路径
/// 
/// # 返回
/// 
/// 成功时返回 RepairPetriNet JSON 字符串
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use trustfall_rustdoc_adapter::petri::convert_rustdoc_file_to_petri;
/// 
/// let petri_net_json = convert_rustdoc_file_to_petri("target/doc/my_crate.json")?;
/// println!("{}", petri_net_json);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_rustdoc_file_to_petri<P: AsRef<Path>>(
    rustdoc_json_path: P,
) -> Result<String, Box<dyn std::error::Error>> {
    let rustdoc_json_str = fs::read_to_string(rustdoc_json_path)?;
    convert_rustdoc_to_petri(&rustdoc_json_str)
}

/// 从 rustdoc.json 文件转换并保存为 RepairPetriNet JSON 文件
/// 
/// # 参数
/// 
/// * `rustdoc_json_path` - rustdoc.json 文件路径
/// * `output_path` - 输出的 RepairPetriNet JSON 文件路径
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use trustfall_rustdoc_adapter::petri::convert_and_save;
/// 
/// convert_and_save(
///     "target/doc/my_crate.json",
///     "petri_net.json"
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_and_save<P1: AsRef<Path>, P2: AsRef<Path>>(
    rustdoc_json_path: P1,
    output_path: P2,
) -> Result<(), Box<dyn std::error::Error>> {
    let petri_net_json = convert_rustdoc_file_to_petri(rustdoc_json_path)?;
    fs::write(output_path, petri_net_json)?;
    Ok(())
}

/// 高级转换选项
#[derive(Debug, Clone)]
pub struct ConversionOptions {
    /// 是否包含私有项
    pub include_private: bool,
    /// 是否包含测试模块
    pub include_tests: bool,
    /// 是否包含文档注释
    pub include_docs: bool,
    /// 最大递归深度(用于处理复杂泛型)
    pub max_depth: usize,
    /// 是否生成默认 guards
    pub generate_guards: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            include_private: false,
            include_tests: false,
            include_docs: false,
            max_depth: 10,
            generate_guards: true,
        }
    }
}

/// 使用自定义选项将 rustdoc.json 转换为 RepairPetriNet JSON
/// 
/// # 参数
/// 
/// * `rustdoc_json_str` - rustdoc.json 的 JSON 字符串内容
/// * `options` - 转换选项
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use trustfall_rustdoc_adapter::petri::{convert_rustdoc_to_petri_with_options, ConversionOptions};
/// 
/// let rustdoc_json = std::fs::read_to_string("target/doc/my_crate.json")?;
/// let options = ConversionOptions {
///     include_private: true,
///     generate_guards: true,
///     ..Default::default()
/// };
/// let petri_net_json = convert_rustdoc_to_petri_with_options(&rustdoc_json, options)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_rustdoc_to_petri_with_options(
    rustdoc_json_str: &str,
    options: ConversionOptions,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. 解析 rustdoc.json
    let crate_data: Crate = serde_json::from_str(rustdoc_json_str)?;
    
    // 2. 构建 Petri 网
    let petri_net = PetriNetBuilder::from_crate(&crate_data);
    
    // 3. 转换为 JSON 表示
    let mut json_net = JsonPetriNet::from(&petri_net);
    
    // 4. 填充元数据
    json_net.metadata.crate_name = crate_data.index.get(&crate_data.root)
        .map(|item| item.name.as_ref().map(|n| n.to_string()))
        .flatten();
    
    json_net.metadata.rustdoc_version = Some(format!("{}.{}.{}", 
        crate_data.format_version / 100,
        (crate_data.format_version / 10) % 10,
        crate_data.format_version % 10
    ));
    
    json_net.metadata.timestamp = Some(chrono::Utc::now().to_rfc3339());
    
    // 5. 根据选项生成 guards
    if options.generate_guards {
        generate_default_guards(&mut json_net);
    }
    
    // 6. 存储选项到 metadata
    json_net.metadata.source_file = Some(format!(
        "Converted with options: include_private={}, generate_guards={}",
        options.include_private,
        options.generate_guards
    ));
    
    // 7. 序列化为 JSON 字符串
    let json_string = json_net.to_json_string()?;
    
    Ok(json_string)
}

/// 生成默认的 guards (所有权和借用检查)
fn generate_default_guards(json_net: &mut JsonPetriNet) {
    use super::schema::{JsonGuard, JsonGuardCondition};
    use serde_json::json;
    
    // Guard 1: 检查输入 token 的所有权
    json_net.guards.push(JsonGuard {
        id: "g_ownership_check".to_string(),
        kind: Some("ownership".to_string()),
        description: Some("检查输入 token 的所有权状态".to_string()),
        conditions: vec![JsonGuardCondition {
            lhs: "input_token.ownership".to_string(),
            op: "in".to_string(),
            rhs: json!(["owned", "shared", "borrowed"]),
            negate: false,
        }],
        scope: Some("global".to_string()),
    });
    
    // Guard 2: 检查 &self 方法的借用
    json_net.guards.push(JsonGuard {
        id: "g_shared_borrow".to_string(),
        kind: Some("borrow".to_string()),
        description: Some("检查共享借用是否有效".to_string()),
        conditions: vec![JsonGuardCondition {
            lhs: "input_token.mode".to_string(),
            op: "==".to_string(),
            rhs: json!("&"),
            negate: false,
        }],
        scope: Some("method".to_string()),
    });
    
    // Guard 3: 检查 &mut self 方法的可变借用
    json_net.guards.push(JsonGuard {
        id: "g_mut_borrow".to_string(),
        kind: Some("borrow".to_string()),
        description: Some("检查可变借用是否有效".to_string()),
        conditions: vec![JsonGuardCondition {
            lhs: "input_token.mode".to_string(),
            op: "==".to_string(),
            rhs: json!("&mut"),
            negate: false,
        }],
        scope: Some("method".to_string()),
    });
    
    // 为所有方法类型的 transition 添加适当的 guard 引用
    for transition in &mut json_net.transitions {
        if transition.kind.as_deref() == Some("method") {
            // 检查第一个输入参数的模式
            if let Some(first_input) = transition.inputs.first() {
                match first_input.mode.as_deref() {
                    Some("&") => {
                        if !transition.guard_refs.contains(&"g_shared_borrow".to_string()) {
                            transition.guard_refs.push("g_shared_borrow".to_string());
                        }
                    }
                    Some("&mut") => {
                        if !transition.guard_refs.contains(&"g_mut_borrow".to_string()) {
                            transition.guard_refs.push("g_mut_borrow".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 批量转换多个 rustdoc.json 文件
/// 
/// # 参数
/// 
/// * `input_files` - 输入的 rustdoc.json 文件路径列表
/// * `output_dir` - 输出目录
/// 
/// # 示例
/// 
/// ```rust,no_run
/// use trustfall_rustdoc_adapter::petri::batch_convert;
/// 
/// let files = vec![
///     "target/doc/crate1.json",
///     "target/doc/crate2.json",
/// ];
/// batch_convert(&files, "output/")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn batch_convert<P: AsRef<Path>>(
    input_files: &[P],
    output_dir: P,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)?;
    
    let mut output_files = Vec::new();
    
    for input_file in input_files {
        let input_path = input_file.as_ref();
        let file_stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        
        let output_path = output_dir.join(format!("{}_petri_net.json", file_stem));
        
        convert_and_save(input_path, &output_path)?;
        
        output_files.push(
            output_path
                .to_str()
                .unwrap_or("unknown")
                .to_string()
        );
    }
    
    Ok(output_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_options_default() {
        let options = ConversionOptions::default();
        assert!(!options.include_private);
        assert!(!options.include_tests);
        assert!(options.generate_guards);
        assert_eq!(options.max_depth, 10);
    }

    #[test]
    fn test_generate_default_guards() {
        let mut json_net = JsonPetriNet {
            places: vec![],
            tokens: vec![],
            transitions: vec![],
            guards: vec![],
            metadata: Default::default(),
        };
        
        generate_default_guards(&mut json_net);
        
        assert_eq!(json_net.guards.len(), 3);
        assert!(json_net.guards.iter().any(|g| g.id == "g_ownership_check"));
        assert!(json_net.guards.iter().any(|g| g.id == "g_shared_borrow"));
        assert!(json_net.guards.iter().any(|g| g.id == "g_mut_borrow"));
    }
}

