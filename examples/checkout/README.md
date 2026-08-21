# Checkout policy example

[简体中文](README.zh-CN.md)

This example calculates an invoice for a sample order. The tax region comes from an environment variable, promotion eligibility depends on the clock, and the result is printed to the console. Efct verifies that those external dependencies remain at declared boundaries while the pricing rules stay deterministic.

## Run the example

From the repository root, check the complete example without executing it:

```console
efct check examples/checkout
```

No output and a zero exit status mean that every checked contract passed.

Run the verified entry point:

```console
efct run examples/checkout/main.py
```

The default region is `standard`:

```text
Order: EFCT-MUG
Region: standard
Subtotal (cents): 7500
Discount (cents): 750
Tax (cents): 556
Total (cents): 7306
```

Set `EFCT_CHECKOUT_REGION` to exercise another explicit input boundary:

```console
EFCT_CHECKOUT_REGION=reduced efct run examples/checkout/main.py
```

## Program structure

The example contains two modules:

- `adapters.py` reads the environment and clock under explicit effect contracts.
- `main.py` defines immutable order and invoice records, applies pure pricing rules, declares console output, and composes the complete program.

The adapter functions expose external values as ordinary immutable data:

```python
from efct import effect, effects


@effects(effect.Environment())
def checkout_region() -> str:
    return os.getenv("EFCT_CHECKOUT_REGION", "standard")


@effects(effect.Clock())
def current_time_ns() -> int:
    return time.time_ns()
```

The pricing function receives those values instead of reading the outside world itself:

```python
from efct import pure


@pure()
def calculate_invoice(
    order: Order,
    region: str,
    current_time: int,
) -> Invoice:
    ...
```

Prices use integer cents. Tax and discount arithmetic uses statically non-zero integer divisors, so the explicit empty partial bound in `@pure()` can be proven.

The program boundary lists every external effect and observable partial behavior:

```python
from efct import effect, effects, partial


@effects(
    effect.Console(),
    effect.Clock(),
    effect.Environment(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def run() -> None:
    ...
```

## What Efct catches

Suppose clock access is moved into a function declared as explicitly pure:

```python
import time

from efct import pure


@pure()
def promotion_active() -> bool:
    return time.time_ns() < 4102444800000000000
```

`efct check` rejects it. With file locations omitted, the diagnostic core is:

```text
P1001
Function promotion_active contains undeclared effect clock
Effect source:
  Call time.time_ns
Suggestion: Declare @efct.effects("clock") or remove the effectful operation
```

Changing the decorator to `@effects(effect.Clock())` would describe an effectful function accurately. This example instead keeps the pricing rule pure: `current_time_ns()` reads the clock once at the boundary, and `calculate_invoice(...)` receives the resulting integer explicitly.

The same structure applies to the tax region. Calling `os.getenv(...)` from the pricing core would add an undeclared `environment` effect; passing the region as a `str` preserves deterministic calculation and makes the dependency visible to callers and tests.
