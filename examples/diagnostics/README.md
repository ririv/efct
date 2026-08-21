# Diagnostic examples

[简体中文](README.zh-CN.md)

This collection contains programs that Efct must reject. It complements the passing [checkout example](../checkout/README.md) by showing what happens when behavior is hidden behind a pure contract.

Run one rejected example from the repository root:

```console
efct check examples/diagnostics/rejected/hidden_file_read.py
```

The command exits with status `1`. File locations are omitted from the excerpts below so the behavior is easier to compare. The checked-in [integration test](../../tests/integration/examples/test_diagnostics.py) locks the exact diagnostic messages, and every program under [`fixed/`](fixed/) passes together:

```console
efct check examples/diagnostics/fixed
```

## Hidden file read

[`hidden_file_read.py`](rejected/hidden_file_read.py) declares an empty pure contract and then opens a path:

```python
from efct import pure


@pure()
def probe_file(path: str) -> None:
    open(path)
```

Efct reports the external dependency and its observable failure modes separately:

```text
P1001 Function probe_file contains undeclared effect file.read
P1001 Function probe_file contains undeclared partial behavior raise:builtins.OSError
P1001 Function probe_file contains undeclared partial behavior raise:builtins.ValueError
```

[`file_boundary.py`](fixed/file_boundary.py) fixes the contract by declaring the file capability and both exceptions:

```python
@effects(
    effect.File.Read(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def probe_file(path: str) -> None:
    open(path)
```

Allowing `File.Read()` does not implicitly allow exceptions. Effects and partial behavior are independent, explicit bounds.

## Hidden clock and randomness

[`hidden_nondeterminism.py`](rejected/hidden_nondeterminism.py) obtains two values that can change without a change to its arguments:

```python
@pure()
def session_marker(low: int, high: int) -> tuple[int, int]:
    return (time.time_ns(), random.randint(low, high))
```

Efct identifies both sources and the invalid range failure from `random.randint`:

```text
P1001 Function session_marker contains undeclared effect clock
P1001 Function session_marker contains undeclared effect random
P1001 Function session_marker contains undeclared partial behavior raise:builtins.ValueError
```

[`nondeterminism_boundary.py`](fixed/nondeterminism_boundary.py) makes that boundary explicit with `Clock()`, `Random()`, and `Raise(ValueError)`. A deterministic core can instead receive the timestamp and sampled value as ordinary immutable arguments, as the checkout example does with its clock value.

## Undeclared exception

[`undeclared_exception.py`](rejected/undeclared_exception.py) promises an empty partial bound but has a reachable `raise`:

```python
@pure()
def require_non_negative(value: int) -> int:
    if value < 0:
        raise ValueError("value must be non-negative")
    return value
```

The failure is part of the function contract even though it does not create an external effect:

```text
P1001 Function require_non_negative contains undeclared partial behavior raise:builtins.ValueError
```

[`exception_contract.py`](fixed/exception_contract.py) uses the explicit whitelist:

```python
@pure(partial.Raise(ValueError))
def require_non_negative(value: int) -> int:
    ...
```

The function remains externally pure, while callers can see that it is partial for negative inputs.

## Uncertified third-party dependency

[`uncertified_dependency.py`](rejected/uncertified_dependency.py) imports and calls `requests` from checked code:

```python
import requests
from efct import pure


@pure()
def download(url: str) -> None:
    requests.get(url)
```

Efct does not guess the behavior of an unknown package:

```text
P1301 Imported module requests is not certified by the MVP
P1004 Value name requests cannot be resolved
```

There are two explicit designs:

- Keep the integration outside the checked pure core and pass immutable response data into verified functions. [`third_party_input.py`](fixed/third_party_input.py) demonstrates this side of the boundary.
- After reviewing the implementation, describe the dependency through Efct's [external library trust manifest](../../README.md#external-libraries). The manifest must list every effect and partial behavior; merely changing the decorator cannot certify an unknown implementation.

## Unproven termination

[`unproven_termination.py`](rejected/unproven_termination.py) claims an empty partial bound but has a path that repeats forever:

```python
@pure()
def wait_forever() -> None:
    while True:
        pass
```

Efct records possible nontermination as `Diverge`:

```text
P1001 Function wait_forever contains undeclared partial behavior diverge
```

[`bounded_iteration.py`](fixed/bounded_iteration.py) replaces the unbounded loop with a finite `range`. If nontermination is intentional, the alternative is to declare `@pure(partial.Diverge())`. That declaration is a whitelist, not a termination proof: it accurately permits divergence instead of making the loop total.
