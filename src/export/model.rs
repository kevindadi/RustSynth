use std::collections::BTreeMap;

use serde::Serialize;

/// 顶层导出结构.
#[derive(Debug, Clone, Serialize)]
pub struct LlmSpec {
    pub crate_name: String,
    pub crate_version: String,
    pub items: Vec<FunctionSpec>,
}

/// 单个公开 API 函数/方法的规格.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionSpec {
    pub kind: String,
    pub path: String,
    pub visibility: String,
    pub signature: String,
    pub generics: SpecGenerics,
    pub inputs: Vec<FunctionInput>,
    pub output: FunctionOutput,
    pub docs: SpecDocs,
    pub invariants: SpecInvariants,
    pub error_cases: Vec<String>,
    pub may_panic: Vec<String>,
    pub traits_bound: Vec<String>,
    pub type_hints: TypeHints,
    pub llm_hints: LlmHints,
    pub source: SourceLocation,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SpecGenerics {
    #[serde(default)]
    pub params: Vec<String>,
    #[serde(default, rename = "where_clauses")]
    pub where_clauses: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionInput {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub by_ref: bool,
    #[serde(default, rename = "mut")]
    pub mutable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionOutput {
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecDocs {
    pub summary: String,
    pub details: String,
    pub sections: SpecDocSections,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SpecDocSections {
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub panics: Vec<String>,
    #[serde(default)]
    pub safety: Vec<String>,
    #[serde(default)]
    pub returns: Vec<String>,
    #[serde(default)]
    pub examples: Vec<DocExample>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DocExample {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SpecInvariants {
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub postconditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TypeHints {
    #[serde(default)]
    pub enums: BTreeMap<String, Vec<String>>,
    #[serde(default, rename = "value_ranges")]
    pub value_ranges: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LlmHints {
    #[serde(default)]
    pub equivalence_classes: Vec<String>,
    #[serde(default)]
    pub boundary_values: Vec<String>,
    #[serde(default)]
    pub mutation_hotspots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SourceLocation {
    pub file: String,
    #[serde(default)]
    pub line_span: [usize; 2],
}

