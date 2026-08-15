import ast
import json
from typing import Any

import pytest
from efct import _core
from efct.frontend import (
    analyze_prepared_runtime,
    encode_source,
    inspect_prepared_imports,
    prepare_module,
)


def _encode(source: str) -> dict[str, Any]:
    return json.loads(encode_source(source.encode("utf-8"), "fixture.py"))


def test_native_frontend_does_not_call_python_ast_parse(monkeypatch) -> None:
    def rejected(*args: object, **kwargs: object) -> object:
        raise AssertionError("Python ast.parse must not be called")

    monkeypatch.setattr(ast, "parse", rejected)

    encoded = _encode("value: int = 1\n")
    root = encoded["language"]["root"]
    assert root["items"][0]["kind"] == "annotated_assignment"


def test_encodes_complete_pure_function_structure() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def add(x: int, y: int) -> int:
    return x + y
"""
    )

    root = encoded["language"]["root"]
    assert isinstance(root, dict)
    items = root["items"]
    assert isinstance(items, list)
    assert [item["kind"] for item in items] == ["import", "function"]
    function = items[1]
    assert function["name"] == "add"
    assert function["body"][0]["value"]["operator"] == "add"


def test_encodes_integer_floor_division_and_modulo_operators() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def calculate(x: int, y: int) -> int:
    x //= y
    return x % y
"""
    )

    function = encoded["language"]["root"]["items"][1]
    assert function["body"][0]["operator"] == "floor_divide"
    assert function["body"][1]["value"]["operator"] == "modulo"


def test_encodes_runtime_tuple_subscript_with_negative_index() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def last(values: tuple[int, str]) -> str:
    return values[-1]
"""
    )

    value = encoded["language"]["root"]["items"][1]["body"][0]["value"]
    assert value["kind"] == "subscript"
    assert value["value"]["identifier"] == "values"
    assert value["slice"]["operator"] == "negative"
    assert value["slice"]["operand"]["value"] == {"kind": "int", "value": "1"}


def test_module_contract_assignment_enters_closed_protocol() -> None:
    encoded = _encode(
        """import efct

_efct = efct.effects("console")
print("ready")
"""
    )

    items = encoded["language"]["root"]["items"]
    assert [item["kind"] for item in items] == ["import", "statement", "statement"]
    assignment = items[1]["statement"]
    assert assignment["kind"] == "assign"
    assert assignment["targets"][0]["identifier"] == "_efct"
    assert assignment["value"]["kind"] == "call"


def test_column_locations_preserve_utf8_byte_offsets() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def identity(value: int) -> int:
    return value
"""
    )

    root = encoded["language"]["root"]
    function = root["items"][1]
    returned_name = function["body"][0]["value"]
    assert returned_name["span"]["start_utf8_byte"] == 11


def test_list_expression_enters_closed_protocol() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def bad() -> tuple[int, ...]:
    return [1]
"""
    )

    root = encoded["language"]["root"]
    value = root["items"][1]["body"][0]["value"]
    assert value["kind"] == "list"
    assert value["elements"][0]["value"] == {"kind": "int", "value": "1"}


def test_result_match_enters_closed_protocol() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def unwrap(result: efct.Result[int, str]) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Err(_):
            return 0
"""
    )

    function = encoded["language"]["root"]["items"][1]
    statement = function["body"][0]
    assert statement["kind"] == "match"
    assert statement["subject"]["identifier"] == "result"
    assert [case["pattern"]["class"]["name"] for case in statement["cases"]] == [
        "Ok",
        "Err",
    ]
    assert statement["cases"][0]["pattern"]["positional"][0]["kind"] == "capture"
    assert statement["cases"][1]["pattern"]["positional"][0]["kind"] == "wildcard"


def test_type_ignore_enters_protocol() -> None:
    encoded = _encode("value = unknown  # type: ignore[name-defined]\n")

    root = encoded["language"]["root"]
    assert root["items"][-1]["kind"] == "type_ignore"
    assert root["items"][-1]["tag"] == "[name-defined]"


def test_recognized_but_disabled_typescript_language_is_rejected() -> None:
    payload = json.dumps(
        {
            "protocol_version": 1,
            "filename": "app.ts",
            "source_sha256": "a" * 64,
            "language": {
                "kind": "type_script",
                "compiler_version": "6.0.0",
            },
        },
        separators=(",", ":"),
    ).encode()

    diagnostics = json.loads(_core.check_ast(payload))

    assert diagnostics[0]["code"] == "P0002"


def test_exception_handlers_use_tagged_union_encoding() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
"""
    )

    root = encoded["language"]["root"]
    statement = root["items"][1]["body"][0]
    assert statement["kind"] == "try"
    assert statement["handlers"][0]["kind"] == "typed"
    assert statement["handlers"][0]["exception"]["identifier"] == "ValueError"

    bound = _encode(
        """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError as error:
        return 0
"""
    )
    bound_handler = bound["language"]["root"]["items"][1]["body"][0]["handlers"][0]
    assert bound_handler["kind"] == "typed_binding"
    assert bound_handler["binding"] == "error"


def test_except_star_uses_distinct_try_star_protocol_variant() -> None:
    encoded = _encode(
        """import efct

