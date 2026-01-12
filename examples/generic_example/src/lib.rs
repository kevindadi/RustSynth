//! 泛型示例：用于测试 SyPetype 的泛型解析能力
//!
//! 包含：
//! - 泛型结构体
//! - 关联类型 (Associated Types)
//! - 基本 Trait 实现 (Default, Clone)
//! - 泛型方法

// ============================================
// 基础类型
// ============================================

/// 简单计数器
#[derive(Clone, Default)]
pub struct Counter {
    value: i32,
}

impl Counter {
    /// 创建新计数器
    pub fn new() -> Self {
        Self { value: 0 }
    }

    /// 创建指定值的计数器
    pub fn with_value(value: i32) -> Self {
        Self { value }
    }

    /// 获取值
    pub fn get(&self) -> i32 {
        self.value
    }

    /// 增加
    pub fn increment(&mut self) {
        self.value += 1;
    }

    /// 重置
    pub fn reset(&mut self) {
        self.value = 0;
    }
}

// ============================================
// 泛型容器
// ============================================

/// 泛型包装器
#[derive(Clone)]
pub struct Wrapper<T> {
    inner: T,
}

impl<T> Wrapper<T> {
    /// 创建新的包装器
    pub fn new(value: T) -> Self {
        Self { inner: value }
    }

    /// 获取内部引用
    pub fn get(&self) -> &T {
        &self.inner
    }

    /// 获取内部可变引用
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// 解包获取内部值
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// 映射转换
    pub fn map<U, F>(self, f: F) -> Wrapper<U>
    where
        F: FnOnce(T) -> U,
    {
        Wrapper {
            inner: f(self.inner),
        }
    }
}

impl<T: Default> Default for Wrapper<T> {
    fn default() -> Self {
        Self {
            inner: T::default(),
        }
    }
}

/// 泛型配对
#[derive(Clone)]
pub struct Pair<A, B> {
    pub first: A,
    pub second: B,
}

impl<A, B> Pair<A, B> {
    /// 创建配对
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    /// 获取第一个元素引用
    pub fn first(&self) -> &A {
        &self.first
    }

    /// 获取第二个元素引用
    pub fn second(&self) -> &B {
        &self.second
    }

    /// 交换顺序
    pub fn swap(self) -> Pair<B, A> {
        Pair {
            first: self.second,
            second: self.first,
        }
    }
}

impl<A: Default, B: Default> Default for Pair<A, B> {
    fn default() -> Self {
        Self {
            first: A::default(),
            second: B::default(),
        }
    }
}

// ============================================
// 关联类型 Trait
// ============================================

/// 容器 Trait（带关联类型）
pub trait Container {
    /// 元素类型
    type Item;

    /// 获取元素数量
    fn len(&self) -> usize;

    /// 是否为空
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 获取第一个元素
    fn first(&self) -> Option<&Self::Item>;

    /// 添加元素
    fn push(&mut self, item: Self::Item);

    /// 弹出元素
    fn pop(&mut self) -> Option<Self::Item>;
}

/// 简单栈实现
pub struct Stack<T> {
    items: [Option<T>; 8], // 固定大小，避免使用 Vec
    top: usize,
}

impl<T> Stack<T> {
    /// 创建空栈
    pub fn new() -> Self {
        Self {
            items: [None, None, None, None, None, None, None, None],
            top: 0,
        }
    }
}

impl<T: Clone> Default for Stack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Container for Stack<T> {
    type Item = T;

    fn len(&self) -> usize {
        self.top
    }

    fn first(&self) -> Option<&Self::Item> {
        if self.top > 0 {
            self.items[self.top - 1].as_ref()
        } else {
            None
        }
    }

    fn push(&mut self, item: Self::Item) {
        if self.top < 8 {
            self.items[self.top] = Some(item);
            self.top += 1;
        }
    }

    fn pop(&mut self) -> Option<Self::Item> {
        if self.top > 0 {
            self.top -= 1;
            self.items[self.top].take()
        } else {
            None
        }
    }
}

// ============================================
// 转换 Trait
// ============================================

/// 自定义 From trait
pub trait MyFrom<T> {
    fn my_from(value: T) -> Self;
}

/// 自定义 Into trait
pub trait MyInto<T> {
    fn my_into(self) -> T;
}

// 如果实现了 MyFrom，自动实现 MyInto
impl<T, U> MyInto<U> for T
where
    U: MyFrom<T>,
{
    fn my_into(self) -> U {
        U::my_from(self)
    }
}

// Counter 可以从 i32 创建
impl MyFrom<i32> for Counter {
    fn my_from(value: i32) -> Self {
        Counter::with_value(value)
    }
}

// Wrapper<Counter> 可以从 Counter 创建
impl MyFrom<Counter> for Wrapper<Counter> {
    fn my_from(value: Counter) -> Self {
        Wrapper::new(value)
    }
}

// ============================================
// 迭代器 Trait（简化版）
// ============================================

/// 简化的迭代器 trait
pub trait SimpleIter {
    type Item;

    fn next(&mut self) -> Option<Self::Item>;

    fn count(mut self) -> usize
    where
        Self: Sized,
    {
        let mut count = 0;
        while self.next().is_some() {
            count += 1;
        }
        count
    }
}

/// 范围迭代器
pub struct Range {
    current: i32,
    end: i32,
}

impl Range {
    /// 创建范围
    pub fn new(start: i32, end: i32) -> Self {
        Self {
            current: start,
            end,
        }
    }
}

impl SimpleIter for Range {
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.end {
            let value = self.current;
            self.current += 1;
            Some(value)
        } else {
            None
        }
    }
}

// ============================================
// 辅助函数
// ============================================

/// 包装值
pub fn wrap<T>(value: T) -> Wrapper<T> {
    Wrapper::new(value)
}

/// 创建配对
pub fn pair<A, B>(a: A, b: B) -> Pair<A, B> {
    Pair::new(a, b)
}

/// 使用容器
pub fn use_container<C: Container>(container: &mut C, item: C::Item) -> usize {
    container.push(item);
    container.len()
}

/// 转换并包装
pub fn convert_and_wrap<T, U>(value: T) -> Wrapper<U>
where
    U: MyFrom<T>,
{
    Wrapper::new(U::my_from(value))
}
