# efct

`efct` provides Rust types for describing external effects with Efct. Rust code
uses enum variants directly; stable string names are reserved for configuration
and interchange boundaries.

Recoverable Rust failures use `Result<T, E>` and are not represented as effects.
Python exceptions and ECMAScript throws remain in their language-specific analyzer
models.

```rust
use efct::Effect;

fn declared_effects() -> [Effect; 2] {
    [Effect::FileRead, Effect::Console]
}

fn parse_configuration(value: &str) -> Result<Effect, efct::ParseEffectError> {
    value.parse()
}
```

This crate currently models effects. It does not analyze Rust source code.

Rust-native semantics remain native: recoverable failures use `Result<T, E>`,
optional values use `Option<T>`, and this crate does not emulate Python `raise` or
ECMAScript `throw`.

For the complete Efct project, see <https://github.com/ririv/efct>.

[简体中文](README.zh-CN.md)
