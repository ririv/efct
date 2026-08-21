import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _codes(source: str) -> list[str]:
    return [str(item["code"]) for item in _diagnostics(source)]


def test_accepts_basic_pure_functions() -> None:
    source = """import efct

@efct.pure
def add(x: int, y: int) -> int:
    return x + y

@efct.pure
def normalize(text: str) -> str:
    return text.strip().lower()
"""

    assert _diagnostics(source) == []


def test_accepts_fully_declared_console_effect() -> None:
    source = """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
"""

    assert _diagnostics(source) == []


def test_accepts_typed_effect_declarations() -> None:
    source = """import efct
from efct import effect, partial

@efct.effects(
    effect.Console(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def reject(value: int) -> None:
    print(value)
    raise ValueError("rejected")
"""

    assert _diagnostics(source) == []


def test_accepts_efct_symbols_imported_with_explicit_aliases() -> None:
    source = """from efct import effect as effect_model
from efct import effects as verified_effects
from efct import partial as partial_model
from efct import pure as verified_pure

_efct = verified_effects(
    effect_model.Console(),
    partial_model.Raise(OSError),
    partial_model.Raise(ValueError),
)

@verified_pure()
def increment(value: int) -> int:
    return value + 1

@verified_effects(
    effect_model.Console(),
    partial_model.Raise(OSError),
    partial_model.Raise(ValueError),
)
def show(value: int) -> None:
    print(increment(value))

show(1)
"""

    assert _diagnostics(source) == []


def test_rejects_unimported_bare_efct_marker() -> None:
    source = """@pure
def increment(value: int) -> int:
    return value + 1
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1006")
    assert diagnostic["message"] == (
        "Only @efct.pure(...), @efct.effects, or @efct.effects(...) markers are allowed"
    )


def test_rejects_efct_name_not_bound_by_aliased_import() -> None:
    source = """import efct as verified

@efct.pure
def increment(value: int) -> int:
    return value + 1
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1006")
    assert diagnostic["message"] == (
        "Only @efct.pure(...), @efct.effects, or @efct.effects(...) markers are allowed"
    )


def test_inferred_pure_accepts_and_propagates_partial_behavior() -> None:
    source = """import efct

@efct.pure
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

@efct.pure
def call_reject(value: int) -> int:
    return reject(value)
"""

    assert _diagnostics(source) == []


def test_empty_pure_contract_rejects_partial_behavior() -> None:
    source = """import efct

@efct.pure()
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1001")
    assert diagnostic["message"] == (
        "Function reject contains undeclared partial behavior raise:builtins.ValueError"
    )


def test_bounded_pure_accepts_declared_partial_behavior() -> None:
    source = """import efct
from efct import partial

