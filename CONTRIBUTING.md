# 贡献指南

感谢你对 SyPetype 的兴趣！本文档提供了如何为项目做出贡献的指南。

## 开发环境设置

### 前置要求

- Rust stable (用于开发)
- Rust nightly (用于生成 rustdoc JSON)
- Git

### 克隆并构建

```bash
git clone <repository>
cd SyPetype
cargo build
cargo test
```

## 代码风格

### Rust 风格

遵循标准 Rust 风格指南：

```bash
# 格式化代码
cargo fmt

# 检查 lints
cargo clippy -- -D warnings
```

### 注释规范

- 所有公开 API 必须有文档注释
- 复杂算法需要内联注释解释
- 使用中文注释（与用户需求一致）

示例：

```rust
/// 归一化类型：将 rustdoc Type 转换为 TypeKey
///
/// # 参数
/// - `ty`: rustdoc 类型
///
/// # 返回
/// (TypeKey, Capability, is_copy)
pub fn normalize_type(&self, ty: &Type) -> Result<(TypeKey, Capability, bool)> {
    // 实现...
}
```

## 提交规范

### Commit Message 格式

```
<type>(<scope>): <subject>

<body>

<footer>
```

类型 (type):
- `feat`: 新功能
- `fix`: 修复 bug
- `docs`: 文档更新
- `style`: 代码格式（不影响功能）
- `refactor`: 重构
- `test`: 测试相关
- `chore`: 构建/工具相关

示例：

```
feat(transition): 添加 reborrow 变迁支持

实现了 &mut T -> &mut T 的重借用变迁，
允许在方法链中使用可变引用。

Closes #42
```

## 测试

### 运行测试

```bash
# 所有测试
cargo test

# 特定模块
cargo test canonicalize

# 带输出
cargo test -- --nocapture
```

### 添加测试

为新功能添加单元测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_feature() {
        // 测试代码
    }
}
```

集成测试放在 `tests/` 目录。

## Pull Request 流程

1. **Fork 仓库**

2. **创建功能分支**
   ```bash
   git checkout -b feature/my-new-feature
   ```

3. **实现功能**
   - 编写代码
   - 添加测试
   - 更新文档

4. **确保通过检查**
   ```bash
   cargo fmt
   cargo clippy
   cargo test
   cargo build --release
   ```

5. **提交更改**
   ```bash
   git add .
   git commit -m "feat: 添加新功能"
   ```

6. **推送到 Fork**
   ```bash
   git push origin feature/my-new-feature
   ```

7. **创建 Pull Request**
   - 描述清楚更改内容
   - 关联相关 issue
   - 等待 review

## 贡献方向

### 优先级高

1. **更精确的 trait 分析**
   - 实现完整的 Copy/Clone trait 检测
   - 文件：`src/type_norm.rs`

2. **Reborrow 支持**
   - 添加 `&mut T -> &mut T` 重借用
   - 文件：`src/transition.rs`

3. **返回引用生命周期分析**
   - 更准确的 origin 推断
   - 文件：`src/transition.rs`

4. **性能优化**
   - 并行搜索
   - 更好的状态缓存
   - 文件：`src/search.rs`

### 优先级中

1. **泛型单态化**
   - 展开泛型参数
   - 文件：`src/type_norm.rs`

2. **更多 Structural Transitions**
   - Deref、Clone 等
   - 文件：`src/transition.rs`

3. **交互式调试模式**
   - 逐步执行
   - 状态可视化

4. **更好的错误消息**
   - 详细的失败原因
   - 建议的修复方案

### 优先级低

1. **图形化界面**
2. **IDE 集成**
3. **Fuzzing 集成**

## 代码审查标准

Pull Request 会根据以下标准审查：

- ✅ 代码风格符合规范
- ✅ 所有测试通过
- ✅ 新功能有测试覆盖
- ✅ 文档已更新
- ✅ 没有引入新的 warnings
- ✅ Commit message 清晰
- ✅ 不破坏现有功能

## 报告 Bug

使用 GitHub Issues 报告 bug，请包含：

1. **环境信息**
   - Rust 版本
   - 操作系统
   - SyPetype 版本

2. **重现步骤**
   - 输入文件（或最小示例）
   - 运行命令
   - 预期行为
   - 实际行为

3. **错误日志**
   ```bash
   RUST_LOG=debug sypetype --input ... 2>&1 | tee error.log
   ```

## 提出功能请求

使用 GitHub Issues 提出功能请求，请包含：

1. **用例描述**
   - 你想做什么？
   - 为什么现有功能不够用？

2. **期望行为**
   - 期望的 API/命令
   - 期望的输出

3. **替代方案**
   - 是否考虑过其他方案？

## 文档贡献

文档同样重要！欢迎改进：

- README.md
- USAGE.md
- ARCHITECTURE.md
- 代码注释
- 示例

## 社区准则

- 尊重他人
- 建设性反馈
- 乐于助人
- 开放心态

## 许可证

贡献的代码将采用项目的双许可证（MIT / Apache-2.0）。

---

有问题？欢迎在 Issues 中提问！

