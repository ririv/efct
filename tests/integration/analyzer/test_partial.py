import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _diagnostic(source: str, code: str) -> dict[str, object]:
    return next(item for item in _diagnostics(source) if item["code"] == code)


def test_exact_exception_handler_satisfies_empty_partial_contract() -> None:
    source = """import efct

@efct.pure()
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
"""

    assert _diagnostics(source) == []


def test_exception_binding_can_be_rendered_inside_handler() -> None:
    source = """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError as error:
        print(str(error))
        return 0
"""

    assert _diagnostics(source) == []


def test_str_remains_restricted_to_exception_objects() -> None:
    source = """import efct

@efct.pure()
def invalid(value: int) -> str:
    return str(value)
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == "str does not accept type int"


def test_bound_parent_handler_reraises_exact_caught_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError as error:
        raise error
"""

    assert _diagnostics(source) == []


def test_explicit_exception_cause_is_metadata_not_an_escaping_partial() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError as error:
        raise TypeError("wrapped") from error
"""

    assert _diagnostics(source) == []


def test_raise_from_none_suppresses_context_without_adding_partial_behavior() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError:
        raise TypeError("wrapped") from None
"""

    assert _diagnostics(source) == []


def test_raise_cause_expression_may_evaluate_to_none() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject(cause: None) -> None:
    raise ValueError("value") from cause
"""

    assert _diagnostics(source) == []


def test_raise_cause_failure_prevents_primary_exception_from_escaping() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def reject() -> None:
    raise ValueError("primary") from TypeError(("cause",)[1])
"""

    assert _diagnostics(source) == []


def test_raise_cause_evaluation_partial_is_combined_with_primary_exception() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(IndexError),
)
def reject(messages: tuple[str, ...], index: int) -> None:
    raise ValueError("primary") from TypeError(messages[index])
"""

    assert _diagnostics(source) == []


def test_primary_exception_failure_skips_cause_evaluation() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def reject() -> None:
    raise ValueError(("primary",)[1]) from TypeError(1 // 0)
"""

    assert _diagnostics(source) == []


def test_raise_cause_must_be_registered_exception_or_none() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def invalid() -> None:
    raise ValueError("primary") from 1
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "The raise cause must be a registered exception or None"
    )


def test_exception_binding_is_cleared_after_handler() -> None:
    source = """import efct

@efct.pure()
def invalid(message: str) -> str:
    try:
        raise ValueError(message)
    except ValueError as error:
        pass
    return str(error)
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "Exception binding error is only available inside its handler"
    )


def test_exception_binding_clear_does_not_restore_shadowed_parameter() -> None:
    source = """import efct

@efct.pure()
def invalid(flag: bool, error: int) -> int:
    try:
        if flag:
            raise ValueError("value")
    except ValueError as error:
        pass
    return error
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "Exception binding error is only available inside its handler"
    )


def test_exception_binding_alias_survives_handler_cleanup() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError as error:
        saved = error
    raise saved
"""

    assert _diagnostics(source) == []


def test_unreachable_bound_handler_does_not_contribute_effects() -> None:
    source = """import efct

@efct.pure()
def identity(value: int) -> int:
    return value

@efct.pure()
def value() -> int:
    try:
        return identity(1)
    except ValueError as error:
        print(str(error))
        return 0
"""

    assert _diagnostics(source) == []


def test_partial_behavior_outside_explicit_whitelist_is_rejected() -> None:
    source = """import efct

@efct.pure("raise:builtins.TypeError")
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function reject contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_unused_partial_whitelist_entry_produces_warning() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def identity(value: int) -> int:
    return value
"""

    diagnostic = _diagnostic(source, "W1001")
    assert diagnostic["severity"] == "Warning"
    assert diagnostic["message"] == (
        "Declared partial behavior raise:builtins.ValueError is not used"
    )
    assert diagnostic["suggestion"] == "Remove the unused partial declaration"


def test_duplicate_partial_whitelist_entry_is_rejected() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(ValueError),
)
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == (
        "Declaration raise:builtins.ValueError appears more than once"
    )


def test_mixed_partial_declaration_forms_are_rejected() -> None:
    source = """import efct

@efct.pure(
    "raise:builtins.ValueError",
    efct.partial.Raise(ValueError),
)
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == (
        "String and typed partial declarations cannot be mixed"
    )