@efct.pure(partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    assert _diagnostics(source) == []


def test_bounded_pure_accepts_stable_string_partial_behavior() -> None:
    source = """import efct

@efct.pure("raise:builtins.ValueError")
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    assert _diagnostics(source) == []


def test_pure_contract_rejects_external_effect_declarations() -> None:
    source = """import efct

@efct.pure(efct.effect.Console())
def show(value: int) -> None:
    print(value)
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1006")
    assert diagnostic["message"] == "A pure contract may only declare partial behavior"


def test_rejects_mixed_effect_declaration_forms() -> None:
    source = """import efct

@efct.effects("console", efct.effect.Network())
def show(value: int) -> None:
    print(value)
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1006")
    assert diagnostic["message"] == "String and typed effect declarations cannot be mixed"


def test_accepts_pure_module_initialization_contract() -> None:
    source = """import efct

_efct = efct.pure
"""

    assert _diagnostics(source) == []


def test_accepts_declared_module_initialization_effect() -> None:
    source = """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("ready")
"""

    assert _diagnostics(source) == []


def test_accepts_typed_module_initialization_effect() -> None:
    source = """import efct

_efct = efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
print("ready")
"""

    assert _diagnostics(source) == []


def test_module_initialization_effect_propagates_through_function_call() -> None:
    source = """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show() -> None:
    print("ready")

show()
"""

    assert _diagnostics(source) == []


def test_pure_module_may_define_an_uncalled_effect_function() -> None:
    source = """import efct

_efct = efct.pure

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show() -> None:
    print("ready")
"""

    assert _diagnostics(source) == []


def test_rejects_effect_in_pure_module_initialization() -> None:
    source = """import efct

_efct = efct.pure
print("not pure")
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1001")
    assert diagnostic["message"] == "Module initialization contains undeclared effect console"
    assert diagnostic["span"]["start_line"] == 4


def test_accepts_uncontracted_module_execution_without_claiming_purity() -> None:
    source = '''import efct

value = 1
print("ordinary module")
if value:
    print("still ordinary")
match value:
    case 1:
        pass

@efct.pure
def identity(item: int) -> int:
    return item
'''

    assert _diagnostics(source) == []


def test_module_contract_enables_initializer_syntax_validation() -> None:
    source = """import efct

_efct = efct.pure
value = 1
"""

    assert "P1401" in _codes(source)


def test_rejects_dynamic_duplicate_and_empty_module_contracts() -> None:
    dynamic = """import efct

contract: str = "pure"
_efct = contract
"""
    duplicate = """import efct

_efct = efct.pure
_efct = efct.pure
"""
    empty = """import efct

_efct = efct.effects()
"""

    assert "P1006" in _codes(dynamic)
    assert "P1006" in _codes(duplicate)
    assert "P1006" in _codes(empty)


def test_rejects_console_effect_in_pure_function() -> None:
    source = """import efct

@efct.pure
def bad(value: int) -> int:
    print(value)
    return value
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1001")
    assert diagnostic["span"] == {
        "start_line": 5,
        "start_utf8_byte": 4,
        "end_line": 5,
        "end_utf8_byte": 16,
    }
    assert diagnostic["effect_trace"] == [
        {
            "function": "bad",
            "filename": "fixture.py",
            "span": diagnostic["span"],
            "operation": "Call builtins.print",
        }
    ]


def test_rejects_missing_and_forbidden_types() -> None:
    missing = """import efct

@efct.pure
def bad(value) -> int:
    return 1
"""
    mutable = """import efct

@efct.pure
def bad(value: list[int]) -> int:
    return 1
"""
    any_type = """import efct
import typing

@efct.pure
def bad(value: typing.Any) -> int:
    return 1
"""

    assert "P1101" in _codes(missing)
    assert "P1201" in _codes(mutable)
    assert "P1103" in _codes(any_type)


def test_rejects_wrong_return_type_and_unknown_call() -> None:
    wrong_return = """import efct

@efct.pure
def bad(value: int) -> str:
    return value
"""
    unknown_call = """import efct

@efct.pure
def bad(value: int) -> int:
    mystery(value)
    return value
"""

    assert "P1104" in _codes(wrong_return)
    assert "P1004" in _codes(unknown_call)


def test_accepts_tuple_loop_and_local_rebinding() -> None:
    source = """import efct

@efct.pure
def total(values: tuple[int, ...]) -> int:
    result = 0
    for value in values:
        result += value
    return result
"""

    assert _diagnostics(source) == []


def test_fixed_tuple_can_return_as_homogeneous_variadic_tuple() -> None:
    source = """import efct

@efct.pure
def pair() -> tuple[int, ...]:
    return (1, 2)
"""

    assert _diagnostics(source) == []


def test_range_step_must_be_statically_nonzero() -> None:
    accepted = """import efct

@efct.pure
def count() -> int:
    result = 0
    for value in range(0, 10, -1):
        result += value
    return result
"""
    rejected = """import efct

@efct.pure
def count(step: int) -> int:
    result = 0
    for value in range(0, 10, step):
        result += value
    return result
"""

    assert _diagnostics(accepted) == []
    assert "P1104" in _codes(rejected)


def test_propagates_declared_effect_in_same_module() -> None:
    source = """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)

@efct.pure
def bad(value: int) -> int:
    show(value)
    return value
"""

    diagnostics = _diagnostics(source)
    assert any(item["code"] == "P1001" and "console" in str(item["message"]) for item in diagnostics)


def test_call_propagates_actual_effects_instead_of_unused_declarations() -> None:
    source = """import efct

@efct.effects("console")
def declared_but_unused(value: int) -> int:
    return value

