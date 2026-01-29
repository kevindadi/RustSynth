//! 简单的计数器示例 - 用于测试 RustSynth

/// 计数器结构
pub struct Counter {
    value: i32,
}

impl Counter {
    /// 创建新计数器
    pub fn new() -> Self {
        Counter { value: 0 }
    }

    /// 递增
    pub fn increment(&mut self) {
        self.value += 1;
    }

    /// 获取当前值
    pub fn get(&self) -> i32 {
        self.value
    }

    /// 重置
    pub fn reset(&mut self) {
        self.value = 0;
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// 创建并返回一个初始值的计数器
pub fn create_counter_with_value(val: i32) -> Counter {
    Counter { value: val }
}

/// 打印计数器的值
pub fn print_counter(counter: &Counter) {
    println!("Counter value: {}", counter.value);
}

