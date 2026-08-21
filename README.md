# Efct

[简体中文](https://github.com/ririv/efct/blob/main/README.zh-CN.md)

Efct is a static effect and pure-function verifier for closed, auditable subsets of Python, TypeScript, and JavaScript. It checks what a function can observe, change, throw, or fail to return from, and rejects code it cannot prove safe.

Efct is currently alpha software. Language frontends share the same effect model and fail-closed policy, while each language has its own deliberately limited certified subset.

## Language support

| Language | Runtime and module system | Package |
| --- | --- | --- |
| Python | CPython 3.13 and 3.14 | [`efct`](https://pypi.org/project/efct/) |
| TypeScript | Node.js 24.19.0, TypeScript 5.9.3, ESM | [`@efct/efct`](https://www.npmjs.com/package/@efct/efct) |
| JavaScript | Node.js 24.19.0, checked `.js`/`.mjs`, ESM | [`@efct/efct`](https://www.npmjs.com/package/@efct/efct) |

TypeScript and JavaScript use the same verifier. Their current certified subset and runtime requirements are documented in [Efct for TypeScript and JavaScript](https://github.com/ririv/efct/blob/main/js/efct/README.md).

## Installation

Install Efct from PyPI:

```console
python -m pip install efct
```

With uv:

```console
uv add efct
```

Confirm the installation:

```console
efct --version
```

## Quick start

Import the small public API directly and decorate a function with `@pure`:

```python
from efct import pure


@pure
def add(left: int, right: int) -> int:
    return left + right


assert add(2, 3) == 5
```

When the module is imported, Efct validates its source, types, calls, and effects. A successful decoration returns a verified `PureFunction`. Validation failure raises `EfctStartupError` and stops the module from importing.

You can also check files without importing them:

```console
efct check app.py
efct check src/
```

Efct rejects unknown syntax, unknown calls, mutable values crossing function boundaries, and behavior outside a declared effect bound. It never treats unverified code as pure.

## TypeScript and JavaScript

Install the Node package:

```console
npm install @efct/efct
```

Define an explicitly pure exported function:

```ts
import { defineModule, pure } from "@efct/efct";

export const { add } = defineModule(import.meta.url, {
  add: pure()(function add(left: number, right: number): number {
    return left + right;
  }),
});
```

Then check or run it through Efct:

```console
npx efct check src/math.ts
npx efct run src/math.ts --call add --args '[20, 22]'
```

The same frontend checks ESM JavaScript in `.js` and `.mjs` files through TypeScript's checked-JavaScript model. See the [TypeScript and JavaScript guide](https://github.com/ririv/efct/blob/main/js/efct/README.md) for effect declarations, partial behavior, supported syntax, and the exact 0.1 boundary.

## Examples

The [checkout policy example](https://github.com/ririv/efct/tree/main/examples/checkout) is a runnable small program that separates environment and clock access from deterministic invoice calculation. The [diagnostic examples](https://github.com/ririv/efct/tree/main/examples/diagnostics) pair five rejected programs with corrected versions for hidden file access, nondeterminism, exceptions, uncertified dependencies, and nontermination.

## Pure functions and partial behavior

Efct separates external effects from partial behavior:

- External effects interact with or depend on the outside world, such as files, networks, clocks, randomness, environment variables, processes, and global state.
- Partial behavior means a computation may not return normally, such as raising an exception or diverging.

The three forms of `pure` have deliberately different meanings:

| Declaration | External effects | Partial behavior |
| --- | --- | --- |
| `@efct.pure` | Forbidden | Inferred and propagated |
| `@efct.pure()` | Forbidden | Explicitly empty |
| `@efct.pure(...)` | Forbidden | Explicit allowlist |

Use the bare decorator while implementing a function:

```python
@efct.pure
def parse(value: str) -> int:
    if value == "":
        raise ValueError("empty")
    return 1
```

Efct infers `Raise(ValueError)` and propagates it to callers.

Use an explicit allowlist at a stable API boundary:

```python
@efct.pure(efct.partial.Raise(ValueError))
def parse(value: str) -> int:
    if value == "":
        raise ValueError("empty")
    return 1
```

Use empty parentheses when the function must have no modeled partial behavior and must be proven to terminate:

```python
@efct.pure()
def increment(value: int) -> int:
    return value + 1
```

Efct models these partial behaviors:

- `efct.partial.Raise(ExceptionType)`
- `efct.partial.RaiseGroup(ExceptionType)`
- `efct.partial.Diverge()`

An explicit declaration is an upper bound. A function may produce fewer behaviors than declared, but it cannot produce an undeclared one.

## Declaring effects

Use `@efct.effects(...)` when a function intentionally interacts with the outside world:

```python
import efct


@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def show(value: int) -> None:
    print(value)
```

`print` has a console effect and may fail with the modeled `OSError` or `ValueError` paths. All three behaviors are therefore part of the public bound.

Available external effect declarations are:

- `efct.effect.Console()`
- `efct.effect.File.Read()` and `efct.effect.File.Write()`
- `efct.effect.Network()`
- `efct.effect.Clock()`
- `efct.effect.Random()`
- `efct.effect.Environment()`
- `efct.effect.Process()`
- `efct.effect.State.Read()` and `efct.effect.State.Write()`
- `efct.effect.Unsafe()`

Stable string declarations remain available:

```python
@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
```

Do not mix typed declarations and string declarations in the same decorator.

## Handling exceptions

Handled exceptions are removed from the outward partial bound:

```python
@efct.pure()
def item_or_zero(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except IndexError:
        return 0
```

Efct tracks exception inheritance, ordered handlers, `except ... as error`, exact re-raising, `try ... else`, `finally`, `contextlib.suppress`, exception groups, and supported `except*` flows. Unmatched exceptions continue to propagate.

For expected application failures, prefer `Result` data:

```python
@efct.pure
def parse(value: str) -> efct.Result[int, str]:
    if value == "":
        return efct.Err("empty")
    return efct.Ok(1)
```

`Result` supports exhaustive `match` over `Ok` and `Err`.

## Pure records

The same decorator can verify a deeply immutable record:

```python
from dataclasses import dataclass

import efct


@efct.pure
@dataclass(frozen=True, slots=True)
class Point:
    x: int
    y: int
```

Values crossing a verified function boundary must belong to Efct's supported immutable value model.

## Higher-order functions

`PureCallable` accepts a verified function with an explicitly empty effect and partial bound:

```python
@efct.pure()
def apply(
    function: efct.PureCallable[[int], int],
    value: int,
) -> int:
    return function(value)
```

Effect-polymorphic functions preserve the concrete effects of a callback:

```python
@efct.effects
def apply_effect[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)
```

The effect variable `E` is instantiated from the callback certificate at each call site.

## Module initialization

Function decorators do not automatically certify arbitrary top-level module code. Declare a module initialization contract with the reserved `_efct` name:

```python
import efct

_efct = efct.pure
```

For effectful initialization, declare an explicit upper bound:

```python
_efct = efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
```

Without `_efct`, the module may still run, but Efct makes no claim about its initialization effects.

## Command line

```console
efct check app.py
efct check src/
efct check src/ --format=json
efct check src/ --verified-only
efct check src/ --deny-unsafe
efct trust fingerprint pytest
efct run app.py
efct --version
```

`efct check` analyzes code without executing it. A zero exit status means the source snapshot described by the report passed verification.

`efct run` verifies the entry point and its local dependency closure before executing a verified source snapshot. Change the source, dependencies, or Python runtime and it must be checked again.

## External libraries

Third-party symbols are never considered pure automatically. Declare an unaudited boundary explicitly when migration is still in progress:

```toml
schema = 1

[[symbol]]
trust = "unsafe"
path = "legacy.math.clamp"
signature = "(int, int, int) -> int"
effects = []
partials = []
reason = "the implementation has not been audited"
```

An `unsafe` call contributes the `unsafe` effect. Use `--deny-unsafe` in CI to reject it.

An audited boundary binds the public contract to exact installed distributions and to the callable's real implementation:

```toml
schema = 1

[python]
implementation = "cpython"
version = "3.14.7"
cache_tag = "cpython-314"

[[distribution]]
name = "ourlib"
version = "1.4.2"
installation_sha256 = "..."
dependencies = []

[[symbol]]
trust = "audited"
path = "ourlib.math.clamp"
owner = "ourlib"
implementation = { kind = "python", path = "ourlib.math.clamp" }
signature = "(int, int, int) -> int"
effects = []
partials = []
```

Generate the installation fields instead of calculating the digest manually:

```console
efct trust fingerprint ourlib
efct trust fingerprint ourlib --format=json
```

List every audited direct dependency in `dependencies` and add a corresponding `[[distribution]]` block. Native extension functions use `kind = "native"`. Efct rejects stale digests, missing installation records, editable installations, undeclared dependency references, mismatched module ownership, replaced runtime callables, and Python code objects that do not match the hashed installation source. It never falls back from `audited` to `unsafe`.

`path` is the public export used by application code. `implementation.path` identifies the function that actually implements it, so audited re-exports can point into a declared dependency. `effects` contains only external effects; `partials` contains `raise:...`, `raise-group:...`, or `diverge`.

## Runtime enforcement

Verified wrappers enforce the certificate when called:

- `EfctContractError` reports an argument contract violation.
- `EfctIntegrityError` reports changed code, bindings, dependencies, or an invalid return value.
- `EfctStartupError` reports source or verification failure during decoration.

Argument and return types are exact. Rebinding a verified dependency invalidates the certificate instead of silently widening it. Audited distribution contents are checked when the certificate is issued; subsequent calls revalidate the loaded object identities and Python code object identity.

## Supported Python boundary

Efct intentionally supports a strict subset of Python. Common supported features include immutable primitive values, tuples, `frozenset`, `FrozenMap`, optional values, `Result`, local control flow, finite iteration, analyzed `while` loops, restricted local lists, static imports, verified cross-module calls, and registered standard-library operations.

Dynamic imports, reflection, monkey patching, ordinary mutable objects crossing boundaries, unknown third-party calls, and unsupported context managers are rejected. Efct is a verifier, not a Python sandbox.

## License

Efct is available under the [MIT License](LICENSE).