@efct.pure
def caller(value: int) -> int:
    return declared_but_unused(value)
"""

    diagnostics = _diagnostics(source)
    assert not any(item["severity"] == "Error" for item in diagnostics)
    assert [item["code"] for item in diagnostics] == ["W1001"]


def test_recursive_pure_function_has_empty_fixed_point() -> None:
    source = """import efct

@efct.pure
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)
"""

    assert _diagnostics(source) == []


def test_indirect_effect_diagnostic_contains_complete_call_path() -> None:
    source = """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def middle(value: int) -> None:
    show(value)

@efct.pure
def bad(value: int) -> int:
    middle(value)
    return value
"""

    diagnostic = next(item for item in _diagnostics(source) if item["code"] == "P1001")
    assert diagnostic["trace"] == ["bad", "middle", "show", "console"]
    assert [frame["function"] for frame in diagnostic["effect_trace"]] == [
        "bad",
        "middle",
        "show",
    ]
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call middle",
        "Call show",
        "Call builtins.print",
    ]
    assert [frame["span"]["start_line"] for frame in diagnostic["effect_trace"]] == [
        13,
        9,
        5,
    ]


def test_calling_invalid_function_is_also_rejected() -> None:
    source = """import efct

@efct.pure
def broken(value: int) -> str:
    return value

@efct.pure
def caller(value: int) -> str:
    return broken(value)
"""

    diagnostics = _diagnostics(source)
    assert any(item["code"] == "P1004" and item["function"] == "caller" for item in diagnostics)


def test_accepts_declared_exception_effect() -> None:
    source = """import efct

@efct.effects("raise:builtins.ValueError")
def reject(message: str) -> None:
    raise ValueError(message)
"""

    assert _diagnostics(source) == []


def test_function_remains_pure_after_handling_direct_exception() -> None:
    source = """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
"""

    assert _diagnostics(source) == []


def test_handles_exception_effect_propagated_by_call() -> None:
    source = """import efct

@efct.effects("raise:builtins.ValueError")
def reject(message: str) -> None:
    raise ValueError(message)

@efct.pure
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        return 0
    return 1
"""

    assert _diagnostics(source) == []


def test_exception_handling_only_removes_matching_effect() -> None:
    source = """import efct

@efct.effects("console", "raise:builtins.ValueError")
def reject(message: str) -> None:
    print(message)
    raise ValueError(message)

@efct.pure
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        return 0
    return 1
"""

    diagnostics = _diagnostics(source)
    assert any(item["code"] == "P1001" and "console" in str(item["message"]) for item in diagnostics)
    assert not any("raise:builtins.ValueError" in str(item["message"]) for item in diagnostics)


def test_rejects_ambiguous_exception_handling_syntax() -> None:
    bare = """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except:
        return 0
"""
    assert "P1401" in _codes(bare)


def test_exception_handler_type_cannot_be_shadowed_by_parameter() -> None:
    source = """import efct

