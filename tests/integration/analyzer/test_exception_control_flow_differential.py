from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest
from efct import _core
from efct.frontend import encode_source

_RUN_PROBE = """\
import ast
import importlib
import sys

module = importlib.import_module("case_module")
arguments = ast.literal_eval(sys.argv[1])
try:
    result = module.probe(*arguments)
except BaseException as error:
    exception_name = f"{type(error).__module__}.{type(error).__qualname__}"
    print(f"exception|{exception_name}|{error}")
else:
    print(f"return|{result!r}")
"""


def _diagnostics(source: str) -> list[dict[str, object]]:
    encoded = encode_source(source.encode("utf-8"), "case_module.py")
    return json.loads(_core.check_ast(encoded))


def _run_probe(
    tmp_path: Path,
    source: str,
    arguments: tuple[object, ...],
) -> subprocess.CompletedProcess[str]:
    (tmp_path / "case_module.py").write_text(source, encoding="utf-8")
    environment = os.environ.copy()
    existing_path = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = (
        str(tmp_path)
        if not existing_path
        else f"{tmp_path}{os.pathsep}{existing_path}"
    )
    return subprocess.run(
        [sys.executable, "-c", _RUN_PROBE, repr(arguments)],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )


@pytest.mark.parametrize(
    ("source", "arguments", "expected"),
    [
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(ValueError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        raise
""",
            (),
            "exception|builtins.ValueError|body",
            id="pending-body-exception",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(RuntimeError))
def probe() -> None:
    try:
        raise ValueError("handled")
    except ValueError:
        pass
    finally:
        raise
""",
            (),
            "exception|builtins.RuntimeError|No active exception to reraise",
            id="handled-exception-is-not-current-in-finally",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(ValueError))
def probe() -> int:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            return 1
        finally:
            raise
""",
            (),
            "exception|builtins.ValueError|outer",
            id="enclosing-handler-exception",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(TypeError))
def probe() -> None:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            raise TypeError("inner")
        finally:
            raise
""",
            (),
            "exception|builtins.TypeError|inner",
            id="pending-exception-precedes-enclosing-handler",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(TypeError))
def probe() -> None:
    try:
        raise ValueError("body")
    except ValueError:
        raise TypeError("handler")
    finally:
        raise
""",
            (),
            "exception|builtins.TypeError|handler",
            id="exception-from-handler",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(TypeError))
def probe() -> None:
    try:
        pass
    except ValueError:
        pass
    else:
        raise TypeError("else")
    finally:
        raise
""",
            (),
            "exception|builtins.TypeError|else",
            id="exception-from-else",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(TypeError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        raise TypeError("cleanup")
""",
            (),
            "exception|builtins.TypeError|cleanup",
            id="explicit-finally-exception-overrides",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe() -> int:
    try:
        raise ValueError("body")
    finally:
        return 7
""",
            (),
            "return|7",
            id="finally-return-overrides",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        assert False, "cleanup"
""",
            (),
            "exception|builtins.AssertionError|cleanup",
            id="false-assertion-overrides",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(ValueError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        assert True, "unreachable"
""",
            (),
            "exception|builtins.ValueError|body",
            id="true-assertion-preserves",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(IndexError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        assert False, ("message",)[1]
""",
            (),
            "exception|builtins.IndexError|tuple index out of range",
            id="assertion-message-fails-first",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(IndexError))
def probe() -> int:
    try:
        raise ValueError("body")
    finally:
        return (1,)[1]
""",
            (),
            "exception|builtins.IndexError|tuple index out of range",
            id="return-expression-fails-before-return",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(ValueError))
def probe() -> None:
    try:
        raise ValueError("body")
    finally:
        try:
            raise
        except ValueError:
            pass
""",
            (),
            "exception|builtins.ValueError|body",
            id="caught-finally-reraise-resumes-original",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.Raise(ValueError))
def operation(flag: bool) -> None:
    if flag:
        raise ValueError("operation")

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(RuntimeError),
)
def probe() -> None:
    try:
        operation(True)
    finally:
        raise
""",
            (),
            "exception|builtins.ValueError|operation",
            id="call-retains-static-normal-path",
        ),
        pytest.param(
            """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(AssertionError),
)
def probe(condition: bool) -> None:
    try:
        raise ValueError("body")
    finally:
        assert condition, "cleanup"
""",
            (False,),
            "exception|builtins.AssertionError|cleanup",
            id="unknown-assertion-retains-both-static-paths",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe(flag: bool) -> str:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except (ValueError, TypeError) as error:
        return str(error)
""",
            (False,),
            "return|'type'",
            id="exception-type-tuple-catches-each-member",
        ),
        pytest.param(
            """import efct

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(TypeError),
)
def probe(flag: bool) -> None:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except (ValueError, TypeError):
        raise
""",
            (False,),
            "exception|builtins.TypeError|type",
            id="exception-type-tuple-bare-reraise-is-exact",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe(flag: bool) -> int:
    try:
        if flag:
            raise ValueError("value")
        raise TypeError("type")
    except ValueError:
        return 1
    except (ValueError, TypeError):
        return 2
""",
            (False,),
            "return|2",
            id="later-type-tuple-keeps-uncaught-members",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe() -> int:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
    except* TypeError:
        pass
    return 7
""",
            (),
            "return|7",
            id="exception-group-all-leaves-handled",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.RaiseGroup(TypeError))
def probe() -> None:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
""",
            (),
            "exception|builtins.ExceptionGroup|errors (1 sub-exception)",
            id="exception-group-unmatched-leaf-propagates",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe() -> int:
    try:
        raise ValueError("value")
    except* ValueError:
        pass
    return 7
""",
            (),
            "return|7",
            id="except-star-wraps-and-handles-naked-exception",
        ),
        pytest.param(
            """import efct

@efct.pure(efct.partial.RaiseGroup(ValueError))
def probe() -> None:
    try:
        raise ValueError("value")
    except* ValueError:
        raise
""",
            (),
            "exception|builtins.ExceptionGroup| (1 sub-exception)",
            id="except-star-bare-raise-reraises-group",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe() -> int:
    try:
        raise ExceptionGroup(
            "outer",
            (
                ValueError("value"),
                ExceptionGroup("inner", (TypeError("type"),)),
            ),
        )
    except* (ValueError, TypeError):
        pass
    return 7
""",
            (),
            "return|7",
            id="nested-exception-group-is-recursively-matched",
        ),
        pytest.param(
            """import efct

@efct.pure()
def probe() -> int:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except ExceptionGroup:
        return 7
""",
            (),
            "return|7",
            id="traditional-handler-catches-whole-exception-group",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe() -> int:
    with contextlib.suppress(ValueError):
        raise ValueError("suppressed")
    return 7
""",
            (),
            "return|7",
            id="context-manager-suppresses-matching-exception",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure(efct.partial.Raise(TypeError))
def probe() -> None:
    with contextlib.suppress(ValueError):
        raise TypeError("unmatched")
""",
            (),
            "exception|builtins.TypeError|unmatched",
            id="context-manager-preserves-unmatched-exception",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe(kind: bool) -> int:
    with contextlib.suppress(ValueError), contextlib.suppress(TypeError):
        if kind:
            raise ValueError("value")
        raise TypeError("type")
    return 7
""",
            (False,),
            "return|7",
            id="multiple-context-managers-compose",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe() -> None:
    with contextlib.suppress(ValueError) as marker:
        raise ValueError("value")
    return marker
""",
            (),
            "return|None",
            id="context-manager-target-is-enter-result",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure(efct.partial.Raise(IndexError))
def probe() -> None:
    with contextlib.suppress(()[0]):
        raise IndexError("body")
""",
            (),
            "exception|builtins.IndexError|tuple index out of range",
            id="context-manager-construction-precedes-protection",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe() -> int:
    with contextlib.suppress(IndexError), contextlib.suppress(()[0]):
        raise ValueError("unreachable")
    return 7
""",
            (),
            "return|7",
            id="outer-context-manager-protects-later-construction",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe() -> int:
    try:
        raise ValueError("outer")
    except ValueError:
        with contextlib.suppress(ValueError):
            raise
    return 7
""",
            (),
            "return|7",
            id="context-manager-suppresses-enclosing-handler-reraise",
        ),
        pytest.param(
            """import contextlib
import efct

@efct.pure()
def probe() -> int:
    with contextlib.suppress(ValueError):
        return 7
""",
            (),
            "return|7",
            id="context-manager-preserves-return",
        ),
    ],
)
def test_analyzer_matches_cpython_exception_control_flow(
    tmp_path: Path,
    source: str,
    arguments: tuple[object, ...],
    expected: str,
) -> None:
    assert _diagnostics(source) == []

    result = _run_probe(tmp_path, source, arguments)

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == expected
