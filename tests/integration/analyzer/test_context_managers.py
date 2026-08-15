import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _diagnostic(source: str, code: str) -> dict[str, object]:
    return next(item for item in _diagnostics(source) if item["code"] == code)


def test_contextlib_suppress_removes_matching_partial_behavior() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(ValueError):
        raise ValueError("value")
    return 1
"""

    assert _diagnostics(source) == []


def test_imported_suppress_symbol_is_supported() -> None:
    source = """from contextlib import suppress
import efct

@efct.pure()
def recover() -> int:
    with suppress(ValueError):
        raise ValueError("value")
    return 1
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_uses_registered_exception_hierarchy() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(LookupError):
        raise IndexError("index")
    return 1
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_supports_registered_custom_exceptions() -> None:
    source = """import contextlib
import efct

class ConfigError(ValueError):
    pass

@efct.pure()
def recover() -> int:
    with contextlib.suppress(ConfigError):
        raise ConfigError("config")
    return 1
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_preserves_unmatched_partial_behavior() -> None:
    source = """import contextlib
import efct

@efct.pure(efct.partial.Raise(TypeError))
def reject() -> None:
    with contextlib.suppress(ValueError):
        raise TypeError("type")
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_preserves_external_effects() -> None:
    source = """import contextlib
import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
)
def recover() -> None:
    with contextlib.suppress(ValueError):
        print("before")
        raise ValueError("value")
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_handles_cross_function_partial_behavior() -> None:
    source = """import contextlib
import efct

@efct.pure
def reject() -> None:
    raise ValueError("value")

@efct.pure()
def recover() -> None:
    with contextlib.suppress(ValueError):
        reject()
"""

    assert _diagnostics(source) == []


def test_multiple_suppress_managers_compose() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover(kind: bool) -> int:
    with contextlib.suppress(ValueError), contextlib.suppress(TypeError):
        if kind:
            raise ValueError("value")
        raise TypeError("type")
    return 1
"""

    assert _diagnostics(source) == []


def test_suppress_target_is_none_and_survives_normal_exit() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> None:
    with contextlib.suppress(ValueError) as marker:
        raise ValueError("value")
    return marker
"""

    assert _diagnostics(source) == []


def test_normal_with_body_locals_survive_when_suppression_is_unreachable() -> None:
    source = """import contextlib
import efct

@efct.pure()
def value() -> int:
    with contextlib.suppress(ValueError):
        result = 1
    return result
"""

    assert _diagnostics(source) == []


def test_possible_suppression_does_not_leak_body_only_locals() -> None:
    source = """import contextlib
import efct

@efct.pure()
def invalid(flag: bool) -> int:
    with contextlib.suppress(ValueError):
        if flag:
            raise ValueError("value")
        result = 1
    return result
"""

    diagnostic = _diagnostic(source, "P1004")
    assert diagnostic["message"] == "Value name result cannot be resolved"


def test_suppress_constructor_failure_occurs_before_body_protection() -> None:
    source = """import contextlib
import efct

@efct.pure(efct.partial.Raise(IndexError))
def reject() -> None:
    with contextlib.suppress(()[0]):
        raise IndexError("body")
"""

    assert _diagnostics(source) == []


def test_outer_suppress_handles_later_context_construction_failure() -> None:
    source = """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(IndexError), contextlib.suppress(()[0]):
        raise ValueError("unreachable")
    return 1
"""

    assert _diagnostics(source) == []


def test_contextlib_suppress_requires_at_least_one_exception_type() -> None:
    source = """import contextlib
import efct

@efct.pure()
def invalid() -> None:
    with contextlib.suppress():
        pass
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "contextlib.suppress requires at least one registered exception type"
    )


def test_contextlib_suppress_rejects_duplicate_or_overlapping_types() -> None:
    duplicate = """import contextlib
import efct

@efct.pure()
def invalid() -> None:
    with contextlib.suppress(ValueError, ValueError):
        pass
"""
    overlapping = """import contextlib
import efct

@efct.pure()
def invalid() -> None:
    with contextlib.suppress(Exception, ValueError):
        pass
"""

    duplicate_diagnostic = _diagnostic(duplicate, "P1104")
    overlap_diagnostic = _diagnostic(overlapping, "P1104")
    assert duplicate_diagnostic["message"] == (
        "contextlib.suppress exception types must not overlap"
    )
    assert overlap_diagnostic["message"] == (
        "contextlib.suppress exception types must not overlap"
    )


def test_contextlib_suppress_rejects_non_exception_arguments() -> None:
    source = """import contextlib
import efct

@efct.pure()
def invalid() -> None:
    with contextlib.suppress(1):
        pass
"""

    diagnostic = _diagnostic(source, "P1104")
    assert diagnostic["message"] == (
        "contextlib.suppress arguments must be registered exception type names"
    )


def test_arbitrary_context_managers_remain_rejected() -> None:
    source = """import efct

@efct.effects(efct.effect.File.Read())
def invalid(path: str) -> None:
    with open(path):
        pass
"""

    diagnostic = _diagnostic(source, "P1401")
    assert diagnostic["message"] == (
        "Only contextlib.suppress is supported as a context manager"
    )