def test_module_initialization_preserves_partial_contract_states() -> None:
    inferred = """import efct

_efct = efct.pure

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

reject(0)
"""
    bounded = """import efct

_efct = efct.pure(efct.partial.Raise(ValueError))

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

reject(0)
"""
    exact = """import efct

_efct = efct.pure()

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

reject(0)
"""

    assert _diagnostics(inferred) == []
    assert _diagnostics(bounded) == []
    diagnostic = _diagnostic(exact, "P1001")
    assert diagnostic["message"] == (
        "Module initialization contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_effect_generic_propagates_bounded_partial_row() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

@efct.pure(efct.partial.Raise(ValueError))
def run(value: int) -> int:
    return apply(reject, value)
"""

    assert _diagnostics(source) == []


def test_effect_generic_partial_row_is_rejected_by_empty_caller() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

@efct.pure()
def run(value: int) -> int:
    return apply(reject, value)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_registered_builtin_exceptions_can_be_declared_and_raised() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(TypeError),
    efct.partial.Raise(RuntimeError),
    efct.partial.Raise(KeyError),
)
def reject(kind: int) -> int:
    if kind == 0:
        raise TypeError("type")
    if kind == 1:
        raise RuntimeError("runtime")
    if kind == 2:
        raise KeyError("key")
    return kind
"""

    assert _diagnostics(source) == []


def test_bare_exception_class_uses_its_zero_argument_constructor() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    raise ValueError
"""

    assert _diagnostics(source) == []


def test_exception_constructor_uses_a_closed_argument_contract() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject() -> None:
    raise TypeError(1)
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "Exception constructor builtins.TypeError requires zero arguments or "
        "one str argument"
    )


def test_unregistered_builtin_exception_is_rejected_in_typed_declaration() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(SystemExit))
def stop() -> None:
    raise SystemExit("stop")
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == "Exception type raise:SystemExit is not registered"


def test_unregistered_string_exception_declaration_is_rejected() -> None:
    source = """import efct

@efct.pure("raise:builtins.SystemExit")
def identity(value: int) -> int:
    return value
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == (
        "Exception type raise:builtins.SystemExit is not registered"
    )


def test_parent_exception_declaration_covers_registered_subclass() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ArithmeticError))
def reject() -> None:
    raise ZeroDivisionError("zero")
"""

    assert _diagnostics(source) == []


def test_subclass_exception_declaration_does_not_cover_parent() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ZeroDivisionError))
def reject() -> None:
    raise ArithmeticError("arithmetic")
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function reject contains undeclared partial behavior "
        "raise:builtins.ArithmeticError"
    )


def test_parent_exception_handler_catches_registered_subclass() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ZeroDivisionError("zero")
    except ArithmeticError:
        return 0
"""

    assert _diagnostics(source) == []


def test_nonzero_integer_literals_keep_floor_division_and_modulo_total() -> None:
    source = """import efct

@efct.pure()
def calculate(value: int) -> int:
    quotient = value // -2
    return quotient % +3
"""

    assert _diagnostics(source) == []


def test_possible_zero_divisor_is_inferred_as_zero_division_error() -> None:
    source = """import efct

@efct.pure
def quotient(value: int, divisor: int) -> int:
    return value // divisor

@efct.pure
def remainder(value: int) -> int:
    return value % 0
"""

    assert _diagnostics(source) == []


def test_zero_division_partiality_accepts_exact_and_parent_whitelists() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ZeroDivisionError))
def quotient(value: int, divisor: int) -> int:
    return value // divisor

@efct.pure(efct.partial.Raise(ArithmeticError))
def remainder(value: int, divisor: int) -> int:
    return value % divisor
"""

    assert _diagnostics(source) == []


def test_zero_division_partiality_is_rejected_by_explicit_empty_contract() -> None:
    source = """import efct

@efct.pure()
def quotient(value: int, divisor: int) -> int:
    return value // divisor
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function quotient contains undeclared partial behavior "
        "raise:builtins.ZeroDivisionError"
    )
    assert diagnostic["effect_trace"] == [
        {
            "function": "quotient",
            "filename": "fixture.py",
            "span": {
                "start_line": 5,
                "start_utf8_byte": 11,
                "end_line": 5,
                "end_utf8_byte": 27,
            },
            "operation": "Integer floor division",
        }
    ]


def test_exception_handler_catches_implicit_zero_division_partiality() -> None:
    source = """import efct

@efct.pure()
def calculate(value: int, divisor: int) -> int:
    try:
        return value // divisor + value % divisor
    except ArithmeticError:
        return 0
"""

    assert _diagnostics(source) == []


def test_implicit_zero_division_partiality_propagates_across_calls() -> None:
    source = """import efct

@efct.pure
def quotient(value: int, divisor: int) -> int:
    return value // divisor

@efct.pure()
def run(value: int, divisor: int) -> int:
    return quotient(value, divisor)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior "
        "raise:builtins.ZeroDivisionError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call quotient",
        "Integer floor division",
    ]


def test_augmented_floor_division_and_modulo_have_the_same_partiality() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ZeroDivisionError))
def calculate(value: int, divisor: int) -> int:
    value //= divisor
    value %= divisor
    return value
"""

    assert _diagnostics(source) == []