@efct.pure
def recover(ValueError: int, message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
"""

    assert "P1104" in _codes(source)


def test_exception_handler_type_cannot_be_shadowed_by_later_local_assignment() -> None:
    source = """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
    ValueError = 1
"""

    assert "P1104" in _codes(source)


def test_handler_effect_is_not_caught_by_sibling_handler() -> None:
    source = """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        print(message)
        return 0
"""

    diagnostics = _diagnostics(source)
    assert any(item["code"] == "P1001" and "console" in str(item["message"]) for item in diagnostics)


def test_effects_from_an_unreachable_handler_are_ignored() -> None:
    source = """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def log(message: str) -> None:
    print(message)

@efct.pure()
def value() -> int:
    try:
        return 1
    except ValueError:
        log("unreachable")
        return 0
"""

    assert _diagnostics(source) == []


def test_only_reachable_handlers_contribute_control_flow() -> None:
    unreachable = """import efct

@efct.pure()
def value() -> int:
    try:
        return 1
    except ValueError:
        pass
"""
    reachable = """import efct

@efct.pure()
def value() -> int:
    try:
        raise ValueError("value")
    except ValueError:
        pass
"""

    assert _diagnostics(unreachable) == []
    assert any(item["code"] == "P1105" for item in _diagnostics(reachable))


def test_handler_reachability_uses_cross_function_exception_inference() -> None:
    source = """import efct

@efct.pure
def reject(message: str) -> None:
    raise ValueError(message)

@efct.pure()
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        print("reachable")
        return 0
    return 1
"""

    diagnostic = next(
        item
        for item in _diagnostics(source)
        if item["code"] == "P1001" and item["function"] == "recover"
    )
    assert diagnostic["message"] == "Function recover contains undeclared effect console"


def test_handler_reachability_respects_exception_hierarchy_and_order() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise TypeError("type")
    except ValueError:
        print("unreachable")
        return 0
    except Exception:
        return 1
"""

    assert _diagnostics(source) == []


def test_outer_handler_is_unreachable_after_nested_try_handles_exception() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        try:
            raise ValueError("value")
        except ValueError:
            return 1
    except ValueError:
        print("unreachable")
        return 0
"""

    assert _diagnostics(source) == []


def test_handler_reachability_converges_for_recursive_calls() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)

@efct.pure(efct.partial.Diverge())
def run(value: int) -> int:
    try:
        return countdown(value)
    except ValueError:
        print("unreachable")
        return 0
"""

    assert _diagnostics(source) == []


def test_higher_order_pure_function_can_call_exact_pure_callable_parameter() -> None:
    source = """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.pure()
def increment(value: int) -> int:
    return value + 1

@efct.pure
def run(value: int) -> int:
    return apply(increment, value)
"""

    assert _diagnostics(source) == []


def test_effect_generic_is_instantiated_from_concrete_callback() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure()
def increment(value: int) -> int:
    return value + 1

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value

@efct.pure
def pure_run(value: int) -> int:
    return apply(increment, value)

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def effect_run(value: int) -> int:
    return apply(show, value)
"""

    assert _diagnostics(source) == []


def test_effect_generic_propagation_rejects_pure_caller() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value

@efct.pure
def bad(value: int) -> int:
    return apply(show, value)
"""

    diagnostics = _diagnostics(source)
    assert any(
        item["code"] == "P1001" and "console" in str(item["message"])
        for item in diagnostics
    )


def test_pure_function_can_retain_effect_generic_capability() -> None:
    source = """import efct

@efct.pure
def keep[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
) -> efct.EffectCallable[[int], int, E]:
    return function

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def run(value: int) -> int:
    selected = keep(show)
    return selected(value)
"""

    assert _diagnostics(source) == []


def test_effect_generic_requires_explicit_constraint_and_complete_capability_type() -> None:
    missing_bound = """import efct

@efct.effects
def bad[E](function: efct.EffectCallable[[int], int, E], value: int) -> int:
    return function(value)
"""
    missing_effect = """import efct

@efct.effects
def bad[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int],
    value: int,
) -> int:
    return function(value)
"""

    assert "P1104" in _codes(missing_bound)
    assert "P1104" in _codes(missing_effect)


def test_higher_order_pure_function_can_return_pure_callable_capability() -> None:
    source = """import efct

@efct.pure
def keep(function: efct.PureCallable[[int], int]) -> efct.PureCallable[[int], int]:
    return function
"""

    assert _diagnostics(source) == []


def test_higher_order_call_rejects_effect_function_and_signature_mismatch() -> None:
    effectful = """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value

@efct.pure
def run(value: int) -> int:
    return apply(show, value)
"""
    mismatched = """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.pure()
def normalize(value: str) -> str:
    return value.strip()

@efct.pure
def run(value: int) -> int:
    return apply(normalize, value)
"""

    assert "P1201" in _codes(effectful)
    assert "P1104" in _codes(mismatched)


def test_empty_effect_declaration_cannot_impersonate_pure_callable() -> None:
    source = """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.effects()
def identity(value: int) -> int:
    return value

@efct.pure
def run(value: int) -> int:
    return apply(identity, value)
"""

    assert "P1201" in _codes(source)


def test_passing_invalid_pure_function_is_also_rejected() -> None:
    source = """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.pure()
def broken(value: int) -> int:
    return "bad"

@efct.pure
def run(value: int) -> int:
    return apply(broken, value)
"""

    diagnostics = _diagnostics(source)
    assert any(item["code"] == "P1004" and item["function"] == "run" for item in diagnostics)


def test_pure_callable_type_arguments_must_be_complete_and_pure() -> None:
    missing_list = """import efct

@efct.pure
def bad(function: efct.PureCallable[int, int]) -> int:
    return 0
"""
    mutable = """import efct

@efct.pure
def bad(function: efct.PureCallable[[list[int]], int]) -> int:
    return 0
"""

    assert "P1104" in _codes(missing_list)
    assert "P1201" in _codes(mutable)


def test_list_is_only_allowed_as_pure_callable_parameter_type_list() -> None:
    source = """import efct

@efct.pure
def bad() -> tuple[int, ...]:
    return [1]
"""

    assert "P1401" in _codes(source)


def test_pure_callable_capability_cannot_be_nested_in_data_container() -> None:
    source = """import efct

@efct.pure
def bad(functions: tuple[efct.PureCallable[[int], int]]) -> int:
    return 0
"""

    assert "P1201" in _codes(source)


def test_clock_random_and_environment_registry() -> None:
    source = """import os
import efct
import random
import time

@efct.effects("clock", "random", "environment", "raise:builtins.ValueError")
def sample(low: int, high: int, name: str) -> tuple[int, int, str]:
    return (time.time_ns(), random.randint(low, high), os.getenv(name, ""))
"""

    assert _diagnostics(source) == []


def test_registered_os_operations_accept_declared_effects() -> None:
    source = """import efct
import os

@efct.effects(
    "file.read",
    "file.write",
    "environment",
    "random",
    "process",
    "raise:builtins.NotImplementedError",
    "raise:builtins.OSError",
    "raise:builtins.ValueError",
    "diverge",
)
def use_os(path: str, name: str, value: str, command: str) -> None:
    os.listdir(path)
    os.remove(path)
    os.putenv(name, value)
    os.urandom(16)
    os.system(command)
    os.popen(command)
"""

    assert _diagnostics(source) == []


def test_process_wait_requires_divergence_declaration() -> None:
    source = """import efct
import os

@efct.effects("process", "raise:builtins.OSError", "raise:builtins.ValueError")
def run(command: str) -> int:
    return os.system(command)
"""

    diagnostic = next(
        item for item in _diagnostics(source) if item["code"] == "P1001"
    )
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior diverge"
    )


