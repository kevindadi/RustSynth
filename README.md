# trustfall-rustdoc
Trustfall adapter for querying rustdoc JSON data.

- [Contributing](https://github.com/obi1kenobi/trustfall-rustdoc-adapter/blob/main/CONTRIBUTING.md)

## CLI 工具

### `petri_synth`
- 构建 rustdoc JSON 的 Petri 网表示,用于做基于类型的程序合成.
- 支持将构建的网导出为 JSON 或 Graphviz DOT,便于调试或可视化.
- 接受初始类型 (`--input`) 与目标类型 (`--goal`),利用 Petri 网搜索可行的调用序列.

示例:
```
cargo run --bin petri_synth -- \
  --rustdoc target/doc/my_crate.json \
  --input "&str" \
  --goal "String" \
  --emit-net petri.json
```

常用参数:
- `--rustdoc <path>`:必填,rustdoc JSON 输入文件.
- `--input <type>`:初始可用类型,可重复指定.
- `--goal <type>`:目标类型,可重复指定.
- `--emit-net <path>`:导出 Petri 网(JSON 或 `.dot`).
- `--max-depth` / `--max-states`:控制搜索深度与状态数上限.

### `rustdoc_llm_export`
- 将 rustdoc JSON 转换为适合大模型消费的 API 规格(结构化 JSON).
- 支持仅导出公开 API,或包含私有条目,并可限制文档长度.

示例:
```
cargo run --bin rustdoc_llm_export -- \
  --rustdoc-json target/doc/my_crate.json \
  --output llm_spec.json \
  --pretty
```

常用参数:
- `--rustdoc-json <path>`:必填,rustdoc JSON 输入文件.
- `--output`/`-o <path>`:输出文件,默认写到 stdout.
- `--crate-name <name>`:覆盖规格中的 crate 名称.
- `--public-only` / `--include-private`:控制导出范围.
- `--max-doc-bytes <n>`:限制 docstring 长度,支持 `none`/`0` 表示不限.
- `--no-panics-pass`:跳过 panic / error 分析.

#### License

<sup>
Available under the <a href="LICENSE-APACHE">Apache License, Version
2.0</a> or <a href="LICENSE-MIT">MIT license</a>, at your option.
</sup>

<br>

<sup>
Copyright 2022-present Predrag Gruevski and Contributors.
</sup>

<br>

<sub>
Contributors are defined in the Apache-2.0 license.
The present date is determined by the timestamp of the most recent commit in the repository.
</sub>

<br>

<sub>
Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
</sub>
