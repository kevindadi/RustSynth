//! Type Flow Example - 展示类型之间的转换和数据流
//!
//! 数据流水线: Source → RawData → ParsedData → ProcessedData → Sink
//!
//! 设计原则:
//! - 避免 'static 生命周期
//! - 使用基本类型 (i32, usize) 作为参数
//! - 展示类型之间的转换关系

// ==================== 数据源 ====================

/// 数据源 - 能够产生原始数据
pub struct Source {
    id: i32,
}

impl Source {
    /// 创建新的数据源
    pub fn new(id: i32) -> Self {
        Source { id }
    }

    /// 从源读取原始数据
    pub fn read(self) -> RawData {
        RawData { value: self.id }
    }
}

// ==================== 原始数据 ====================

/// 原始数据 - 未解析的数据
pub struct RawData {
    value: i32,
}

impl RawData {
    /// 解析原始数据
    pub fn parse(self) -> ParsedData {
        ParsedData {
            value: self.value,
            valid: true,
        }
    }

    /// 获取原始值
    pub fn get(&self) -> i32 {
        self.value
    }
}

// ==================== 解析后的数据 ====================

/// 解析后的数据
pub struct ParsedData {
    value: i32,
    valid: bool,
}

impl ParsedData {
    /// 处理解析后的数据
    pub fn process(self) -> ProcessedData {
        ProcessedData {
            result: self.value * 2,
        }
    }

    /// 获取值
    pub fn value(&self) -> i32 {
        self.value
    }

    /// 是否有效
    pub fn is_valid(&self) -> bool {
        self.valid
    }
}

// ==================== 处理后的数据 ====================

/// 处理后的数据 - 可以被输出
pub struct ProcessedData {
    result: i32,
}

impl ProcessedData {
    /// 获取结果
    pub fn result(&self) -> i32 {
        self.result
    }

    /// 输出到 Sink
    pub fn write_to(self, sink: &mut Sink) {
        sink.received += 1;
    }

    /// 合并两个处理结果
    pub fn merge(self, other: ProcessedData) -> MergedData {
        MergedData {
            total: self.result + other.result,
        }
    }
}

/// 合并后的数据
pub struct MergedData {
    total: i32,
}

impl MergedData {
    /// 获取合并结果
    pub fn total(&self) -> i32 {
        self.total
    }
}

// ==================== 数据接收器 ====================

/// 数据接收器 - 消费处理后的数据
pub struct Sink {
    received: usize,
}

impl Sink {
    /// 创建新的接收器
    pub fn new() -> Self {
        Sink { received: 0 }
    }

    /// 获取已接收的数据量
    pub fn count(&self) -> usize {
        self.received
    }
}

impl Default for Sink {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 流水线函数 ====================

/// 完整处理流程: i32 → Source → RawData → ParsedData → ProcessedData
pub fn full_pipeline(id: i32) -> ProcessedData {
    Source::new(id).read().parse().process()
}

/// 合并两个源的数据
pub fn merge_sources(id1: i32, id2: i32) -> MergedData {
    let p1 = Source::new(id1).read().parse().process();
    let p2 = Source::new(id2).read().parse().process();
    p1.merge(p2)
}