def test_static_tuple_indices_are_total_and_preserve_exact_element_types() -> None:
    source = """import efct

@efct.pure()
def first(values: tuple[int, str]) -> int:
    return values[0]

@efct.pure()
def last(values: tuple[int, str]) -> str:
    return values[-1]
"""

    assert _diagnostics(source) == []


def test_dynamic_homogeneous_tuple_index_infers_index_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def item(values: tuple[int, int], index: int) -> int:
    return values[index]
"""

    assert _diagnostics(source) == []


def test_variadic_tuple_index_accepts_lookup_error_parent_whitelist() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(LookupError))
def item(values: tuple[str, ...], index: int) -> str:
    return values[index]
"""

    assert _diagnostics(source) == []


def test_tuple_index_error_is_rejected_by_explicit_empty_contract() -> None:
    source = """import efct

@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    return values[index]
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function item contains undeclared partial behavior raise:builtins.IndexError"
    )
    assert diagnostic["effect_trace"] == [
        {
            "function": "item",
            "filename": "fixture.py",
            "span": {
                "start_line": 5,
                "start_utf8_byte": 11,
                "end_line": 5,
                "end_utf8_byte": 24,
            },
            "operation": "Index tuple",
        }
    ]


def test_static_out_of_bounds_tuple_index_uses_never_return_type() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def missing() -> str:
    return (1, "value")[2]

@efct.pure(efct.partial.Raise(IndexError))
def empty() -> int:
    return ()[0]
"""

    assert _diagnostics(source) == []


def test_never_short_circuits_eager_expression_evaluation() -> None:
    source = """import efct

@efct.pure()
def consume(first: int, second: int) -> int:
    return first + second

@efct.pure(efct.partial.Raise(IndexError))
def binary() -> int:
    return ()[0] + print("unreachable")

@efct.pure(efct.partial.Raise(IndexError))
def call_argument() -> int:
    return consume(()[0], print("unreachable"))

@efct.pure(efct.partial.Raise(IndexError))
def tuple_element() -> int:
    return (()[0], print("unreachable"))

@efct.pure(efct.partial.Raise(IndexError))
def range_argument(step: int) -> int:
    return range(()[0], 1, step)
"""

    assert _diagnostics(source) == []


def test_never_argument_does_not_hide_an_unregistered_callee() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def invalid() -> int:
    return missing(()[0])
"""

    diagnostic = _diagnostic(source, "P1004")
    assert diagnostic["message"] == "Call target missing is not registered"


def test_never_stops_statement_control_flow() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def assignment() -> int:
    value = ()[0]
    print("unreachable")
    return value

@efct.pure()
def recovered() -> int:
    try:
        value = ()[0]
        print("unreachable")
    except IndexError:
        return 0

@efct.pure(efct.partial.Raise(ValueError))
def duplicate_assignment() -> int:
    mapping = efct.FrozenMap((("same", 1), ("same", 2)))
    print("unreachable")
    return len(mapping)
"""

    assert _diagnostics(source) == []


def test_never_is_the_bottom_type_in_conditional_expressions() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def conditional(flag: bool) -> int:
    return ()[0] if flag else 1

@efct.pure(efct.partial.Raise(IndexError))
def boolean(flag: bool) -> bool:
    return flag and ()[0]

@efct.pure(efct.partial.Raise(IndexError))
def comparison(left: int, right: int) -> bool:
    return left < right < ()[0]
"""

    assert _diagnostics(source) == []


def test_exception_handler_catches_implicit_tuple_index_error() -> None:
    source = """import efct

@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        return 0
"""

    assert _diagnostics(source) == []


def test_dynamic_index_rejects_heterogeneous_fixed_tuple() -> None:
    source = """import efct

@efct.pure
def item(values: tuple[int, str], index: int) -> int:
    return values[index]
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "A dynamic index requires a homogeneous fixed tuple"
    )


def test_tuple_index_requires_exact_int() -> None:
    source = """import efct

@efct.pure
def item(values: tuple[int, ...], index: bool) -> int:
    return values[index]
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == "A tuple index must be an exact int"


def test_implicit_tuple_index_error_propagates_across_calls() -> None:
    source = """import efct

@efct.pure
def item(values: tuple[int, ...], index: int) -> int:
    return values[index]

@efct.pure()
def run(values: tuple[int, ...], index: int) -> int:
    return item(values, index)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior raise:builtins.IndexError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call item",
        "Index tuple",
    ]


def test_frozen_map_lookup_infers_key_error() -> None:
    source = """import efct

@efct.pure
def item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]
"""

    assert _diagnostics(source) == []


def test_frozen_map_lookup_accepts_exact_and_parent_whitelists() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(KeyError))
def exact(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]

