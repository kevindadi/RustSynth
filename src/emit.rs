//! Rust 代码生成：从 trace 生成可编译的代码片段

use anyhow::{Context, Result};
use std::collections::HashMap;

use crate::model::VarId;
use crate::transition::{AdaptationStrategy, StructuralTransition, Transition};
use crate::type_norm::TypeContext;

/// 生成 Rust 代码片段
pub fn emit_code(
    trace: &[Transition],
    _type_ctx: &TypeContext,
    verbose: bool,
) -> Result<String> {
    let mut output = String::new();

    // 函数头
    output.push_str("fn generated_witness() {\n");

    // 变量名映射: VarId -> 代码中的名称
    let mut var_names: HashMap<VarId, String> = HashMap::new();
    let mut next_x_id = 0;
    let mut next_r_id = 0;

    // 生成变量名
    let alloc_var_name = |id: VarId,
                          is_ref: bool,
                          names: &mut HashMap<VarId, String>,
                          next_x: &mut usize,
                          next_r: &mut usize| {
        let name = if is_ref {
            let name = format!("r{}", *next_r);
            *next_r += 1;
            name
        } else {
            let name = format!("x{}", *next_x);
            *next_x += 1;
            name
        };
        names.insert(id, name.clone());
        name
    };

    // 遍历 trace
    for (step_idx, trans) in trace.iter().enumerate() {
        if verbose {
            output.push_str(&format!("    // Step {}: {}\n", step_idx, trans.description()));
        }

        match trans {
            Transition::ApiCall(call) => {
                // 构建参数列表
                let args: Vec<String> = call
                    .arg_bindings
                    .iter()
                    .map(|binding| {
                        let var_name = var_names
                            .get(&binding.token_id)
                            .cloned()
                            .unwrap_or_else(|| format!("v{}", binding.token_id));

                        match binding.adaptation {
                            AdaptationStrategy::Direct => var_name,
                            AdaptationStrategy::OwnedToShared => format!("&{}", var_name),
                            AdaptationStrategy::OwnedToMut => format!("&mut {}", var_name),
                            AdaptationStrategy::MutToShared => format!("&*{}", var_name),
                        }
                    })
                    .collect();

                // 生成调用
                let call_expr = format!("{}({})", call.api.full_path, args.join(", "));

                // 如果有返回值，绑定到变量
                if let Some(return_id) = call.return_var {
                    let is_ref = matches!(
                        call.api.return_mode,
                        crate::api_extract::ReturnMode::SharedRef(_)
                            | crate::api_extract::ReturnMode::MutRef(_)
                    );
                    let var_name = alloc_var_name(
                        return_id,
                        is_ref,
                        &mut var_names,
                        &mut next_x_id,
                        &mut next_r_id,
                    );
                    output.push_str(&format!("    let {} = {};\n", var_name, call_expr));
                } else {
                    output.push_str(&format!("    {};\n", call_expr));
                }
            }
            Transition::Structural(structural) => match structural {
                StructuralTransition::DropOwned { token_id } => {
                    if let Some(var_name) = var_names.get(token_id) {
                        output.push_str(&format!("    drop({});\n", var_name));
                    }
                }
                StructuralTransition::BorrowShr { owner_id, ref_id } => {
                    let owner_name = var_names
                        .get(owner_id)
                        .cloned()
                        .unwrap_or_else(|| format!("v{}", owner_id));
                    let ref_name = alloc_var_name(
                        *ref_id,
                        true,
                        &mut var_names,
                        &mut next_x_id,
                        &mut next_r_id,
                    );
                    output.push_str(&format!("    let {} = &{};\n", ref_name, owner_name));
                }
                StructuralTransition::BorrowMut { owner_id, ref_id } => {
                    let owner_name = var_names
                        .get(owner_id)
                        .cloned()
                        .unwrap_or_else(|| format!("v{}", owner_id));
                    let ref_name = alloc_var_name(
                        *ref_id,
                        true,
                        &mut var_names,
                        &mut next_x_id,
                        &mut next_r_id,
                    );
                    output.push_str(&format!("    let {} = &mut {};\n", ref_name, owner_name));
                }
                StructuralTransition::EndBorrow { ref_id, .. } => {
                    // 可选：显式 drop(r) 或依赖 scope
                    if let Some(ref_name) = var_names.get(ref_id) {
                        output.push_str(&format!("    drop({});\n", ref_name));
                    }
                }
            },
        }
    }

    output.push_str("}\n");

    Ok(output)
}

/// 验证生成的代码 (在临时 crate 中运行 cargo check)
pub fn verify_code(snippet: &str, crate_name: &str) -> Result<()> {
    use std::fs;
    use std::process::Command;

    let temp_dir = tempfile::tempdir()?;
    let temp_path = temp_dir.path();

    // 创建临时 Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "verify_snippet"
version = "0.1.0"
edition = "2021"

[dependencies]
{} = {{ path = "." }}
"#,
        crate_name
    );
    fs::write(temp_path.join("Cargo.toml"), cargo_toml)?;

    // 创建 src/main.rs
    fs::create_dir(temp_path.join("src"))?;
    fs::write(temp_path.join("src/main.rs"), snippet)?;

    // 运行 cargo check
    tracing::info!("运行 cargo check 在临时目录: {:?}", temp_path);
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(temp_path)
        .output()
        .context("运行 cargo check 失败")?;

    if output.status.success() {
        tracing::info!("✓ cargo check 通过");
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("✗ cargo check 失败:\n{}", stderr);
        anyhow::bail!("生成的代码无法通过 cargo check")
    }
}

