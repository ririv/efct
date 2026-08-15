from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

from tests.trust_support import (
    installation_digest,
    installation_fingerprint,
    python_manifest_header,
    write_fixture_distribution,
)


def _run_module(
    tmp_path: Path, name: str, source: str, command: str
) -> subprocess.CompletedProcess[str]:
    (tmp_path / f"{name}.py").write_text(source, encoding="utf-8")
    environment = os.environ.copy()
    existing_path = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = (
        str(tmp_path) if not existing_path else f"{tmp_path}{os.pathsep}{existing_path}"
    )
    return subprocess.run(
        [sys.executable, "-c", command],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )


def test_valid_pure_function_is_callable_after_decoration(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "valid_module",
        """import efct

@efct.pure
def add(x: int, y: int) -> int:
    return x + y
""",
        "import valid_module, efct; assert valid_module.add(2, 3) == 5; "
        "assert type(valid_module.add) is efct.PureFunction; "
        "assert not hasattr(valid_module.add, '__wrapped__')",
    )

    assert result.returncode == 0, result.stderr


def test_typed_effect_declaration_verifies_at_import(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "typed_effect_module",
        """import efct

@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def reject(value: int) -> None:
    print(value)
    raise ValueError("rejected")
""",
        "import typed_effect_module as module; "
        "assert module.reject.certificate.declared_effects == "
        "('console', 'raise:builtins.OSError', 'raise:builtins.ValueError')",
    )

    assert result.returncode == 0, result.stderr


def test_bounded_pure_partial_declaration_verifies_at_import(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "bounded_pure_module",
        """import efct

@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
""",
        "import bounded_pure_module as module, efct; "
        "assert type(module.reject) is efct.PureFunction; "
        "assert module.reject.certificate.callable_kind.value == "
        "'bounded_pure'; "
        "assert module.reject.certificate.declared_effects == "
        "('raise:builtins.ValueError',); "
        "assert module.reject(2) == 2; "
        "\ntry: module.reject(0)"
        "\nexcept ValueError: pass"
        "\nelse: raise AssertionError('declared exception must remain observable')",
    )

    assert result.returncode == 0, result.stderr


def test_divergence_declaration_verifies_without_running_function(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "divergence_module",
        """import efct

@efct.pure(efct.partial.Diverge())
def wait_forever() -> None:
    while True:
        pass
""",
        "import divergence_module as module, efct; "
        "assert type(module.wait_forever) is efct.PureFunction; "
        "assert module.wait_forever.certificate.declared_effects == ('diverge',)",
    )

    assert result.returncode == 0, result.stderr


def test_undeclared_divergence_rejects_module_before_function_runs(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "undeclared_divergence_module",
        """import efct

@efct.pure()
def wait_forever() -> None:
    while True:
        pass
""",
        "import undeclared_divergence_module",
    )

    assert result.returncode != 0
    assert "undeclared partial behavior diverge" in result.stderr