@efct.pure(efct.partial.Raise(LookupError))
def parent(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]
"""

    assert _diagnostics(source) == []


def test_frozen_map_key_error_is_rejected_by_explicit_empty_contract() -> None:
    source = """import efct

@efct.pure()
def item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function item contains undeclared partial behavior raise:builtins.KeyError"
    )
    assert diagnostic["effect_trace"] == [
        {
            "function": "item",
            "filename": "fixture.py",
            "span": {
                "start_line": 5,
                "start_utf8_byte": 11,
                "end_line": 5,
                "end_utf8_byte": 23,
            },
            "operation": "Index FrozenMap",
        }
    ]


def test_exception_handler_catches_implicit_frozen_map_key_error() -> None:
    source = """import efct

@efct.pure()
def item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    try:
        return mapping[key]
    except LookupError:
        return 0
"""

    assert _diagnostics(source) == []


def test_frozen_map_lookup_requires_the_declared_key_type() -> None:
    source = """import efct

@efct.pure
def item(mapping: efct.FrozenMap[str, int], key: int) -> int:
    return mapping[key]
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "FrozenMap key type mismatch: expected str, got int"
    )


def test_known_frozen_map_entry_still_uses_conservative_key_error_model() -> None:
    source = """import efct

@efct.pure()
def answer() -> int:
    return efct.FrozenMap((("answer", 42),))["answer"]
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function answer contains undeclared partial behavior raise:builtins.KeyError"
    )


def test_implicit_frozen_map_key_error_propagates_across_calls() -> None:
    source = """import efct

@efct.pure
def item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]

@efct.pure()
def run(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return item(mapping, key)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior raise:builtins.KeyError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call item",
        "Index FrozenMap",
    ]


def test_frozen_map_with_statically_distinct_keys_is_total() -> None:
    source = """import efct

@efct.pure()
def settings() -> efct.FrozenMap[str, int]:
    return efct.FrozenMap((("left", 1), ("right", 2)))
"""

    assert _diagnostics(source) == []


def test_frozen_map_with_static_duplicate_uses_never_and_value_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def duplicate() -> efct.FrozenMap[str, int]:
    return efct.FrozenMap((("same", 1), ("same", 2)))
"""

    assert _diagnostics(source) == []


def test_possible_frozen_map_duplicate_infers_value_error() -> None:
    source = """import efct

@efct.pure
def build(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))

@efct.pure(efct.partial.Raise(ValueError))
def bounded(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))
"""

    assert _diagnostics(source) == []


def test_frozen_map_duplicate_is_rejected_by_explicit_empty_contract() -> None:
    source = """import efct

@efct.pure()
def build(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function build contains undeclared partial behavior raise:builtins.ValueError"
    )
    assert diagnostic["effect_trace"] == [
        {
            "function": "build",
            "filename": "fixture.py",
            "span": {
                "start_line": 5,
                "start_utf8_byte": 11,
                "end_line": 5,
                "end_utf8_byte": 52,
            },
            "operation": "Construct FrozenMap",
        }
    ]


def test_exception_handler_catches_possible_frozen_map_duplicate() -> None:
    source = """import efct

@efct.pure()
def build(first: str, second: str) -> efct.FrozenMap[str, int]:
    try:
        return efct.FrozenMap(((first, 1), (second, 2)))
    except ValueError:
        return efct.FrozenMap(((first, 0),))
"""

    assert _diagnostics(source) == []


def test_frozen_map_duplicate_partiality_propagates_across_calls() -> None:
    source = """import efct

@efct.pure
def build(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))

@efct.pure()
def run(first: str, second: str) -> efct.FrozenMap[str, int]:
    return build(first, second)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function run contains undeclared partial behavior raise:builtins.ValueError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call build",
        "Construct FrozenMap",
    ]


def test_subclass_exception_handler_does_not_catch_parent() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ArithmeticError("arithmetic")
    except ZeroDivisionError:
        return 0
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function recover contains undeclared partial behavior "
        "raise:builtins.ArithmeticError"
    )


def test_handler_covered_by_earlier_parent_is_rejected() -> None:
    source = """import efct

@efct.pure()
def recover(kind: int) -> int:
    try:
        if kind == 0:
            raise ValueError("value")
        raise TypeError("type")
    except Exception:
        return 0
    except ValueError:
        return 1
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "The exception handler type is covered by an earlier handler and is "
        "unreachable"
    )


def test_sibling_exception_handlers_remain_reachable() -> None:
    source = """import efct

@efct.pure()
def recover(kind: int) -> int:
    try:
        if kind == 0:
            raise ValueError("value")
        raise TypeError("type")
    except ValueError:
        return 0
    except TypeError:
        return 1
