//! Rustdoc JSON 加载器

use anyhow::{Context, Result};
use rustdoc_types::Crate;
use std::path::Path;

/// 从文件加载 rustdoc JSON
pub fn load_rustdoc_json(path: &Path) -> Result<Crate> {
    let contents = std::fs::read_to_string(path)
        .context(format!("读取文件失败: {:?}", path))?;

    let krate: Crate = serde_json::from_str(&contents)
        .context("解析 rustdoc JSON 失败")?;

    // 基本验证
    if krate.format_version < 28 {
        tracing::warn!(
            "rustdoc JSON format version {} 较旧，可能不兼容 (建议 >= 28)",
            krate.format_version
        );
    }

    Ok(krate)
}