def test_optimized_python_removes_runtime_assert_but_keeps_effect_upper_bound(
    tmp_path: Path,
) -> None:
    (tmp_path / "assert_module.py").write_text(
        """import efct

@efct.pure(efct.partial.Raise(AssertionError))
def require() -> None:
    assert False, "required"
""",
        encoding="utf-8",
    )
    environment = os.environ.copy()
    existing_path = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = (
        str(tmp_path) if not existing_path else f"{tmp_path}{os.pathsep}{existing_path}"
    )

    result = subprocess.run(
        [
            sys.executable,
            "-O",
            "-c",
            "import assert_module as module\n"
            "if module.require.certificate.declared_effects != "
            "('raise:builtins.AssertionError',):\n"
            " raise RuntimeError('assert effect must remain in the certificate')\n"
            "module.require()",
        ],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert result.returncode == 0, result.stderr


def test_explicit_empty_pure_contract_has_bounded_runtime_certificate(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "exact_pure_module",
        """import efct

@efct.pure()
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
    return 1
""",
        "import exact_pure_module as module, efct; "
        "assert module.recover('bad') == 0; "
        "assert module.recover.certificate.callable_kind.value == "
        "'bounded_pure'; "
        "assert module.recover.certificate.declared_effects == ()",
    )

    assert result.returncode == 0, result.stderr


def test_native_call_gate_binds_keywords_and_rejects_invalid_calls(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "keyword_contract_module",
        """import efct

@efct.pure
def add(x: int, y: int) -> int:
    return x + y
""",
        "import efct, keyword_contract_module as module"
        "\nassert module.add(y=3, x=2) == 5"
        "\nfor call in ("
        "lambda: module.add(1), "
        "lambda: module.add(1, x=2), "
        "lambda: module.add(1, 2, 3), "
        "lambda: module.add(1, extra=2)):"
        "\n try: call()"
        "\n except efct.EfctContractError: pass"
        "\n else: raise AssertionError('invalid argument binding must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_native_call_gate_allows_verified_recursion(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "recursive_module",
        """import efct

@efct.pure
def factorial(value: int) -> int:
    if value == 0:
        return 1
    return value * factorial(value - 1)
""",
        "import recursive_module; assert recursive_module.factorial(5) == 120",
    )

    assert result.returncode == 0, result.stderr


def test_pure_function_with_handled_exception_is_callable(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "exception_module",
        """import efct

@efct.pure
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0
""",
        "import exception_module; assert exception_module.recover('bad') == 0",
    )

    assert result.returncode == 0, result.stderr


def test_pure_function_can_exhaustively_match_result(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "result_match_module",
        """import efct

@efct.pure
def unwrap_or(result: efct.Result[int, str], fallback: int) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Err(_):
            return fallback
""",
        "import efct, result_match_module as module"
        "\nassert module.unwrap_or(efct.Ok(7), 0) == 7"
        "\nassert module.unwrap_or(efct.Err('bad'), 5) == 5",
    )

    assert result.returncode == 0, result.stderr


def test_result_match_variant_binding_is_anchored(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "result_match_integrity_module",
        """import efct

@efct.pure
def unwrap_or(result: efct.Result[int, str], fallback: int) -> int:
    match result:
        case efct.Ok(value):
            return value
        case efct.Err(_):
            return fallback
""",
        "import efct, result_match_integrity_module as module"
        "\noriginal = efct.Ok"
        "\nefct.Ok = lambda value: value"
        "\ntry: module.unwrap_or(efct.Err('bad'), 5)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced Result variant must be rejected')"
        "\nfinally: efct.Ok = original",
    )

    assert result.returncode == 0, result.stderr


def test_local_list_mutation_has_no_observable_effect(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "local_list_module",
        """import efct

@efct.pure
def total(value: int) -> int:
    values = [1, 2]
    alias = values
    alias.append(value)
    return sum(values) + len(alias)
""",
        "import local_list_module; assert local_list_module.total(3) == 9",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_handles_cross_module_exception(tmp_path: Path) -> None:
    (tmp_path / "exception_source.py").write_text(
        """import efct

@efct.effects("raise:builtins.ValueError")
def reject(message: str) -> None:
    raise ValueError(message)
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "exception_caller",
        """import efct
from exception_source import reject

@efct.pure()
def recover(message: str) -> str:
    try:
        reject(message)
    except ValueError as error:
        return str(error)
    return message
""",
        "import exception_caller; assert exception_caller.recover('bad') == 'bad'",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_preserves_explicit_exception_cause(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "exception_cause_module",
        """import efct

@efct.pure(efct.partial.Raise(TypeError))
def chained(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError as error:
        raise TypeError("wrapped") from error

@efct.pure(efct.partial.Raise(TypeError))
def suppressed(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError:
        raise TypeError("wrapped") from None
""",
        "import exception_cause_module as module; "
        "\ntry: module.chained('value')"
        "\nexcept TypeError as error:"
        "\n assert isinstance(error.__cause__, ValueError)"
        "\n assert str(error.__cause__) == 'value'"
        "\nelse: raise AssertionError('chained exception must escape')"
        "\ntry: module.suppressed('value')"
        "\nexcept TypeError as error:"
        "\n assert error.__cause__ is None"
        "\n assert error.__suppress_context__ is True"
        "\nelse: raise AssertionError('suppressed exception must escape')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_handles_cross_module_custom_exception(tmp_path: Path) -> None:
    (tmp_path / "application_errors.py").write_text(
        """class ConfigError(ValueError):
    pass
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "custom_exception_caller",
        """import contextlib
import efct
from application_errors import ConfigError

@efct.pure(efct.partial.Raise(ConfigError))
def reject(message: str) -> None:
    raise ConfigError(message)

@efct.pure()
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        return 0
    return 1

@efct.pure()
def recover_suppressed(message: str) -> int:
    with contextlib.suppress(ConfigError):
        reject(message)
    return 0
""",
        "import custom_exception_caller as module; "
        "assert module.recover('bad') == 0; "
        "assert module.recover_suppressed('bad') == 0; "
        "assert module.reject.certificate.declared_effects == "
        "('raise:application_errors.ConfigError',)",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_handles_cross_module_custom_exception_group(
    tmp_path: Path,
) -> None:
    (tmp_path / "application_group_errors.py").write_text(
        """class ConfigError(ValueError):
    pass

class ParseError(TypeError):
    pass
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "custom_exception_group_caller",
        """import efct
from application_group_errors import ConfigError, ParseError

@efct.pure(
    efct.partial.RaiseGroup(ConfigError),
    efct.partial.RaiseGroup(ParseError),
)
def reject() -> None:
    raise ExceptionGroup(
        "errors",
        (ConfigError("config"), ParseError("parse")),
    )

@efct.pure()
def recover() -> int:
    try:
        reject()
    except* (ConfigError, ParseError):
        pass
    return 1
""",
        "import custom_exception_group_caller as module; "
        "assert module.recover() == 1; "
        "assert module.reject.certificate.declared_effects == "
        "('raise-group:application_group_errors.ConfigError', "
        "'raise-group:application_group_errors.ParseError')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_handles_custom_exception_through_module_import(
    tmp_path: Path,
) -> None:
    (tmp_path / "application_errors.py").write_text(
        """class ConfigError(ValueError):
    pass
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "custom_exception_module_caller",
        """import efct
import application_errors as errors

@efct.pure(efct.partial.Raise(errors.ConfigError))
def reject(message: str) -> None:
    raise errors.ConfigError(message)

@efct.pure()
def recover(message: str) -> int:
    try:
        reject(message)
    except errors.ConfigError:
        return 0
    return 1
""",
        "import custom_exception_module_caller as module; "
        "assert module.recover('bad') == 0; "
        "assert module.reject.certificate.declared_effects == "
        "('raise:application_errors.ConfigError',)",
    )

    assert result.returncode == 0, result.stderr


def test_higher_order_pure_function_only_accepts_exact_verified_callable(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "higher_order_module",
        """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)

@efct.pure()
def increment(value: int) -> int:
    return value + 1

@efct.pure
def normalize(value: str) -> str:
    return value.strip()

@efct.pure
def inferred_identity(value: int) -> int:
    return value

@efct.pure(efct.partial.Raise(ValueError))
def partial_identity(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value

@efct.pure
def keep(function: efct.PureCallable[[int], int]) -> efct.PureCallable[[int], int]:
    return function

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value

@efct.effects()
def identity(value: int) -> int:
    return value
""",
        "import higher_order_module, efct; "
        "assert higher_order_module.apply(higher_order_module.increment, 1) == 2; "
        "assert higher_order_module.keep(higher_order_module.increment) is higher_order_module.increment; "
        "\ndef plain(value): raise AssertionError('plain function must not execute')"
        "\nfor invalid in (plain, higher_order_module.normalize, higher_order_module.inferred_identity, higher_order_module.partial_identity, higher_order_module.show, higher_order_module.identity):"
        "\n try: higher_order_module.apply(invalid, 1)"
        "\n except efct.EfctContractError: pass"
        "\n else: raise AssertionError('a mismatched pure function must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_instantiates_effect_generic_callback(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "effect_generic_module",
        """import efct

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
def keep[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
) -> efct.EffectCallable[[int], int, E]:
    return function
""",
        "import effect_generic_module as module; "
        "assert module.apply(module.increment, 1) == 2; "
        "assert module.apply(module.show, 2) == 2; "
        "assert module.keep(module.show) is module.show",
    )

    assert result.returncode == 0, result.stderr


def test_library_effect_generic_rejects_plain_function_and_signature_mismatch(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "effect_generic_contract",
        """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)

@efct.pure
def normalize(value: str) -> str:
    return value.strip()
""",
        "import efct, effect_generic_contract as module"
        "\ndef plain(value): return value"
        "\nfor invalid in (plain, module.normalize):"
        "\n try: module.apply(invalid, 1)"
        "\n except efct.EfctContractError: pass"
        "\n else: raise AssertionError('uncertified or mismatched function must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_composes_cross_module_higher_order_pure_function(
    tmp_path: Path,
) -> None:
    (tmp_path / "runtime_functions.py").write_text(
        """import efct

@efct.pure()
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    (tmp_path / "runtime_higher.py").write_text(
        """import efct

@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "runtime_app",
        """import efct
from runtime_functions import increment
from runtime_higher import apply

@efct.pure
def run(value: int) -> int:
    return apply(increment, value)
""",
        "import runtime_app; assert runtime_app.run(1) == 2",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_composes_cross_module_effect_generic_function(
    tmp_path: Path,
) -> None:
    (tmp_path / "generic_functions.py").write_text(
        """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value
""",
        encoding="utf-8",
    )
    (tmp_path / "generic_apply.py").write_text(
        """import efct

@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "generic_app",
        """import efct
from generic_apply import apply
from generic_functions import show

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def run(value: int) -> int:
    return apply(show, value)
""",
        "import generic_app; assert generic_app.run(1) == 1",
    )

    assert result.returncode == 0, result.stderr


def test_undeclared_effect_prevents_module_import(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "invalid_module",
        """import efct

@efct.pure
def bad(value: int) -> int:
    print(value)
    return value
""",
        "import invalid_module",
    )

    assert result.returncode != 0
    assert "EfctStartupError" in result.stderr
    assert "P1001" in result.stderr
    assert "Call builtins.print" in result.stderr
    assert "invalid_module.py:5" in result.stderr


def test_exact_argument_contract_rejects_before_function_body_executes(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "contract_module",
        """import efct

@efct.pure
def identity(value: int) -> int:
    return value
""",
        "import contract_module, efct; "
        "\ntry: contract_module.identity(True)"
        "\nexcept efct.EfctContractError: pass"
        "\nelse: raise AssertionError('bool must be rejected as int')",
    )

    assert result.returncode == 0, result.stderr


def test_same_module_pure_functions_compose_through_private_environment(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "composed_module",
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1

@efct.pure
def twice(value: int) -> int:
    return increment(increment(value))
""",
        "import composed_module; assert composed_module.twice(1) == 3",
    )

    assert result.returncode == 0, result.stderr


def test_global_rebinding_revokes_certificate(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "integrity_module",
        """import efct

OFFSET: int = 1

@efct.pure
def shift(value: int) -> int:
    return value + OFFSET
""",
        "import integrity_module, efct; "
        "assert integrity_module.shift(1) == 2; "
        "integrity_module.OFFSET = 2; "
        "\ntry: integrity_module.shift(1)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('rebound dependency must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_constant_type_tampering_before_first_call_is_rejected(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "constant_module",
        """import efct

OFFSET: int = 1

@efct.pure
def shift(value: int) -> int:
    return value + OFFSET
""",
        "import constant_module, efct; constant_module.OFFSET = 'bad'; "
        "\ntry: constant_module.shift(1)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('type-mismatched constant must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_replaced_builtin_prevents_execution(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "builtin_module",
        """import efct

@efct.pure
def size(values: tuple[int, ...]) -> int:
    return len(values)
""",
        "import builtin_module, builtins, efct; "
        "assert builtin_module.size((1, 2)) == 2; builtins.len = lambda value: 0; "
        "\ntry: builtin_module.size((1, 2))"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced builtin must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_exception_group_constructor_identity_is_anchored(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "exception_group_module",
        """import efct

@efct.pure()
def recover() -> int:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except* ValueError:
        pass
    return 1
""",
        "import builtins, efct, exception_group_module as module; "
        "assert module.recover() == 1; "
        "original = builtins.ExceptionGroup; "
        "builtins.ExceptionGroup = lambda message, errors: original(message, errors); "
        "\ntry: module.recover()"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced ExceptionGroup must be rejected')"
        "\nfinally: builtins.ExceptionGroup = original",
    )

    assert result.returncode == 0, result.stderr


def test_builtin_identity_is_anchored_before_decorator_execution(
    tmp_path: Path,
) -> None:
    result = _run_module(
        tmp_path,
        "early_builtin_module",
        """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
""",
        "import builtins, efct; original = builtins.print; "
        "builtins.print = lambda *values: None; "
        "import early_builtin_module; "
        "\ntry: early_builtin_module.show(1)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('a builtin replaced before decoration must be rejected')"
        "\nfinally: builtins.print = original",
    )

    assert result.returncode == 0, result.stderr


def test_effect_function_also_undergoes_startup_validation(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "effect_module",
        """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
""",
        "import effect_module; effect_module.show(7)",
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == "7"


def test_optimized_mode_cannot_skip_validation(tmp_path: Path) -> None:
    (tmp_path / "optimized_module.py").write_text(
        """import efct

@efct.pure
def bad(value: int) -> int:
    print(value)
    return value
""",
        encoding="utf-8",
    )
    environment = os.environ.copy()
    environment["PYTHONPATH"] = str(tmp_path)
    result = subprocess.run(
        [sys.executable, "-O", "-c", "import optimized_module"],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert result.returncode != 0
    assert "EfctStartupError" in result.stderr


def test_dynamic_source_cannot_fall_back_to_plain_function(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "dynamic_module",
        "import efct\n",
        "import efct; namespace = {}; "
        "exec(compile('def value(x: int) -> int:\\n    return x\\n', '<dynamic>', 'exec'), namespace); "
        "efct.pure(namespace['value'])",
    )

    assert result.returncode != 0
    assert "EfctStartupError" in result.stderr


def test_library_mode_composes_cross_module_pure_functions(tmp_path: Path) -> None:
    (tmp_path / "maths.py").write_text(
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "app",
        """import efct
from maths import increment

@efct.pure
def twice(value: int) -> int:
    return increment(increment(value))
""",
        "import app; assert app.twice(1) == 3",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_revokes_replaced_cross_module_binding(tmp_path: Path) -> None:
    (tmp_path / "maths.py").write_text(
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "app",
        """import efct
from maths import increment

@efct.pure
def use(value: int) -> int:
    return increment(value)
""",
        "import app, efct; app.increment = lambda value: value; "
        "\ntry: app.use(1)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced cross-module dependency must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_supports_module_style_cross_file_call(tmp_path: Path) -> None:
    (tmp_path / "maths.py").write_text(
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "app",
        """import maths
import efct

@efct.pure
def use(value: int) -> int:
    return maths.increment(value)
""",
        "import app; assert app.use(1) == 2",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_supports_registered_effect_module(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "registry_module",
        """import efct
import time

@efct.effects("clock")
def now() -> int:
    return time.time_ns()
""",
        "import registry_module; assert type(registry_module.now()) is int",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_discovers_new_api_modules_from_the_model(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "io_api_module",
        """import efct
import io

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def open_file(path: str) -> None:
    io.open(path)
""",
        "import io_api_module",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_anchors_registered_module_alias(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "api_alias_module",
        """import efct
import os as operating_system

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def scan(path: str) -> None:
    operating_system.listdir(path)
""",
        "import api_alias_module as module, efct, types; module.scan('.')"
        "\noriginal = module.operating_system"
        "\nmodule.operating_system = types.SimpleNamespace(listdir=lambda path: [])"
        "\ntry: module.scan('.')"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced API module alias must be rejected')"
        "\nfinally: module.operating_system = original",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_only_exposes_and_anchors_used_api_members(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "api_member_module",
        """import efct
import os

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def scan(path: str) -> None:
    os.listdir(path)
""",
        "import api_member_module as module, efct, os; module.scan('.')"
        "\noriginal_remove = os.remove"
        "\nos.remove = lambda path: None"
        "\ntry: module.scan('.')"
        "\nfinally: os.remove = original_remove"
        "\noriginal_listdir = os.listdir"
        "\nos.listdir = lambda path: []"
        "\ntry: module.scan('.')"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced used API member must be rejected')"
        "\nfinally: os.listdir = original_listdir",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_anchors_contextlib_suppress(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "suppress_api_module",
        """import contextlib
import efct

@efct.pure()
def recover() -> int:
    with contextlib.suppress(ValueError):
        raise ValueError("value")
    return 1
""",
        "import contextlib, efct, suppress_api_module as module; "
        "assert module.recover() == 1"
        "\noriginal = contextlib.suppress"
        "\ncontextlib.suppress = lambda *exceptions: original(*exceptions)"
        "\ntry: module.recover()"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced context manager must be rejected')"
        "\nfinally: contextlib.suppress = original",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_anchors_registered_symbol_import(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "api_symbol_module",
        """import efct
from os import listdir as list_directory

@efct.effects("file.read", "raise:builtins.OSError", "raise:builtins.ValueError")
def scan(path: str) -> None:
    list_directory(path)
""",
        "import api_symbol_module as module, efct; module.scan('.')"
        "\noriginal = module.list_directory"
        "\nmodule.list_directory = lambda path: []"
        "\ntry: module.scan('.')"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replaced API symbol must be rejected')"
        "\nfinally: module.list_directory = original",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_checks_native_optional_boundary_contract(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "option_module",
        """import efct

@efct.pure
def keep(value: int | None) -> int | None:
    return value
""",
        "import option_module, efct; from efct.certificates import OptionalType; "
        "assert option_module.keep(1) == 1; "
        "assert option_module.keep(None) is None; "
        "assert isinstance(option_module.keep.certificate.return_type, OptionalType); "
        "\ntry: option_module.keep(True)"
        "\nexcept efct.EfctContractError: pass"
        "\nelse: raise AssertionError('invalid optional value must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_pure_record_remains_deeply_immutable(tmp_path: Path) -> None:
    result = _run_module(
        tmp_path,
        "record_module",
        """from dataclasses import dataclass
import efct

@efct.pure
@dataclass(frozen=True, slots=True)
class User:
    name: str
    level: int

@efct.pure
def promote(user: User) -> User:
    return User(user.name, user.level + 1)
""",
        "import record_module; user = record_module.User('Ada', 1); "
        "assert record_module.promote(user) == record_module.User('Ada', 2)",
    )

    assert result.returncode == 0, result.stderr


def test_replacement_with_same_module_function_before_first_call_revokes(
    tmp_path: Path,
) -> None:
    (tmp_path / "maths.py").write_text(
        """import efct

@efct.pure
def increment(value: int) -> int:
    return value + 1

@efct.pure
def identity(value: int) -> int:
    return value
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "app",
        """import efct
from maths import increment

@efct.pure
def use(value: int) -> int:
    return increment(value)
""",
        "import app, maths, efct; app.increment = maths.identity; "
        "\ntry: app.use(1)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('wrong verified function must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_binds_unsafe_external_symbol_to_certificate(
    tmp_path: Path,
) -> None:
    (tmp_path / "vendor.py").write_text(
        """def identity(value):
    return value
""",
        encoding="utf-8",
    )
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.identity"
signature = "(int) -> int"
effects = []
partials = []
reason = "unaudited boundary for testing"
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "unsafe_module",
        """import efct
from vendor import identity

@efct.effects("unsafe")
def use(value: int) -> int:
    return identity(value)
""",
        "import unsafe_module, efct; "
        "from efct.certificates import UnsafeBoundary; "
        "boundary = unsafe_module.use.certificate.external_functions[0].boundary; "
        "assert isinstance(boundary, UnsafeBoundary); "
        "assert boundary.reason == 'unaudited boundary for testing'; "
        "assert unsafe_module.use(3) == 3; "
        "unsafe_module.identity = lambda value: value + 1; "
        "\ntry: unsafe_module.use(3)"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('replacing external symbol must revoke')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_uses_fully_matching_audited_certification(tmp_path: Path) -> None:
    version, installation_hash = installation_fingerprint("pytest")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_pytest.pathlib.fnmatch_ex"
owner = "pytest"
implementation = {{ kind = "python", path = "_pytest.pathlib.fnmatch_ex" }}
signature = "(str, str) -> bool"
effects = []
partials = []
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "audited_module",
        """import efct
from _pytest.pathlib import fnmatch_ex

@efct.pure
def matches(path: str, pattern: str) -> bool:
    return fnmatch_ex(pattern, path)
""",
        "import audited_module; "
        "from efct.certificates import AuditedBoundary; "
        "boundary = audited_module.matches.certificate.external_functions[0].boundary; "
        "assert isinstance(boundary, AuditedBoundary); "
        "assert boundary.owner == 'pytest'; "
        "assert len(boundary.boundary_id) == 64; "
        "assert audited_module.matches('a.py', '*.py')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_rejects_audited_symbol_replaced_before_certificate_sealing(
    tmp_path: Path,
) -> None:
    version, installation_hash = installation_fingerprint("pytest")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_pytest.pathlib.fnmatch_ex"
owner = "pytest"
implementation = {{ kind = "python", path = "_pytest.pathlib.fnmatch_ex" }}
signature = "(str, str) -> bool"
effects = []
partials = []
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "prepatched_audited_module",
        """import efct
from _pytest.pathlib import fnmatch_ex

@efct.pure
def matches(path: str, pattern: str) -> bool:
    return fnmatch_ex(pattern, path)
""",
        "import _pytest.pathlib as pathlib, types"
        "\ndef forged_code(pattern, path): return True"
        "\nforged = types.FunctionType("
        "forged_code.__code__.replace(co_filename=pathlib.__file__), "
        "pathlib.__dict__, 'fnmatch_ex')"
        "\nforged.__module__ = '_pytest.pathlib'"
        "\nforged.__qualname__ = 'fnmatch_ex'"
        "\npathlib.fnmatch_ex = forged"
        "\nimport prepatched_audited_module, efct"
        "\ntry: prepatched_audited_module.matches('a.py', '*.py')"
        "\nexcept efct.EfctIntegrityError: pass"
        "\nelse: raise AssertionError('prepatched audited function must be rejected')",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_accepts_audited_native_function(tmp_path: Path) -> None:
    import _hashlib

    source_artifact = Path(_hashlib.__spec__.origin)
    artifact = tmp_path / source_artifact.name
    shutil.copyfile(source_artifact, artifact)
    dist_info = tmp_path / "native_fixture-1.0.dist-info"
    dist_info.mkdir()
    metadata = "Metadata-Version: 2.1\nName: native-fixture\nVersion: 1.0\n"
    metadata_path = dist_info / "METADATA"
    metadata_path.write_text(metadata, encoding="utf-8")
    record_logical = f"{dist_info.name}/RECORD"
    metadata_logical = f"{dist_info.name}/METADATA"
    record = f"{artifact.name},,\n{metadata_logical},,\n{record_logical},,\n"
    record_path = dist_info / "RECORD"
    record_path.write_text(record, encoding="utf-8")
    digest = installation_digest(
        [
            (artifact.name, artifact.read_bytes()),
            (metadata_logical, metadata_path.read_bytes()),
            (record_logical, record_path.read_bytes()),
        ]
    )
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "native-fixture"
version = "1.0"
installation_sha256 = "{digest}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_hashlib.get_fips_mode"
owner = "native-fixture"
implementation = {{ kind = "native", path = "_hashlib.get_fips_mode" }}
signature = "() -> int"
effects = []
partials = []
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "native_audited_module",
        """import efct
from _hashlib import get_fips_mode

@efct.pure
def fips_mode() -> int:
    return get_fips_mode()
""",
        "import native_audited_module; "
        "assert type(native_audited_module.fips_mode()) is int",
    )

    assert result.returncode == 0, result.stderr


def test_library_mode_accepts_reexport_from_audited_dependency_closure(
    tmp_path: Path,
) -> None:
    dependency_hash = write_fixture_distribution(
        tmp_path,
        "audited-dependency",
        "1.0",
        {"audited_dependency.py": "def identity(value):\n    return value\n"},
    )
    owner_hash = write_fixture_distribution(
        tmp_path,
        "audited-owner",
        "1.0",
        {"audited_owner/__init__.py": "from audited_dependency import identity\n"},
    )
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "audited-owner"
version = "1.0"
installation_sha256 = "{owner_hash}"
dependencies = ["audited-dependency"]

[[distribution]]
name = "audited-dependency"
version = "1.0"
installation_sha256 = "{dependency_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "audited_owner.identity"
owner = "audited-owner"
implementation = {{ kind = "python", path = "audited_dependency.identity" }}
signature = "(int) -> int"
effects = []
partials = []
""",
        encoding="utf-8",
    )
    result = _run_module(
        tmp_path,
        "audited_reexport_module",
        """import efct
from audited_owner import identity

@efct.pure
def use(value: int) -> int:
    return identity(value)
""",
        "import audited_reexport_module; assert audited_reexport_module.use(3) == 3",
    )

    assert result.returncode == 0, result.stderr
