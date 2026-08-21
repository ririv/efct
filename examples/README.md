# Efct examples

[简体中文](README.zh-CN.md)

Each example is a small runnable program that demonstrates how to keep external effects at explicit boundaries while composing verified functions in the core.

## Available examples

- [Checkout policy](checkout/README.md): separates environment and clock access from deterministic invoice calculation and console output.
- [Diagnostic examples](diagnostics/README.md): pairs five rejected programs with corrected versions for file reads, nondeterminism, exceptions, third-party dependencies, and termination.

Every runnable example is checked by the integration test suite so that its commands, output, and Efct contracts remain current.
