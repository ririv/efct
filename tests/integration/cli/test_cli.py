import json
from pathlib import Path
from typing import Any

import efct
import pytest
from efct.cli import main


def test_version_reports_the_distribution_version(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exit_info:
        main(["--version"])

    assert exit_info.value.code == 0
    assert capsys.readouterr().out == f"efct {efct.__version__}\n"


def test_empty_file_check_succeeds(tmp_path: Path) -> None:
    source = tmp_path / "empty.py"
    source.write_text("", encoding="utf-8")

    assert main(["check", str(source)]) == 0


def test_uncontracted_module_execution_passes_cli(tmp_path: Path) -> None:
    source = tmp_path / "value.py"
    source.write_text('value = 1\nprint("ordinary module")\n', encoding="utf-8")

    assert main(["check", str(source)]) == 0


def test_missing_path_fails(tmp_path: Path) -> None:
    assert main(["check", str(tmp_path / "missing.py")]) == 2


def test_pure_function_passes_cli(tmp_path: Path) -> None:
    source = tmp_path / "valid.py"
    source.write_text(
        """import efct

@efct.pure
def add(x: int, y: int) -> int:
    return x + y
""",
        encoding="utf-8",
    )

    assert main(["check", str(source)]) == 0


def test_run_executes_verified_module_from_captured_source(
    tmp_path: Path,
    capsys: object,
) -> None:
    source = tmp_path / "app.py"
    source.write_text(
        """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("verified")
""",
        encoding="utf-8",
    )

    assert main(["run", str(source)]) == 0
    assert capsys.readouterr().out == "verified\n"


def test_run_executes_uncontracted_module_without_claiming_purity(
    tmp_path: Path,
    capsys: object,
) -> None:
    source = tmp_path / "app.py"
    source.write_text('print("ordinary module")\n', encoding="utf-8")

    assert main(["run", str(source)]) == 0
    assert capsys.readouterr().out == "ordinary module\n"


def test_run_does_not_require_contracts_in_an_uncontracted_import_graph(
    tmp_path: Path,
    capsys: object,
) -> None:
    (tmp_path / "dependency.py").write_text(
        'print("ordinary dependency")\n',
        encoding="utf-8",
    )
    source = tmp_path / "app.py"
    source.write_text(
        'import dependency\nprint("ordinary entry")\n',
        encoding="utf-8",
    )

    assert main(["run", str(source)]) == 0
    assert capsys.readouterr().out == "ordinary dependency\nordinary entry\n"


def test_run_rejects_before_pure_module_effect_executes(
    tmp_path: Path,
    capsys: object,
) -> None:
    source = tmp_path / "app.py"
    source.write_text(
        """import efct

_efct = efct.pure
print("must not execute")
""",
        encoding="utf-8",
    )

    assert main(["run", str(source)]) == 1
    output = capsys.readouterr().out
    assert "Module initialization contains undeclared effect console" in output
    assert "must not execute\n" not in output


def test_run_propagates_dependency_initialization_effects(
    tmp_path: Path,
    capsys: object,
) -> None:
    (tmp_path / "dependency.py").write_text(
        """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("dependency executed")
""",
        encoding="utf-8",
    )
    entry = tmp_path / "app.py"
    entry.write_text(
        """import efct
import dependency

_efct = efct.pure
""",
        encoding="utf-8",
    )

    assert main(["run", str(entry)]) == 1
    output = capsys.readouterr().out
    assert "Module initialization contains undeclared effect console" in output
    assert "dependency executed\n" not in output


def test_run_reports_dependency_syntax_error_before_execution(
    tmp_path: Path,
    capsys: object,
) -> None:
    (tmp_path / "dependency.py").write_text("def broken(:\n", encoding="utf-8")
    entry = tmp_path / "app.py"
    entry.write_text(
        '''import efct
import dependency

_efct = efct.pure
''',
        encoding="utf-8",
    )

    assert main(["run", str(entry)]) == 1
    output = capsys.readouterr().out
    assert "P1401" in output
    assert "dependency.py" in output


def test_run_accepts_declared_dependency_initialization_effect(
    tmp_path: Path,
    capsys: object,
) -> None:
    (tmp_path / "dependency.py").write_text(
        """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("dependency executed")
""",
        encoding="utf-8",
    )
    entry = tmp_path / "app.py"
    entry.write_text(
        """import efct
import dependency

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
""",
        encoding="utf-8",
    )

    assert main(["run", str(entry)]) == 0
    assert capsys.readouterr().out == "dependency executed\n"


def test_run_requires_dependency_module_contract(
    tmp_path: Path,
    capsys: object,
) -> None:
    (tmp_path / "dependency.py").write_text(
        """import efct

@efct.pure
def value() -> int:
    return 1
""",
        encoding="utf-8",
    )
    entry = tmp_path / "app.py"
    entry.write_text(
        """import efct
import dependency

_efct = efct.pure
""",
        encoding="utf-8",
    )

    assert main(["run", str(entry)]) == 1
    assert "dependency.<module>" in capsys.readouterr().out


def test_run_propagates_parent_package_initialization_effects(
    tmp_path: Path,
    capsys: object,
) -> None:
    package = tmp_path / "package"
    package.mkdir()
    (package / "__init__.py").write_text(
        """import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("package executed")
""",
        encoding="utf-8",
    )
    (package / "values.py").write_text(
        """import efct

_efct = efct.pure
""",
        encoding="utf-8",
    )
    entry = tmp_path / "app.py"
    entry.write_text(
        """import efct
import package.values

_efct = efct.pure
""",
        encoding="utf-8",
    )

    assert main(["run", str(entry)]) == 1
    output = capsys.readouterr().out
    assert "Module initialization contains undeclared effect console" in output
    assert "package executed\n" not in output


def test_effect_diagnostic_text_points_to_specific_call(
    tmp_path: Path,
    capsys: object,
) -> None:
    source = tmp_path / "console.py"
    source.write_text(
        """import efct

@efct.pure
def bad(value: int) -> int:
    print(value)
    return value
""",
        encoding="utf-8",
    )

    assert main(["check", str(source)]) == 1
    output = capsys.readouterr().out
    assert f"{source}:5:5" in output
    assert "Call builtins.print" in output
    assert "Effect source:" in output


def test_json_contains_byte_and_user_visible_locations(
    tmp_path: Path,
    capsys: object,
) -> None:
    source = tmp_path / "unicode.py"
    source.write_text(
        """import efct

@efct.pure
def bad(α: int) -> int:
    return α + missing
""",
        encoding="utf-8",
    )

    assert main(["check", str(source), "--format=json"]) == 1
    captured = capsys.readouterr()
    output = json.loads(captured.out)
    diagnostic = next(
        item for item in output["diagnostics"] if item["code"] == "P1004"
    )
    assert diagnostic["span"]["start_utf8_byte"] == 16
    assert diagnostic["display_span"]["start_column"] == 16


def test_syntax_error_also_outputs_structured_json(tmp_path: Path, capsys: object) -> None:
    source = tmp_path / "syntax.py"
    source.write_text("def broken(:\n", encoding="utf-8")

    assert main(["check", str(source), "--format=json"]) == 1
    output = json.loads(capsys.readouterr().out)
    assert output["diagnostics"][0]["code"] == "P1401"
    assert "syntax error" in output["diagnostics"][0]["message"]


def test_directory_mode_verifies_cross_module_pure_function(tmp_path: Path) -> None:
    (tmp_path / "maths.py").write_text(
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from maths import increment

@efct.pure
def twice(value: int) -> int:
    return increment(increment(value))
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_cli_current_process_does_not_use_serialized_check_boundary(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    source = tmp_path / "valid.py"
    source.write_text(
        """import efct

@efct.pure
def identity(value: int) -> int:
    return value
""",
        encoding="utf-8",
    )

    def rejected(*args: object, **kwargs: object) -> object:
        raise AssertionError("The CLI must consume prepared HIR modules")

    monkeypatch.setattr("efct.cli._core.check_ast", rejected)
    monkeypatch.setattr("efct.cli._core.check_project", rejected)

    assert main(["check", str(tmp_path)]) == 0


def test_cli_prepares_each_source_once(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    (tmp_path / "first.py").write_text("", encoding="utf-8")
    (tmp_path / "second.py").write_text("", encoding="utf-8")

    from efct import cli

    original = cli.prepare_module
    calls: list[str] = []

    def counted(raw: bytes, filename: str) -> object:
        calls.append(filename)
        return original(raw, filename)

    monkeypatch.setattr(cli, "prepare_module", counted)

    assert main(["check", str(tmp_path)]) == 0
    assert sorted(calls) == sorted(str(path) for path in tmp_path.glob("*.py"))


def test_cli_does_not_write_persistent_source_cache(tmp_path: Path) -> None:
    (tmp_path / "app.py").write_text("", encoding="utf-8")

    assert main(["check", str(tmp_path)]) == 0
    assert not (tmp_path / ".efct").exists()


def test_directory_mode_handles_cross_module_exception(tmp_path: Path) -> None:
    (tmp_path / "errors.py").write_text(
        """import efct

@efct.effects("raise:builtins.ValueError")
def reject(message: str) -> None:
    raise ValueError(message)
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from errors import reject

@efct.pure()
def recover(message: str) -> str:
    try:
        reject(message)
    except ValueError as error:
        return str(error)
    return message
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_handles_cross_module_exception_type_tuple(
    tmp_path: Path,
) -> None:
    (tmp_path / "errors.py").write_text(
        """import efct

class ConfigError(ValueError):
    pass

class ParseError(TypeError):
    pass

@efct.pure(
    efct.partial.Raise(ConfigError),
    efct.partial.Raise(ParseError),
)
def reject(use_config: bool) -> None:
    if use_config:
        raise ConfigError("config")
    raise ParseError("parse")
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from errors import ConfigError, ParseError, reject

@efct.pure()
def recover(use_config: bool) -> str:
    try:
        reject(use_config)
    except (ConfigError, ParseError) as error:
        return str(error)
    return "missing"
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_handles_cross_module_exception_group(
    tmp_path: Path,
) -> None:
    (tmp_path / "errors.py").write_text(
        """import efct

class ConfigError(ValueError):
    pass

class ParseError(TypeError):
    pass

@efct.pure(
    efct.partial.RaiseGroup(ConfigError),
    efct.partial.RaiseGroup(ParseError),
)
def reject() -> None:
    raise ExceptionGroup(
        "errors",
        (ConfigError("config"), ParseError("parse")),
    )
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from errors import ConfigError, ParseError, reject

@efct.pure()
def recover() -> int:
    try:
        reject()
    except* (ConfigError, ParseError):
        pass
    return 1
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_resolves_explicit_cause_across_modules(tmp_path: Path) -> None:
    (tmp_path / "errors.py").write_text(
        """import efct

class SourceError(ValueError):
    pass

@efct.pure(efct.partial.Raise(SourceError))
def reject(message: str) -> None:
    raise SourceError(message)
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from errors import SourceError, reject

@efct.pure(efct.partial.Raise(TypeError))
def wrap(message: str) -> None:
    try:
        reject(message)
    except SourceError as error:
        raise TypeError("wrapped") from error

@efct.pure(efct.partial.Raise(TypeError))
def direct(message: str) -> None:
    raise TypeError("wrapped") from SourceError(message)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_resolves_cross_module_call_in_try_else(tmp_path: Path) -> None:
    (tmp_path / "values.py").write_text(
        """import efct

@efct.pure(efct.partial.Raise(IndexError))
def item(items: tuple[int, ...], index: int) -> int:
    return items[index]

@efct.pure()
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from values import increment, item

@efct.pure()
def item_or_zero(items: tuple[int, ...], index: int) -> int:
    try:
        value = item(items, index)
    except IndexError:
        return 0
    else:
        return increment(value)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_resolves_finally_effects_across_modules(tmp_path: Path) -> None:
    (tmp_path / "operations.py").write_text(
        """import efct

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
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def cleanup() -> None:
    print("cleanup")
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from operations import cleanup, operation

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
    efct.partial.Raise(TypeError),
)
def recover() -> None:
    try:
        operation()
    finally:
        cleanup()
        raise TypeError("cleanup")
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_preserves_cross_module_call_rethrow_in_finally(
    tmp_path: Path,
) -> None:
    (tmp_path / "operations.py").write_text(
        """import efct

@efct.pure(efct.partial.Raise(ValueError))
def operation() -> None:
    raise ValueError("value")
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from operations import operation

@efct.pure(
    efct.partial.Raise(ValueError),
    efct.partial.Raise(RuntimeError),
)
def reject() -> None:
    try:
        operation()
    finally:
        raise
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_propagates_assertion_error_across_modules(
    tmp_path: Path,
) -> None:
    (tmp_path / "validation.py").write_text(
        """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def require(condition: bool) -> None:
    assert condition, "required"
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from validation import require

@efct.pure()
def validate(condition: bool) -> int:
    try:
        require(condition)
    except AssertionError:
        return 0
    return 1
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_suppresses_cross_module_custom_exception(
    tmp_path: Path,
) -> None:
    (tmp_path / "errors.py").write_text(
        """import efct

class ConfigError(ValueError):
    pass

@efct.pure(efct.partial.Raise(ConfigError))
def reject() -> None:
    raise ConfigError("config")
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import contextlib
import efct
from errors import ConfigError, reject

@efct.pure()
def recover() -> int:
    with contextlib.suppress(ConfigError):
        reject()
    return 1
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_directory_mode_verifies_cross_module_higher_order_function(
    tmp_path: Path,
) -> None:
    (tmp_path / "functions.py").write_text(
        """import efct

@efct.pure()
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    (tmp_path / "higher.py").write_text(
        """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from functions import increment
from higher import apply

@efct.pure
def run(value: int) -> int:
    return apply(increment, value)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 0


def test_cross_module_effect_propagates_to_caller(tmp_path: Path, capsys: object) -> None:
    (tmp_path / "output.py").write_text(
        """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from output import show

@efct.pure
def bad(value: int) -> int:
    show(value)
    return value
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 1
    output = json.loads(capsys.readouterr().out)
    diagnostic = next(item for item in output["diagnostics"] if item["code"] == "P1001")
    assert diagnostic["trace"] == ["app.bad", "output.show", "console"]
    assert [frame["function"] for frame in diagnostic["effect_trace"]] == [
        "app.bad",
        "output.show",
    ]
    assert [Path(frame["filename"]).name for frame in diagnostic["effect_trace"]] == [
        "app.py",
        "output.py",
    ]
    assert [frame["operation"] for frame in diagnostic["effect_trace"]] == [
        "Call output.show",
        "Call builtins.print",
    ]


def test_cross_module_divergence_propagates_to_caller(
    tmp_path: Path, capsys: object
) -> None:
    (tmp_path / "worker.py").write_text(
        """import efct

@efct.pure(efct.partial.Diverge())
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)
""",
        encoding="utf-8",
    )
    (tmp_path / "app.py").write_text(
        """import efct
from worker import countdown

@efct.pure()
def run(value: int) -> int:
    return countdown(value)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 1
    output = json.loads(capsys.readouterr().out)
    diagnostic = next(item for item in output["diagnostics"] if item["code"] == "P1001")
    assert diagnostic["message"] == (
        "Function app.run contains undeclared partial behavior diverge"
    )
    assert diagnostic["trace"] == [
        "app.run",
        "worker.countdown",
        "diverge",
    ]


def test_directory_mode_rejects_unresolved_module(tmp_path: Path) -> None:
    (tmp_path / "app.py").write_text(
        """import efct
from missing import value

@efct.pure
def use(value_: int) -> int:
    return value(value_)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 1


def test_strict_policy_preserves_result_without_external_boundaries(
    tmp_path: Path, capsys: object
) -> None:
    source = tmp_path / "valid.py"
    source.write_text(
        """import efct
@efct.pure
def identity(value: int) -> int:
    return value
""",
        encoding="utf-8",
    )

    assert main(["check", str(source), "--verified-only", "--format=json"]) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["policy"] == "verified_only"
    assert output["trusted_boundaries"] == []


def test_unsafe_boundary_must_be_explicit_and_can_be_rejected_by_strict_policy(
    tmp_path: Path, capsys: object
) -> None:
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.math.clamp"
signature = "(int) -> int"
effects = []
partials = []
reason = "manual audit is incomplete"
""",
        encoding="utf-8",
    )
    source = tmp_path / "app.py"
    source.write_text(
        """import efct
from vendor.math import clamp

@efct.effects("unsafe")
def use(value: int) -> int:
    return clamp(value)
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 0
    default_report = json.loads(capsys.readouterr().out)
    assert default_report["trusted_boundaries"][0]["trust"] == "unsafe"
    assert main(["check", str(tmp_path), "--deny-unsafe"]) == 1
    assert "P1303" in capsys.readouterr().out


def test_trust_fingerprint_reports_installed_distribution(
    capsys: object,
) -> None:
    assert main(["trust", "fingerprint", "pytest", "--format=json"]) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["name"] == "pytest"
    assert output["version"]
    assert len(output["installation_sha256"]) == 64


def test_same_input_produces_byte_stable_complete_report(
    tmp_path: Path, capsys: object
) -> None:
    source = tmp_path / "stable.py"
    source.write_text(
        """import efct
@efct.pure
def identity(value: int) -> int:
    return value
""",
        encoding="utf-8",
    )

    assert main(["check", str(source), "--format=json"]) == 0
    first = capsys.readouterr().out
    assert main(["check", str(source), "--format=json"]) == 0
    second = capsys.readouterr().out

    assert first == second
    report = json.loads(first)
    assert report["modules"][0]["source_sha256"]
    assert report["runtime"]["registry"] == 1
