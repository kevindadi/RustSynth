//! 高级示例：展示泛型和生命周期建模

/// 泛型容器
pub struct Container<T> {
    value: T,
}

impl<T> Container<T> {
    /// 创建新容器
    pub fn new(value: T) -> Self {
        Container { value }
    }

    /// 获取值的引用
    pub fn get(&self) -> &T {
        &self.value
    }

    /// 获取值的可变引用
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// 消费容器，返回内部值
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Clone> Container<T> {
    /// 克隆内部值（仅当 T 实现 Clone）
    pub fn clone_inner(&self) -> T {
        self.value.clone()
    }
}

/// 泛型对
pub struct Pair<A, B> {
    pub first: A,
    pub second: B,
}

impl<A, B> Pair<A, B> {
    /// 创建新对
    pub fn new(first: A, second: B) -> Self {
        Pair { first, second }
    }

    /// 交换元素
    pub fn swap(self) -> Pair<B, A> {
        Pair {
            first: self.second,
            second: self.first,
        }
    }
}

// ==================== 生命周期示例 ====================

/// 返回两个字符串切片中较长的一个
/// 
/// 展示生命周期参数：返回值的生命周期与输入参数绑定
pub fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}

/// 返回第一个字符串切片（忽略第二个）
/// 
/// 展示不同的生命周期参数：返回值只与第一个参数绑定
pub fn first_str<'a, 'b>(x: &'a str, _y: &'b str) -> &'a str {
    x
}

/// 在容器中查找最大值
pub fn find_max<T: Ord>(container: &Container<T>) -> &T {
    container.get()
}


/// 创建字符串容器
pub fn make_string_container(s: String) -> Container<String> {
    Container::new(s)
}

/// 创建整数对
pub fn make_int_pair(a: i32, b: i32) -> Pair<i32, i32> {
    Pair::new(a, b)
}
