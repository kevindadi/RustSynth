/// Petri Net 通用 Trait 定义
///
/// 提供统一的接口用于：
/// 1. 从 IrGraph 转换到 Petri Net
/// 2. 导出 Petri Net（DOT 和 JSON）
use crate::ir_graph::structure::IrGraph;
use std::io;
use std::path::Path;

/// 从 IrGraph 构建 Petri Net 的 Trait
pub trait FromIrGraph: Sized {
    /// 从 IR Graph 构建 Petri Net
    fn from_ir_graph(ir: &IrGraph) -> Self;
}

/// Petri Net 导出功能的 Trait
pub trait PetriNetExport {
    /// 导出为 DOT 格式字符串
    fn to_dot(&self) -> String;

    /// 导出为 JSON 格式字符串
    fn to_json(&self) -> Result<String, serde_json::Error>;

    /// 导出 DOT 文件
    fn export_dot<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let dot_content = self.to_dot();
        std::fs::write(path, dot_content)
    }

    /// 导出 JSON 文件
    fn export_json<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let json_content = self
            .to_json()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(path, json_content)
    }

    /// 打印统计信息
    fn print_stats(&self);

    /// 获取统计信息字符串（可选实现）
    fn get_stats_string(&self) -> String {
        // 默认实现：返回空字符串，由具体类型覆盖
        String::new()
    }
}

/// Petri Net 的通用标识 Trait
///
/// 用于在编译时区分不同类型的 Petri Net
pub trait PetriNetKind {
    /// Petri Net 的类型名称
    fn kind_name() -> &'static str;

    /// Petri Net 的简短描述
    fn description() -> &'static str;
}
