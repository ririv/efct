import json

from efct import _core
from efct.frontend import encode_source


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "fixture.py")
    return json.loads(_core.check_ast(encoded))


def _partial_messages(source: str) -> list[str]:
    return [
        str(diagnostic["message"])
        for diagnostic in _diagnostics(source)
        if diagnostic["code"] == "P1001"
        and "partial behavior" in str(diagnostic["message"])
    ]


def test_randint_requires_value_error_in_the_contract() -> None:
    source = """import efct
import random

@efct.effects("random")
def sample(low: int, high: int) -> int:
    return random.randint(low, high)
"""

    assert _partial_messages(source) == [
        "Function sample contains undeclared partial behavior raise:builtins.ValueError"
    ]


def test_randint_value_error_can_be_handled() -> None:
    source = """import efct
import random

@efct.effects("random")
def sample(low: int, high: int) -> int:
    try:
        return random.randint(low, high)
    except ValueError:
        return low
"""

    assert _diagnostics(source) == []


def test_urandom_requires_divergence_in_the_complete_contract() -> None:
    source = """import efct
import os

@efct.effects(
    "random",
    "raise:builtins.NotImplementedError",
    "raise:builtins.OSError",
    "raise:builtins.ValueError",
)
def random_bytes(size: int) -> bytes:
    return os.urandom(size)
"""

    assert _partial_messages(source) == [
        "Function random_bytes contains undeclared partial behavior diverge"
    ]


def test_print_requires_operational_failure_partial_behaviors() -> None:
    source = """import efct

@efct.effects("console")
def show(value: int) -> None:
    print(value)
"""

    assert _partial_messages(source) == [
        "Function show contains undeclared partial behavior raise:builtins.OSError",
        "Function show contains undeclared partial behavior raise:builtins.ValueError",
    ]


def test_os_error_handler_covers_the_file_operation_family() -> None:
    source = """import efct
import os

@efct.effects("file.read", "raise:builtins.ValueError")
def scan(path: str) -> None:
    try:
        os.listdir(path)
    except OSError:
        pass
"""

    assert _diagnostics(source) == []
