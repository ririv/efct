import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _diagnostic(source: str, code: str) -> dict[str, object]:
    return next(item for item in _diagnostics(source) if item["code"] == code)


def test_except_star_handlers_can_consume_all_group_leaves() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
    except* TypeError:
        pass
    return 1
"""

    assert _diagnostics(source) == []


def test_unmatched_group_leaf_requires_group_partial_declaration() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(TypeError))
def reject() -> None:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
"""

    assert _diagnostics(source) == []


def test_naked_exception_is_consumed_by_except_star() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ValueError("value")
    except* ValueError:
        pass
    return 1
"""

    assert _diagnostics(source) == []


def test_bare_raise_in_except_star_reraises_a_group() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    except* ValueError:
        raise
"""

    assert _diagnostics(source) == []


def test_bound_except_star_group_can_be_rendered_and_reraised() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    try:
        raise ValueError("value")
    except* ValueError as errors:
        str(errors)
        raise errors
"""

    assert _diagnostics(source) == []


def test_nested_exception_groups_are_flattened_for_partial_analysis() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(KeyError))
def reject() -> None:
    try:
        raise ExceptionGroup(
            "outer",
            (
                ValueError("value"),
                ExceptionGroup(
                    "inner",
                    (TypeError("type"), KeyError("key")),
                ),
            ),
        )
    except* (ValueError, TypeError):
        pass
"""

    assert _diagnostics(source) == []


def test_traditional_exception_group_handler_catches_the_whole_group() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except ExceptionGroup:
        return 1
"""

    assert _diagnostics(source) == []


def test_traditional_exception_parent_handler_catches_exception_group() -> None:
    source = """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except Exception:
        return 1
"""

    assert _diagnostics(source) == []


def test_traditional_exception_group_handler_bare_raise_preserves_group() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except ExceptionGroup:
        raise
"""

    assert _diagnostics(source) == []


def test_try_star_else_only_runs_after_normal_try_completion() -> None:
    source = """import efct

@efct.pure()
def recover(raise_group: bool) -> int:
    try:
        if raise_group:
            raise ExceptionGroup("errors", (ValueError("value"),))
    except* ValueError:
        pass
    else:
        return 2
    return 1
"""

    assert _diagnostics(source) == []


def test_finally_can_override_pending_exception_group() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    finally:
        raise TypeError("cleanup")
"""

    assert _diagnostics(source) == []


def test_finally_bare_raise_preserves_pending_exception_group() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    finally:
        raise
"""

    assert _diagnostics(source) == []


def test_except_star_handles_group_partial_across_function_call() -> None:
    source = """import efct

@efct.pure(
    efct.partial.RaiseGroup(ValueError),
    efct.partial.RaiseGroup(TypeError),
)
def reject() -> None:
    raise ExceptionGroup(
        "errors",
        (ValueError("value"), TypeError("type")),
    )

@efct.pure()
def recover() -> int:
    try:
        reject()
    except* (ValueError, TypeError):
        pass
    return 1
"""

    assert _diagnostics(source) == []


def test_exception_group_can_be_suppressed_as_a_whole() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(ExceptionGroup):
        raise ExceptionGroup("errors", (ValueError("value"),))
    return 1
"""

    assert _diagnostics(source) == []


def test_exception_parent_suppresses_exception_group_as_a_whole() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(Exception):
        raise ExceptionGroup("errors", (ValueError("value"),))
    return 1
"""

    assert _diagnostics(source) == []


def test_suppressing_leaf_type_does_not_split_exception_group() -> None:
    source = """import contextlib
import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    with contextlib.suppress(ValueError):
        raise ExceptionGroup("errors", (ValueError("value"),))
"""

    assert _diagnostics(source) == []


def test_group_partial_is_not_covered_by_naked_raise_declaration() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(ValueError))
def invalid() -> None:
    raise ExceptionGroup("errors", (ValueError("value"),))
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function invalid contains undeclared partial behavior "
        "raise-group:builtins.ValueError"
    )
    assert diagnostic["suggestion"] == (
        "Declare @efct.pure(efct.partial.RaiseGroup(...)) or remove the partial "
        "operation raise-group:builtins.ValueError"
    )


def test_group_partial_declaration_uses_exception_inheritance() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(Exception))
def reject() -> None:
    raise ExceptionGroup("errors", (ValueError("value"),))
"""

    assert _diagnostics(source) == []


def test_group_partial_declaration_does_not_cover_naked_raise() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(Exception))
def invalid() -> None:
    raise ValueError("value")
"""

    diagnostic = _diagnostic(source, "P1001")
    assert diagnostic["message"] == (
        "Function invalid contains undeclared partial behavior "
        "raise:builtins.ValueError"
    )


def test_except_leaf_type_does_not_catch_group_leaf() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def reject() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except ValueError:
        pass
"""

    assert _diagnostics(source) == []


def test_except_star_cannot_match_exception_group_wrapper() -> None:
    source = """import efct

@efct.pure()
def invalid() -> None:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except* ExceptionGroup:
        pass
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "An except* handler cannot match ExceptionGroup directly; match its leaf "
        "exception types"
    )


def test_exception_group_requires_nonempty_static_tuple() -> None:
    source = """import efct

@efct.pure()
def invalid() -> None:
    raise ExceptionGroup("errors", ())
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "ExceptionGroup requires a str message and a non-empty tuple of "
        "registered exceptions"
    )


def test_exception_group_rejects_non_exception_child() -> None:
    source = """import efct

@efct.pure()
def invalid() -> None:
    raise ExceptionGroup("errors", (1,))
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "ExceptionGroup children must be registered exception instances or "
        "ExceptionGroup values"
    )


def test_except_star_handler_rejects_new_partial_behavior() -> None:
    source = """import efct

@efct.pure(efct.partial.Raise(TypeError))
def invalid() -> None:
    try:
        raise ValueError("value")
    except* ValueError:
        raise TypeError("new")
"""

    diagnostic = _diagnostic(source, "P1201")
    assert diagnostic["message"] == (
        "An except* handler may only handle normally or re-raise its matched "
        "subgroup; raising new partial behavior is not supported"
    )


def test_custom_exception_cannot_inherit_exception_group() -> None:
    source = """import efct

class InvalidGroup(ExceptionGroup):
    pass

@efct.pure()
def value() -> int:
    return 1
"""

    diagnostic = _diagnostic(source, "P1201")
    assert diagnostic["message"] == (
        "Custom exceptions cannot inherit from ExceptionGroup"
    )


def test_exception_group_declaration_names_leaf_type() -> None:
    source = """import efct

@efct.pure(efct.partial.RaiseGroup(ExceptionGroup))
def invalid() -> None:
    pass
"""

    diagnostic = _diagnostic(source, "P1006")
    assert diagnostic["message"] == (
        "ExceptionGroup declarations must name their registered leaf exception "
        "types"
    )
