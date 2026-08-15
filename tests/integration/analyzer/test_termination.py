import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _diagnostic(source: str, code: str) -> dict[str, object]:
    return next(item for item in _diagnostics(source) if item["code"] == code)


def test_static_true_loop_requires_divergence_declaration() -> None:
    source = """import efct

@efct.pure()
def wait() -> None:
    while True:
        pass
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function wait contains undeclared partial behavior diverge"
    )
    assert diagnostic["suggestion"] == (
        "Declare @efct.pure(efct.partial.Diverge()) or prove that the operation "
        "terminates"
    )
    assert diagnostic["effect_trace"][0]["operation"] == "Repeat while loop"


def test_typed_divergence_declaration_accepts_static_true_loop() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    while True:
        pass
"""

    assert _diagnostics(source) == []


def test_string_divergence_declaration_accepts_static_true_loop() -> None:
    source = """import efct

@efct.pure("diverge")
def wait() -> None:
    while True:
        pass
"""

    assert _diagnostics(source) == []


def test_bare_pure_infers_divergence() -> None:
    source = """import efct

@efct.pure
def wait() -> None:
    while True:
        pass
"""

    assert _diagnostics(source) == []


def test_static_false_loop_body_is_unreachable() -> None:
    source = """import efct

@efct.pure()
def finish() -> int:
    while False:
        print("unreachable")
    else:
        return 1
"""

    assert _diagnostics(source) == []


def test_static_true_loop_with_break_is_proven_finite() -> None:
    source = """import efct

@efct.pure()
def finish() -> int:
    while True:
        break
    return 1
"""

    assert _diagnostics(source) == []


def test_unknown_loop_with_repeating_path_may_diverge() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait(condition: bool) -> None:
    while condition:
        pass
"""

    assert _diagnostics(source) == []


def test_unknown_loop_with_unconditional_break_is_proven_finite() -> None:
    source = """import efct

@efct.pure()
def finish(condition: bool) -> int:
    while condition:
        break
    return 1
"""

    assert _diagnostics(source) == []


def test_static_true_loop_makes_following_statement_unreachable() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> int:
    while True:
        pass
    print("unreachable")
"""

    assert _diagnostics(source) == []


def test_direct_recursion_requires_divergence_declaration() -> None:
    source = """import efct

@efct.pure()
def recurse(value: int) -> int:
    if value == 0:
        return 0
    return recurse(value - 1)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function recurse contains undeclared partial behavior diverge"
    )
    assert diagnostic["effect_trace"][0]["operation"] == (
        "Recursive call to recurse"
    )


def test_direct_recursion_accepts_divergence_declaration() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def recurse(value: int) -> int:
    if value == 0:
        return 0
    return recurse(value - 1)
"""

    assert _diagnostics(source) == []


def test_guarded_decreasing_integer_recursion_is_proven_finite() -> None:
    source = """import efct

@efct.pure()
def countdown(value: int) -> int:
    if value <= 0:
        return 0
    return countdown(value - 1)
"""

    assert _diagnostics(source) == []


def test_recursive_branch_guard_proves_decreasing_integer_recursion() -> None:
    source = """import efct

@efct.pure()
def countdown(value: int) -> int:
    if value > 0:
        return countdown(value - 1)
    return 0
"""

    assert _diagnostics(source) == []


def test_guarded_increasing_integer_recursion_is_proven_finite() -> None:
    source = """import efct

@efct.pure()
def countup(value: int) -> int:
    if value >= 0:
        return 0
    return countup(value + 1)
"""

    assert _diagnostics(source) == []


def test_reversed_and_negated_guard_proves_integer_recursion() -> None:
    source = """import efct

@efct.pure()
def countdown(value: int) -> int:
    if not 0 < value:
        return 0
    return countdown(value - 1)
"""

    assert _diagnostics(source) == []


def test_asserted_bound_does_not_prove_integer_recursion() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def countdown(value: int) -> int:
    assert value >= 0
    if value == 0:
        return 0
    return countdown(value - 1)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function countdown contains undeclared partial behavior diverge"
    )


def test_multiple_recursive_calls_must_share_one_integer_measure() -> None:
    source = """import efct

@efct.pure()
def fibonacci(value: int) -> int:
    if value <= 1:
        return value
    return fibonacci(value - 1) + fibonacci(value - 2)
"""

    assert _diagnostics(source) == []


def test_wrong_recursive_direction_still_may_diverge() -> None:
    source = """import efct

@efct.pure()
def grow(value: int) -> int:
    if value <= 0:
        return 0
    return grow(value + 1)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function grow contains undeclared partial behavior diverge"
    )


def test_parameter_reassignment_invalidates_recursive_measure() -> None:
    source = """import efct

@efct.pure()
def reset(value: int) -> int:
    value = 1
    if value <= 0:
        return 0
    return reset(value - 1)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function reset contains undeclared partial behavior diverge"
    )


def test_different_measures_do_not_form_an_implicit_lexicographic_proof() -> None:
    source = """import efct

@efct.pure()
def reduce_pair(left: int, right: int) -> int:
    if left > 0:
        return reduce_pair(left - 1, right)
    if right > 0:
        return reduce_pair(left, right - 1)
    return 0
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function reduce_pair contains undeclared partial behavior diverge"
    )


def test_mutual_recursion_marks_each_function_as_may_diverge() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def left(value: int) -> int:
    if value == 0:
        return 0
    return right(value - 1)

@efct.pure(efct.partial.Diverge())
def right(value: int) -> int:
    if value == 0:
        return 0
    return left(value - 1)
"""

    assert _diagnostics(source) == []


def test_static_unreachable_recursive_call_does_not_form_cycle() -> None:
    source = """import efct

@efct.pure()
def finish() -> int:
    if False:
        return finish()
    return 1
"""

    assert _diagnostics(source) == []


def test_exception_handler_cannot_catch_divergence() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    try:
        while True:
            pass
    except Exception:
        pass
"""

    assert _diagnostics(source) == []


def test_finally_return_does_not_override_already_diverging_path() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> int:
    try:
        while True:
            pass
    finally:
        return 1
"""

    assert _diagnostics(source) == []


def test_diverging_finally_overrides_pending_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    try:
        raise ValueError("failure")
    finally:
        while True:
            pass
"""

    assert _diagnostics(source) == []


def test_except_star_handler_may_diverge_without_group_remerge() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    try:
        raise ValueError("failure")
    except* ValueError:
        while True:
            pass
"""

    assert _diagnostics(source) == []


def test_finite_for_loop_does_not_introduce_divergence() -> None:
    source = """import efct

@efct.pure()
def finish() -> None:
    for value in range(3):
        value + 1
"""

    assert _diagnostics(source) == []


def test_divergence_propagates_across_function_call() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    while True:
        pass

@efct.pure(efct.partial.Diverge())
def caller() -> None:
    wait()
"""

    assert _diagnostics(source) == []


def test_effect_generic_propagates_divergence() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure(efct.partial.Diverge())
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)

@efct.pure(efct.partial.Diverge())
def run(value: int) -> int:
    return apply(countdown, value)
"""

    assert _diagnostics(source) == []


def test_effect_generic_divergence_is_rejected_by_empty_caller() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure(efct.partial.Diverge())
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)

@efct.pure()
def run(value: int) -> int:
    return apply(countdown, value)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior diverge"
    )


def test_module_initializer_can_declare_divergence() -> None:
    source = """import efct

_efct = efct.effects(efct.partial.Diverge())

@efct.pure(efct.partial.Diverge())
def wait() -> None:
    while True:
        pass

wait()
"""

    assert _diagnostics(source) == []