@efct.pure()
def recover() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except* ValueError as errors:
        pass
"""
    )

    statement = encoded["language"]["root"]["items"][1]["body"][0]
    assert statement["kind"] == "try_star"
    assert statement["handlers"][0]["kind"] == "typed_binding"
    assert statement["handlers"][0]["exception"]["identifier"] == "ValueError"
    assert statement["handlers"][0]["binding"] == "errors"


def test_assert_enters_closed_protocol_with_optional_message() -> None:
    encoded = _encode(
        """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def require(value: bool) -> None:
    assert value, "required"
"""
    )

    statement = encoded["language"]["root"]["items"][1]["body"][0]
    assert statement["kind"] == "assert"
    assert statement["condition"]["identifier"] == "value"
    assert statement["message"]["value"] == {
        "kind": "str",
        "value": "required",
    }


def test_with_items_enter_closed_protocol() -> None:
    encoded = _encode(
        """import contextlib
import efct

@efct.pure()
def recover() -> None:
    with contextlib.suppress(ValueError):
        pass
    with contextlib.suppress(ValueError) as marker:
        return marker
"""
    )

    statements = encoded["language"]["root"]["items"][2]["body"]
    assert [statement["kind"] for statement in statements] == ["with", "with"]
    assert statements[0]["items"][0]["kind"] == "unbound"
    assert statements[1]["items"][0]["kind"] == "bound"
    assert statements[1]["items"][0]["context"]["kind"] == "call"
    assert statements[1]["items"][0]["target"]["identifier"] == "marker"


def test_exception_class_body_uses_explicit_protocol_variants() -> None:
    encoded = _encode(
        '''class ConfigError(ValueError):
    """Invalid application configuration."""
    pass
'''
    )

    exception = encoded["language"]["root"]["items"][0]
    assert exception["kind"] == "class"
    assert exception["bases"][0]["identifier"] == "ValueError"
    assert [item["kind"] for item in exception["body"]] == ["docstring", "pass"]


def test_pure_callable_parameter_list_is_encoded_explicitly() -> None:
    encoded = _encode(
        """import efct

@efct.pure
def apply(function: efct.PureCallable[[int, str], int], value: int) -> int:
    return function(value, "")
"""
    )

    root = encoded["language"]["root"]
    annotation = root["items"][1]["parameters"]["positional"][0]["annotation"]
    assert annotation["slice"]["elements"][0]["kind"] == "list"


def test_effect_generic_parameter_enters_closed_protocol() -> None:
    encoded = _encode(
        """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)
"""
    )

    root = encoded["language"]["root"]
    parameter = root["items"][1]["type_parameters"][0]
    assert parameter["kind"] == "type_variable"
    assert parameter["name"] == "E"
    assert parameter["bound"]["kind"] == "attribute"
    assert parameter["has_default"] is False


def test_rust_builds_all_runtime_plans_from_one_prepared_module() -> None:
    source = b"""import efct
import time

LIMIT: int = 3

@efct.effects("clock")
def sample(value: int) -> tuple[int, int]:
    return (value + LIMIT, time.time_ns())

@efct.pure
def increment(value: int) -> int:
    return value + LIMIT
"""
    prepared, _ = prepare_module(source, "fixture.py")

    imports = json.loads(inspect_prepared_imports(prepared))
    analysis = json.loads(analyze_prepared_runtime(prepared))
    plan = analysis["plans"]["sample"]

    assert analysis["diagnostics"] == []
    assert sorted(analysis["plans"]) == ["increment", "sample"]
    assert plan["callable_kind"] == "bounded_effect"
    assert plan["declared_effects"] == ["clock"]
    assert plan["parameter_types"] == [{"kind": "scalar", "name": "int"}]
    assert plan["constant_types"] == [
        {
            "name": "LIMIT",
            "value_type": {"kind": "scalar", "name": "int"},
        }
    ]
    assert plan["module_members"] == [
        {"binding": "time", "module": "time", "members": ["time_ns"]}
    ]
    assert imports == ["efct", "time"]

    with pytest.raises(ValueError, match="already been consumed"):
        inspect_prepared_imports(prepared)


def test_runtime_plans_preserve_all_pure_partial_contract_states() -> None:
    source = b"""import efct

@efct.pure
def inferred(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

@efct.pure()
def exact(value: int) -> int:
    return value

@efct.pure(efct.partial.Raise(ValueError))
def bounded(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
"""
    prepared, _ = prepare_module(source, "fixture.py")

    analysis = json.loads(analyze_prepared_runtime(prepared))

    assert analysis["diagnostics"] == []
    assert analysis["plans"]["inferred"]["callable_kind"] == "inferred_pure"
    assert analysis["plans"]["inferred"]["declared_effects"] == []
    assert analysis["plans"]["exact"]["callable_kind"] == "bounded_pure"
    assert analysis["plans"]["exact"]["declared_effects"] == []
    assert analysis["plans"]["bounded"]["callable_kind"] == "bounded_pure"
    assert analysis["plans"]["bounded"]["declared_effects"] == [
        "raise:builtins.ValueError"
    ]