"""

    assert _diagnostics(source) == []


def test_exception_handler_type_tuple_catches_each_registered_type() -> None:
    source = """import efct

@efct.pure()
def recover(flag: bool) -> str:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except (ValueError, TypeError) as error:
        return str(error)
"""

    assert _diagnostics(source) == []


def test_exception_handler_type_tuple_catches_cross_function_partial_behavior() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject_value() -> None:
    raise ValueError("value")

@efct.pure(efct.partial.Raise(TypeError))
def reject_type() -> None:
    raise TypeError("type")

@efct.pure()
def recover(flag: bool) -> int:
    try:
        if flag:
            reject_value()
        else:
            reject_type()
    except (ValueError, TypeError):
        return 0
    return 1
"""

    assert _diagnostics(source) == []


def test_exception_handler_type_tuple_bare_raise_preserves_concrete_types() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(TypeError),
)
def reject(flag: bool) -> None:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except (ValueError, TypeError):
        raise
"""

    assert _diagnostics(source) == []


def test_later_type_tuple_handler_keeps_members_not_caught_earlier() -> None:
    source = """import efct

@efct.pure()
def recover(flag: bool) -> int:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except ValueError:
        return 1
    except (ValueError, TypeError):
        return 2
"""

    assert _diagnostics(source) == []


def test_empty_exception_handler_type_tuple_is_rejected() -> None:
    source = """import efct

@efct.pure()
def invalid() -> int:
    try:
        raise ValueError("value")
    except ():
        return 0
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == "An exception handler type tuple must not be empty"


def test_duplicate_exception_handler_tuple_type_is_rejected() -> None:
    source = """import efct

@efct.pure()
def invalid() -> int:
    try:
        raise ValueError("value")
    except (ValueError, ValueError):
        return 0
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "Exception handler type builtins.ValueError appears more than once"
    )


def test_inheritance_overlap_in_exception_handler_tuple_is_rejected() -> None:
    source = """import efct

@efct.pure()
def invalid(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except (LookupError, IndexError):
        return 0
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "Exception handler types builtins.LookupError and builtins.IndexError "
        "overlap by inheritance"
    )


def test_type_tuple_handler_fully_covered_by_earlier_handler_is_rejected() -> None:
    source = """import efct

@efct.pure()
def invalid(flag: bool) -> int:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except Exception:
        return 0
    except (ValueError, TypeError):
        return 1
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "The exception handler type is covered by an earlier handler and is "
        "unreachable"
    )


def test_nested_exception_handler_type_tuple_is_rejected() -> None:
    source = """import efct

@efct.pure()
def invalid() -> int:
    try:
        raise ValueError("value")
    except ((ValueError,), TypeError):
        return 0
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "An exception handler must use a registered exception type name"
    )


def test_bare_raise_rethrows_only_the_concrete_exception_caught_by_parent() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_reports_the_exact_rethrown_exception_and_origin() -> None:
    source = """import efct

@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        raise
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function item contains undeclared partial behavior "
        "raise:builtins.IndexError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Re-raise builtins.IndexError"
    ]


def test_bare_raise_preserves_each_possible_caught_exception() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(IndexError),
    efct.partial.Raise(KeyError),
)
def reject(kind: int) -> None:
    try:
        if kind == 0:
            raise IndexError("index")
        raise KeyError("key")
    except LookupError:
        raise
"""

    assert _diagnostics(source) == []


def test_outer_handler_catches_exact_exception_rethrown_by_inner_handler() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        try:
            raise IndexError("index")
        except LookupError:
            raise
    except IndexError:
        return 0
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_unreachable_handler_adds_no_partial_behavior() -> None:
    source = """import efct

@efct.pure()
def value() -> int:
    try:
        return 1
    except ValueError:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_uses_exception_inferred_across_call_boundary() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def fail() -> None:
    raise IndexError("index")

@efct.pure(efct.partial.Raise(IndexError))
def reraised() -> None:
    try:
        fail()
    except LookupError:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_fixed_point_remains_exact_for_recursive_call() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Diverge(),
)
def recurse(value: int) -> None:
    try:
        if value == 0:
            raise ValueError("value")
        recurse(value - 1)
    except Exception:
        raise
"""

    assert _diagnostics(source) == []


def test_nested_handler_restores_outer_exception_for_later_bare_raise() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reraised_outer() -> None:
    try:
        raise ValueError("value")
    except ValueError:
        try:
            raise TypeError("type")
        except TypeError:
            pass
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_preserves_exact_custom_exception_under_builtin_parent() -> None:
    source = """import efct

class ConfigError(ValueError):
    pass

@efct.pure(efct.partial.Raise(ConfigError))
def reject() -> None:
    try:
        raise ConfigError("config")
    except ValueError:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_outside_exception_handler_is_rejected() -> None:
    source = """import efct

