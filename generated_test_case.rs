#[cfg(test)]
mod tests {
    use std::vec::Vec;

    #[test]
    fn test_api_sequence() {
        // 测试目的：验证向量基本操作序列（创建、添加元素、获取长度、获取元素）的正确性
        
        // 1. 创建一个新的空向量
        let mut vec: Vec<i32> = Vec::new();
        assert!(vec.is_empty(), "新创建的向量应该是空的");
        
        // 2. 向向量中添加一个元素
        vec.push(42);
        assert_eq!(vec.len(), 1, "添加元素后向量长度应为1");
        
        // 3. 验证向量长度
        let length = vec.len();
        assert_eq!(length, 1, "向量长度应该为1");
        
        // 4. 获取向量中的第一个元素
        match vec.get(0) {
            Some(&value) => {
                // 验证获取到的元素值是否正确
                assert_eq!(value, 42, "获取到的元素值应该为42");
            },
            None => {
                // 如果获取失败，测试应该失败
                panic!("应该能够获取索引0处的元素，但获取失败");
            }
        }
        
        // 额外验证：确保向量内容正确
        assert_eq!(vec, vec![42], "向量内容应该为[42]");
    }
}