//! 导出适用于 LLM 消费的 crate 规格.
//!
//! 该模块基于 `IndexedCrate` 收集公开 API 函数/方法的语义信息,
//! 经过文档清洗与启发式分析后生成结构化 JSON.

use std::io::Write;

use anyhow::Context;
use serde_json::Value;

use crate::IndexedCrate;

mod clean;
mod collect;
mod hints;
mod model;

use clean::{clean_documentation, CleanedDocs};
use collect::{collect_items, CollectOptions, CollectedItem};
use hints::{infer_hints, HintsInput};
use model::{FunctionSpec, LlmSpec, SpecDocs};

/// 导出行为配置.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// 是否仅导出公共（可导入） API.默认 `true`.
    pub public_only: bool,
    /// 限制文档分析时使用的最大原始 doc 字节数,超出则截断.
    pub max_doc_bytes: Option<usize>,
    /// 跳过 panic/错误分析的快速路径.
    pub skip_panic_pass: bool,
    /// 手动覆盖 crate 名称.
    pub crate_name_override: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            public_only: true,
            max_doc_bytes: Some(32 * 1024),
            skip_panic_pass: false,
            crate_name_override: None,
        }
    }
}

/// 构建完整 LLM 规格 JSON 值.
pub fn export_llm_spec(indexed: &IndexedCrate<'_>) -> anyhow::Result<Value> {
    export_llm_spec_with_options(indexed, &ExportOptions::default())
}

/// 将 LLM 规格写入给定 writer.
pub fn export_llm_spec_to_writer(
    indexed: &IndexedCrate<'_>,
    mut writer: impl Write,
) -> anyhow::Result<()> {
    let spec = build_llm_spec(indexed, &ExportOptions::default())
        .context("failed to build LLM spec from indexed crate")?;
    serde_json::to_writer(&mut writer, &spec).context("failed to serialize LLM spec")?;
    Ok(())
}

pub(crate) fn export_llm_spec_with_options(
    indexed: &IndexedCrate<'_>,
    options: &ExportOptions,
) -> anyhow::Result<Value> {
    let spec = build_llm_spec(indexed, options)?;
    Ok(serde_json::to_value(spec)?)
}

pub fn build_llm_spec(indexed: &IndexedCrate<'_>, options: &ExportOptions) -> anyhow::Result<LlmSpec> {
    let derived_name = indexed
        .inner
        .paths
        .get(&indexed.inner.root)
        .and_then(|summary| summary.path.last())
        .cloned();
    let crate_name = options
        .crate_name_override
        .clone()
        .or(derived_name)
        .unwrap_or_else(|| "unknown".to_string());
    let crate_version = indexed
        .inner
        .crate_version
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    let collected = collect_items(
        indexed,
        CollectOptions {
            public_only: options.public_only,
        },
    )?;

    let mut items = Vec::with_capacity(collected.len());
    for item in collected {
        items.push(enrich_item(indexed, &item, options)?);
    }

    Ok(LlmSpec {
        crate_name,
        crate_version,
        items,
    })
}

fn enrich_item(
    indexed: &IndexedCrate<'_>,
    item: &CollectedItem,
    options: &ExportOptions,
) -> anyhow::Result<FunctionSpec> {
    let raw_docs = item
        .docs
        .as_deref()
        .map(|doc| {
            if let Some(limit) = options.max_doc_bytes {
                doc.get(..limit).unwrap_or(doc)
            } else {
                doc
            }
        })
        .unwrap_or_default();

    let cleaned: CleanedDocs = clean_documentation(raw_docs);
    let hints = infer_hints(
        indexed,
        &HintsInput {
            collected: item,
            cleaned: &cleaned,
            skip_panics: options.skip_panic_pass,
        },
    );

    Ok(FunctionSpec {
        kind: item.kind.clone(),
        path: item.path.clone(),
        visibility: item.visibility.clone(),
        signature: item.signature.clone(),
        generics: item.generics.clone(),
        inputs: item.inputs.clone(),
        output: item.output.clone(),
        docs: SpecDocs {
            summary: cleaned.summary,
            details: cleaned.details,
            sections: cleaned.sections,
        },
        invariants: hints.invariants,
        error_cases: hints.error_cases,
        may_panic: hints.may_panic,
        traits_bound: item.traits_bound.clone(),
        type_hints: hints.type_hints,
        llm_hints: hints.llm_hints,
        source: item.source.clone(),
    })
}