def test_registered_io_open_accepts_declared_file_effects() -> None:
    source = """import efct
import io

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def read_file(path: str) -> None:
    io.open(path)

@efct.effects("file.write", "raise:builtins.OSError", "raise:builtins.ValueError")
def write_file(path: str) -> None:
    io.open(path, "w")

@efct.effects(
    "file.read",
    "file.write",
    "raise:builtins.OSError",
    "raise:builtins.ValueError",
)
def update_file(path: str) -> None:
    io.open(path, "r+")

@efct.effects("file.write", "raise:builtins.OSError", "raise:builtins.ValueError")
def write_with_builtin(path: str) -> None:
    open(path, "w")
"""

    assert _diagnostics(source) == []


def test_file_open_rejects_dynamic_and_unsupported_modes() -> None:
    dynamic = """import efct
import io

@efct.effects("file.read")
def read_file(path: str, mode: str) -> None:
    io.open(path, mode)
"""
    unsupported = """import efct
import io

@efct.effects("file.read")
def read_file(path: str) -> None:
    io.open(path, "unknown")
"""

    dynamic_diagnostic = _diagnostics(dynamic)[0]
    unsupported_diagnostic = _diagnostics(unsupported)[0]

    assert dynamic_diagnostic["code"] == "P1104"
    assert dynamic_diagnostic["message"] == "The file mode must be a static string literal"
    assert unsupported_diagnostic["code"] == "P1104"
    assert unsupported_diagnostic["message"] == (
        "The file mode is not supported by the Python API model"
    )


def test_python_api_model_resolves_import_aliases() -> None:
    source = """import efct
import os as operating_system

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def list_directory(path: str) -> None:
    operating_system.listdir(path)
"""

    assert _diagnostics(source) == []


