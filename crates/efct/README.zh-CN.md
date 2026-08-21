# efct

`efct` 提供用于描述 Efct 外部效果的 Rust 类型。Rust 代码直接使用枚举变体；
稳定字符串名称只用于配置和交换边界。

Rust 中可恢复的失败使用 `Result<T, E>`，不建模为效果。Python 异常和
ECMAScript 抛出行为继续由各自语言的分析器模型表示。

```rust
use efct::Effect;

fn declared_effects() -> [Effect; 2] {
    [Effect::FileRead, Effect::Console]
}

fn parse_configuration(value: &str) -> Result<Effect, efct::ParseEffectError> {
    value.parse()
}
```

当前 crate 只提供效果模型，不分析 Rust 源代码。

Rust 原生语义继续使用原生表达：可恢复失败使用 `Result<T, E>`，可选值使用
`Option<T>`；本 crate 不模拟 Python `raise` 或 ECMAScript `throw`。

完整 Efct 项目请访问 <https://github.com/ririv/efct>。

[English](README.md)