@efct.pure
def invalid() -> None:
    raise
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "A bare raise may only appear inside an exception handler"
    )


def test_callers_exception_handler_does_not_authorize_cross_function_bare_raise() -> None:
    source = """import efct

@efct.pure
def reraiser() -> None:
    raise

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    except ValueError:
        reraiser()
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "A bare raise may only appear inside an exception handler"
    )


def test_try_else_uses_locals_from_normal_protected_path() -> None:
    source = """import efct

@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    try:
        value = values[index]
    except IndexError:
        return 0
    else:
        return value
"""

    assert _diagnostics(source) == []


def test_try_else_and_handler_locals_merge_after_the_statement() -> None:
    source = """import efct

@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    try:
        value = values[index]
    except IndexError:
        result = 0
    else:
        result = value
    return result
"""

    assert _diagnostics(source) == []


def test_try_else_uses_local_from_only_normally_completing_if_branch() -> None:
    source = """import efct

@efct.pure()
def value(flag: bool) -> int:
    try:
        if flag:
            return 0
        else:
            result = 1
    except ValueError:
        return 2
    else:
        return result
"""

    assert _diagnostics(source) == []


def test_current_handlers_do_not_catch_partial_behavior_from_try_else() -> None:
    source = """import efct

@efct.pure()
def invalid() -> None:
    try:
        value = 1
    except ValueError:
        pass
    else:
        raise ValueError("else")
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function invalid contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Raise builtins.ValueError"
    ]


def test_outer_handler_can_catch_partial_behavior_from_nested_try_else() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        try:
            value = 1
        except ValueError:
            return 0
        else:
            raise ValueError("else")
    except ValueError:
        return 1
"""

    assert _diagnostics(source) == []


def test_try_else_is_unreachable_after_handled_exception() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ValueError("value")
    except ValueError:
        return 0
    else:
        print("unreachable")
        return 1
"""

    assert _diagnostics(source) == []


def test_try_else_is_unreachable_after_return_or_loop_exit() -> None:
    returned = """import efct

@efct.pure()
def value() -> int:
    try:
        return 1
    except ValueError:
        return 0
    else:
        print("unreachable")
        return 2
"""
    broken = """import efct

@efct.pure()
def value() -> int:
    for item in (1,):
        try:
            break
        except ValueError:
            return item
        else:
            print("unreachable")
    return 0
"""

    assert _diagnostics(returned) == []
    assert _diagnostics(broken) == []


def test_try_else_is_unreachable_when_all_branches_exit_differently() -> None:
    source = """import efct

@efct.pure()
def value(flag: bool) -> int:
    for item in (1,):
        try:
            if flag:
                break
            else:
                continue
        except ValueError:
            return item
        else:
            print("unreachable")
    return 0
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_try_else_requires_an_enclosing_handler() -> None:
    source = """import efct

@efct.pure
def invalid() -> None:
    try:
        value = 1
    except ValueError:
        pass
    else:
        raise
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "A bare raise may only appear inside an exception handler"
    )


def test_try_else_preserves_enclosing_handler_rethrow_context() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reraised_outer() -> None:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            value = 1
        except TypeError:
            pass
        else:
            raise
"""

    assert _diagnostics(source) == []


def test_try_finally_without_handler_preserves_escaping_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    finally:
        pass
"""

    assert _diagnostics(source) == []


def test_finally_runs_after_handled_exception_and_contributes_effects() -> None:
    source = """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def recover() -> int:
    try:
        raise ValueError("value")
    except ValueError:
        return 0
    finally:
        print("cleanup")
"""

    assert _diagnostics(source) == []


def test_finally_raise_overrides_original_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject() -> None:
    try:
        raise ValueError("value")
    finally:
        raise TypeError("cleanup")
"""

    assert _diagnostics(source) == []


def test_outer_handler_catches_exception_raised_by_finally() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        try:
            return 1
        finally:
            raise ValueError("cleanup")
    except ValueError:
        return 0
"""

    assert _diagnostics(source) == []


def test_finally_return_suppresses_original_exception() -> None:
    source = """import efct

@efct.pure()
def value() -> int:
    try:
        raise ValueError("value")
    finally:
        return 1
"""

    assert _diagnostics(source) == []


def test_conditional_finally_raise_preserves_original_fallthrough_exception() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(TypeError),
)
def reject(flag: bool) -> None:
    try:
        raise ValueError("value")
    finally:
        if flag:
            raise TypeError("cleanup")
"""

    assert _diagnostics(source) == []


def test_non_fallthrough_finally_preserves_prior_external_call_effect() -> None:
    source = """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def operation() -> None:
    print("operation")
    raise ValueError("value")

@efct.effects(
    efct.effect.Console(),
)
def recover() -> int:
    try:
        operation()
    finally:
        return 0