def test_python_api_model_resolves_symbol_imports() -> None:
    source = """import efct
from os import listdir as list_directory

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def scan(path: str) -> None:
    list_directory(path)
"""

    assert _diagnostics(source) == []


def test_python_api_model_does_not_capture_shadowed_module_names() -> None:
    source = """import efct
import os

@efct.pure
def inspect(os: str, path: str) -> None:
    os.listdir(path)
"""

    diagnostics = _diagnostics(source)

    assert [item["code"] for item in diagnostics] == ["P1004"]
    assert diagnostics[0]["message"] == "Method str.listdir is not registered"


def test_unmodeled_operation_on_registered_module_is_explicitly_rejected() -> None:
    source = """import efct
import os

@efct.pure
def inspect(path: str) -> None:
    os.stat(path)
"""

    diagnostics = _diagnostics(source)

    assert [item["code"] for item in diagnostics] == ["P1004"]
    assert diagnostics[0]["message"] == "Python API operation os.stat is not modeled"


def test_accepts_immutable_containers_and_tagged_union_boundaries() -> None:
    source = """from typing import Optional as Maybe

import efct
import typing as t

@efct.pure
def keep_optional(value: int | None) -> int | None:
    return value

@efct.pure
def keep_qualified_optional(value: t.Optional[int]) -> t.Optional[int]:
    return value

@efct.pure
def keep_imported_optional(value: Maybe[int]) -> Maybe[int]:
    return value

@efct.pure
def keep_result(value: efct.Result[int, str]) -> efct.Result[int, str]:
    return value

@efct.pure
def keep_map(value: efct.FrozenMap[str, int]) -> efct.FrozenMap[str, int]:
    return value

@efct.pure
def keep_set(value: frozenset[int]) -> frozenset[int]:
    return value
"""

    assert _diagnostics(source) == []


def test_constructs_immutable_containers_and_tagged_unions() -> None:
    source = """import efct

@efct.pure
def present(value: int) -> int | None:
    return value

@efct.pure
def missing() -> int | None:
    return None

@efct.pure
def ok(value: int) -> efct.Result[int, str]:
    return efct.Ok(value)

@efct.pure
def frozen() -> efct.FrozenMap[str, int]:
    return efct.FrozenMap((("answer", 42),))
"""

    assert _diagnostics(source) == []


def test_result_match_is_exhaustive_and_narrows_variants() -> None:
    source = """import efct

@efct.pure
def unwrap_or(result: efct.Result[int, str], fallback: int) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Err(_):
            return fallback

@efct.pure
def read_variant_field(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok():
            return result.value
        case efct.Err():
            return 0

@efct.pure
def increment(result: efct.Result[int, str]) -> efct.Result[int, str]:
    match result:
        case efct.Err(error):
            return efct.Err(error)
        case efct.Ok(value):
            return efct.Ok(value + 1)
"""

    assert _diagnostics(source) == []


def test_result_match_rejects_missing_and_duplicate_variants() -> None:
    missing = """import efct

@efct.pure
def unwrap(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value):
            return value
"""
    duplicate = """import efct

@efct.pure
def unwrap(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Ok(other):
            return other
        case efct.Err(_):
            return 0
"""

    missing_diagnostics = _diagnostics(missing)
    duplicate_diagnostics = _diagnostics(duplicate)

    assert [item["code"] for item in missing_diagnostics] == ["P1401"]
    assert missing_diagnostics[0]["message"] == "Result match is not exhaustive; missing Err"
    assert [item["code"] for item in duplicate_diagnostics] == ["P1401"]
    assert duplicate_diagnostics[0]["message"] == (
        "The Ok Result pattern is duplicated and unreachable"
    )


def test_result_match_rejects_dynamic_patterns() -> None:
    guarded = """import efct

@efct.pure
def unwrap(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value) if value == 0:
            return value
        case efct.Err(_):
            return 0
"""
    wildcard = """import efct

@efct.pure
def unwrap(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value):
            return value
        case _:
            return 0
"""
    wrong_subject = """import efct

@efct.pure
def classify(value: int) -> int:
    match value:
        case efct.Ok(item):
            return item
        case efct.Err(_):
            return 0
"""

    assert _diagnostics(guarded)[0]["message"] == "Match guards are not currently supported"
    assert _diagnostics(wildcard)[0]["message"] == (
        "A Result match must use explicit efct.Ok and efct.Err class patterns"
    )
    assert _diagnostics(wrong_subject)[0]["message"] == (
        "A supported match subject must be Result, not int"
    )