"""

    assert _diagnostics(source) == []


def test_non_fallthrough_finally_suppresses_recursive_exceptional_call() -> None:
    source = """import efct

@efct.pure(efct.partial.Diverge())
def recurse(value: int) -> int:
    try:
        if value == 0:
            raise ValueError("value")
        return recurse(value - 1)
    finally:
        return 0
"""

    assert _diagnostics(source) == []


def test_non_fallthrough_finally_rejects_unresolved_effect_variable() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    try:
        return function(value)
    finally:
        return 0
"""

    diagnostic = _diagnostic(source, "P1201")
    assert diagnostic["message"] == (
        "A non-fallthrough finally cannot override an unresolved effect variable"
    )


def test_fallthrough_finally_preserves_unresolved_effect_variable() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    try:
        return function(value)
    finally:
        pass
"""

    assert _diagnostics(source) == []


def test_finally_can_use_preexisting_local_and_preserves_normal_locals() -> None:
    source = """import efct

@efct.pure()
def value(base: int) -> int:
    try:
        result = base + 1
    finally:
        cleanup = base + 2
    return result + cleanup
"""

    assert _diagnostics(source) == []


def test_finally_assignment_must_match_normal_path_type() -> None:
    source = """import efct

@efct.pure()
def invalid() -> int:
    try:
        value = 1
    finally:
        value = "wrong"
    return value
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "Finally assigns local value with a type incompatible with the normal path"
    )


def test_finally_cannot_read_local_created_only_in_protected_region() -> None:
    source = """import efct

@efct.pure()
def invalid() -> int:
    try:
        value = 1
    finally:
        cleanup = value + 1
    return cleanup
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "Local variable value is not available in finally because it was not "
        "defined before try"
    )


def test_bare_raise_in_finally_reraises_pending_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_finally_without_current_exception_raises_runtime_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(RuntimeError))
def reject() -> None:
    try:
        pass
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_finally_combines_pending_and_missing_exception_paths() -> None:
    source = """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(RuntimeError),
)
def reject(flag: bool) -> None:
    try:
        if flag:
            raise ValueError("value")
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_finally_uses_enclosing_handler_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            pass
        finally:
            raise
"""

    assert _diagnostics(source) == []


def test_pending_exception_in_finally_precedes_enclosing_handler_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject() -> None:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            raise TypeError("inner")
        finally:
            raise
"""

    assert _diagnostics(source) == []


def test_handled_exception_does_not_remain_current_for_same_finally() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(RuntimeError))
def reject() -> None:
    try:
        raise ValueError("handled")
    except ValueError:
        pass
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_finally_rethrow_preserves_partial_behavior_from_called_function() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def operation() -> None:
    raise ValueError("value")

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(RuntimeError),
)
def reject() -> None:
    try:
        operation()
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_finally_rethrow_does_not_override_unresolved_effect_variable() -> None:
    source = """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    try:
        return function(value)
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_caught_finally_rethrow_resumes_original_pending_exception() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    finally:
        try:
            raise
        except ValueError:
            pass
"""

    assert _diagnostics(source) == []


def test_non_fallthrough_finally_rethrow_path_preserves_original_exception() -> None:
    source = """import efct

@efct.pure()
def invalid(flag: bool) -> int:
    try:
        raise ValueError("value")
    finally:
        if flag:
            raise
        else:
            return 0
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function invalid contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_finally_loop_exit_overrides_protected_loop_exit() -> None:
    source = """import efct

@efct.pure()
def value() -> int:
    for item in (1,):
        try:
            break
        finally:
            continue
        print("unreachable")
    return 0
"""

    assert _diagnostics(source) == []


def test_try_except_else_finally_composes_all_paths() -> None:
    source = """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def item(values: tuple[int, ...], index: int) -> int:
    try:
        value = values[index]
    except IndexError:
        return 0
    else:
        return value
    finally:
        print("cleanup")
"""

    assert _diagnostics(source) == []


def test_simple_custom_exception_can_be_declared_raised_and_caught() -> None:
    source = '''import efct

class ConfigError(ValueError):
    """Invalid application configuration."""
    pass

@efct.pure(efct.partial.Raise(ConfigError))
def reject(message: str) -> None:
    raise ConfigError(message)

@efct.pure()
def recover(message: str) -> int:
    try:
        reject(message)
    except ConfigError:
        return 0
    return 1
'''

    assert _diagnostics(source) == []


def test_builtin_parent_declaration_and_handler_cover_custom_exception() -> None:
    source = """import efct

class ConfigError(ValueError):
    pass

@efct.pure(efct.partial.Raise(ValueError))
def reject(message: str) -> None:
    raise ConfigError(message)

@efct.pure()
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        return 0
    return 1