def test_result_match_propagates_branch_effects() -> None:
    source = """import efct

@efct.pure
def inspect(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Err(error):
            print(error)
            return 0
"""

    diagnostics = _diagnostics(source)

    assert [item["code"] for item in diagnostics] == ["P1001"]
    assert diagnostics[0]["message"] == "Function inspect contains undeclared effect console"


def test_rejects_non_optional_and_nested_unions() -> None:
    source = """import efct

@efct.pure
def general(value: int | str) -> int:
    return 1

@efct.pure
def nested(value: (int | None) | None) -> int:
    return 1
"""

    diagnostics = _diagnostics(source)

    assert [item["code"] for item in diagnostics] == ["P1104", "P1104"]
    assert diagnostics[0]["message"] == "Only a union of one type and None is supported"
    assert diagnostics[1]["message"] == "An optional type must contain exactly one non-None type"


def test_pure_record_construction_and_field_access() -> None:
    source = """from dataclasses import dataclass

import efct

@efct.pure
@dataclass(frozen=True, slots=True)
class User:
    name: str
    level: int

@efct.pure
def promote(user: User) -> User:
    return User(user.name, user.level + 1)
"""

    assert _diagnostics(source) == []


def test_pure_record_rejects_methods_and_mutable_fields() -> None:
    method = """from dataclasses import dataclass
import efct
@efct.pure
@dataclass(frozen=True, slots=True)
class Bad:
    value: int
    def method(self) -> int:
        return self.value
"""
    mutable = """from dataclasses import dataclass
import efct
@efct.pure
@dataclass(frozen=True, slots=True)
class Bad:
    values: list[int]
"""
    assert "P1201" in _codes(method)
    assert "P1201" in _codes(mutable)


def test_rejects_incomplete_return_path() -> None:
    source = """import efct

@efct.pure
def choose(flag: bool) -> int:
    if flag:
        return 1
"""

    assert "P1105" in _codes(source)


def test_object_identity_comparison_only_allows_none() -> None:
    rejected = """import efct

@efct.pure
def same(left: int, right: int) -> bool:
    return left is right
"""
    accepted = """import efct

@efct.pure
def empty(value: None) -> bool:
    return value is None
"""

    assert "P1401" in _codes(rejected)
    assert _diagnostics(accepted) == []


def test_pure_function_can_construct_a_registered_local_exception_object() -> None:
    source = """import efct

@efct.pure
def bad(message: str) -> None:
    ValueError(message)
"""

    assert _diagnostics(source) == []


def test_local_list_can_be_mutated_and_consumed_through_alias() -> None:
    source = """import efct

@efct.pure
def total(value: int) -> int:
    values = [1, 2]
    alias = values
    alias.append(value)
    return sum(values) + len(alias)
"""

    assert _diagnostics(source) == []


def test_local_list_rejects_wrong_elements_and_empty_literal() -> None:
    wrong_element = """import efct

@efct.pure
def bad() -> int:
    values = [1]
    values.append("bad")
    return sum(values)
"""
    empty = """import efct

@efct.pure
def bad() -> int:
    values = []
    return len(values)
"""

    assert "P1104" in _codes(wrong_element)
    assert "P1104" in _codes(empty)


def test_local_list_cannot_escape_through_return_or_container_nesting() -> None:
    returned = """import efct

@efct.pure
def bad() -> tuple[int, ...]:
    values = [1]
    return values
"""
    embedded = """import efct

@efct.pure
def bad() -> tuple[int, ...]:
    values = [1]
    wrapped = (values,)
    return (1,)
"""

    assert "P1202" in _codes(returned)
    assert "P1202" in _codes(embedded)


def test_local_list_cannot_escape_as_call_argument() -> None:
    source = """import efct

@efct.pure
def consume(values: tuple[int, ...]) -> int:
    return sum(values)

@efct.pure
def bad() -> int:
    values = [1]
    return consume(values)
"""

    assert "P1202" in _codes(source)