"""

    assert _diagnostics(source) == []


def test_custom_exception_declaration_does_not_cover_builtin_parent() -> None:
    source = """import efct

class ConfigError(ValueError):
    pass

@efct.pure(efct.partial.Raise(ConfigError))
def reject(message: str) -> None:
    raise ValueError(message)
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function reject contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_custom_exception_must_have_a_registered_single_base() -> None:
    unregistered = """import efct

class Stop(int):
    pass
"""
    multiple = """import efct

class ConfigError(ValueError, TypeError):
    pass
"""

    diagnostic = _diagnostic(unregistered, "P1201")
    assert diagnostic["message"] == "Exception base int is not registered"
    diagnostic = _diagnostic(multiple, "P1201")
    assert diagnostic["message"] == (
        "An exception class requires one base class and no decorators or "
        "metaclass arguments"
    )


def test_custom_exception_body_rejects_runtime_behavior() -> None:
    source = """import efct

class ConfigError(ValueError):
    def describe(self) -> str:
        return "bad"
"""

    diagnostic = _diagnostic(source, "P1201")
    assert diagnostic["message"] == (
        "An exception class body may only contain a docstring and pass"
    )


def test_custom_exception_base_must_be_defined_first() -> None:
    source = """import efct

class ChildError(ParentError):
    pass

class ParentError(ValueError):
    pass
"""

    diagnostic = _diagnostic(source, "P1201")
    assert diagnostic["message"] == (
        "Exception base ParentError must be defined before subclass ChildError"
    )


def test_custom_exception_string_declaration_requires_full_name() -> None:
    source = """import efct

class ConfigError(ValueError):
    pass

@efct.pure("raise:ConfigError")
def reject(message: str) -> None:
    raise ConfigError(message)
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == (
        "A string exception declaration must use a fully qualified registered name"
    )


def test_assert_with_unknown_condition_requires_assertion_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def require(condition: bool) -> None:
    assert condition
"""

    assert _diagnostics(source) == []


def test_assertion_error_is_rejected_by_explicit_empty_partial_contract() -> None:
    source = """import efct

@efct.pure()
def invalid(condition: bool) -> None:
    assert condition
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function invalid contains undeclared partial behavior "
        "raise:builtins.AssertionError"
    )
    effect_trace = diagnostic["effect_trace"]
    assert isinstance(effect_trace, list)
    assert len(effect_trace) == 1
    frame = effect_trace[0]
    assert isinstance(frame, dict)
    assert frame["operation"] == "Assert condition"


def test_statically_true_assert_is_total_and_does_not_evaluate_message() -> None:
    source = """import efct

@efct.pure()
def valid() -> int:
    assert True, print("unreachable")
    return 1
"""

    assert _diagnostics(source) == []


def test_negated_false_assert_is_statically_true() -> None:
    source = """import efct

@efct.pure()
def valid() -> int:
    assert not False
    return 1
"""

    assert _diagnostics(source) == []


def test_statically_false_assert_uses_never_control_flow() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def invalid() -> int:
    assert False
    print("unreachable")
"""

    assert _diagnostics(source) == []


def test_assert_condition_must_be_exact_bool() -> None:
    source = """import efct

@efct.pure
def invalid(value: int) -> None:
    assert value
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == "An assert condition must be an exact bool"


def test_assert_message_effects_only_belong_to_failure_path() -> None:
    source = """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
    efct.partial.Raise(AssertionError),
)
def require(condition: bool) -> None:
    assert condition, print("required")
"""

    assert _diagnostics(source) == []


def test_assert_message_cannot_escape_a_local_mutable_value() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def invalid(condition: bool) -> None:
    values = [1]
    assert condition, values
"""

    diagnostic = _diagnostic(source, "P1202")
    assert diagnostic["message"] == (
        "An assert message must be a supported immutable data value"
    )


def test_assert_message_failure_precedes_assertion_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def invalid() -> None:
    assert False, (1,)[2]
"""

    assert _diagnostics(source) == []


def test_conditional_assert_message_failure_preserves_normal_path() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def require(condition: bool) -> int:
    assert condition, (1,)[2]
    return 1
"""

    assert _diagnostics(source) == []


def test_assert_condition_failure_skips_message() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(IndexError))
def invalid() -> None:
    assert (1,)[2] == 0, print("unreachable")
"""

    assert _diagnostics(source) == []


def test_exception_handler_catches_assertion_error() -> None:
    source = """import efct

@efct.pure()
def require(condition: bool) -> int:
    try:
        assert condition
    except AssertionError:
        return 0
    return 1
"""

    assert _diagnostics(source) == []


def test_finally_bare_raise_reraises_assertion_error() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def invalid() -> None:
    try:
        assert False
    finally:
        raise
"""

    assert _diagnostics(source) == []
