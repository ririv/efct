from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest
from efct.cli import main

_REJECTED_ROOT = Path(__file__).parent / "rejected"
_REJECTED_SOURCES = tuple(sorted(_REJECTED_ROOT.rglob("*.py")))


def _import_from(directory: Path, module: str, command: str) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    existing_path = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = (
        str(directory) if not existing_path else f"{directory}{os.pathsep}{existing_path}"
    )
    return subprocess.run(
        [sys.executable, "-c", f"import {module}; {command}"],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )


def test_mvp_example_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "mvp.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "mvp",
        "assert mvp.add(2, 3) == 5; "
        "assert mvp.normalize('  A  ') == 'a'; "
        "assert mvp.total((1, 2, 3)) == 6",
    )
    assert result.returncode == 0, result.stderr


def test_exception_handling_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "exceptions.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "exceptions",
        "assert exceptions.recover('bad') == 0; "
        "assert exceptions.exception_message('bad') == 'bad'; "
        "assert exceptions.selected_exception_message(True) == 'value'; "
        "assert exceptions.selected_exception_message(False) == 'type'; "
        "assert exceptions.recover_chained_error('bad') == 'wrapped'; "
        "assert exceptions.recover_suppressed_context('bad') == 'wrapped'; "
        "assert exceptions.unreachable_handler() == 1; "
        "assert exceptions.recover_reraised_item((), 0) == 0; "
        "assert exceptions.item_or_zero((7,), 0) == 7; "
        "assert exceptions.item_or_zero((), 0) == 0; "
        "assert exceptions.recover_finally_override() == 0; "
        "assert exceptions.recover_finally_rethrow() == 'pending'; "
        "assert exceptions.recover_missing_current_exception() == "
        "'No active exception to reraise'; "
        "\ntry: exceptions.rethrow_enclosing_handler_exception()"
        "\nexcept ValueError as error: assert str(error) == 'outer'"
        "\nelse: raise AssertionError('outer exception must be reraised'); "
        "assert exceptions.exception_message.certificate.dependency_names == "
        "('ValueError', 'str'); "
        "assert exceptions.reraised_item.certificate.declared_effects == "
        "('raise:builtins.IndexError',)",
    )
    assert result.returncode == 0, result.stderr


def test_context_manager_suppression_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "context_managers.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "context_managers",
        "assert context_managers.recover('value') == 1; "
        "assert context_managers.recover_lookup() == 1; "
        "assert context_managers.recover_imported() == 1; "
        "\ntry: context_managers.preserve_unmatched()"
        "\nexcept TypeError as error: assert str(error) == 'type'"
        "\nelse: raise AssertionError('TypeError must escape')",
    )
    assert result.returncode == 0, result.stderr


def test_exception_groups_pass_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "exception_groups.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "exception_groups",
        "assert exception_groups.recover() == 1; "
        "assert exception_groups.recover_nested() == 1; "
        "assert exception_groups.recover_naked() == 1; "
        "assert exception_groups.recover_whole_group() == 1; "
        "\ntry: exception_groups.preserve_unmatched()"
        "\nexcept ExceptionGroup as errors: "
        "assert tuple(type(error) for error in errors.exceptions) == (TypeError,)"
        "\nelse: raise AssertionError('unmatched subgroup must escape'); "
        "assert exception_groups.preserve_unmatched.certificate.declared_effects == "
        "('raise-group:builtins.TypeError',)",
    )
    assert result.returncode == 0, result.stderr


def test_termination_contracts_pass_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "termination.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "termination",
        "assert termination.finite_loop() == 1; "
        "assert termination.skip_unreachable_cycle() == 1; "
        "assert termination.countdown(3) == 0; "
        "assert termination.guarded_countdown(3) == 0; "
        "assert termination.guarded_countdown(-3) == 0; "
        "assert termination.wait_forever.certificate.declared_effects == "
        "('diverge',)",
    )
    assert result.returncode == 0, result.stderr


def test_partial_contracts_pass_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "partial_contracts.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "partial_contracts",
        "assert partial_contracts.inferred(2) == 2; "
        "assert partial_contracts.exact(2) == 3; "
        "assert partial_contracts.bounded(2) == 2; "
        "assert partial_contracts.handled('bad') == 0; "
        "assert partial_contracts.recover_custom('bad') == 0; "
        "assert partial_contracts.recover_custom_string('bad') == 0; "
        "assert partial_contracts.recover_assertion(True) == 'valid'; "
        "assert partial_contracts.recover_assertion(False) == 'required'; "
        "assert partial_contracts.divide_by_literal(7) == 3; "
        "assert partial_contracts.quotient(7, 2) == 3; "
        "assert partial_contracts.recover_division(7, 0) == 0; "
        "assert partial_contracts.last_pair((7, 'value')) == 'value'; "
        "assert partial_contracts.tuple_item((7, 8), 1) == 8; "
        "assert partial_contracts.recover_index((), 0) == 0; "
        "mapping = __import__('efct').FrozenMap((('answer', 42),)); "
        "assert partial_contracts.map_item(mapping, 'answer') == 42; "
        "assert partial_contracts.recover_key(mapping, 'missing') == 0; "
        "assert len(partial_contracts.distinct_map()) == 2; "
        "assert len(partial_contracts.make_map('left', 'right')) == 2; "
        "assert partial_contracts.recover_duplicate('same')['same'] == 0; "
        "assert partial_contracts.recover_static_index() == 0; "
        "assert partial_contracts.conditional_index(False) == 1; "
        "assert partial_contracts.custom.certificate.declared_effects == "
        "('raise:partial_contracts.ConfigError',); "
        "assert partial_contracts.custom_string.certificate.declared_effects == "
        "('raise:partial_contracts.ConfigError',); "
        "assert partial_contracts.exact.certificate.declared_effects == (); "
        "assert partial_contracts.bounded.certificate.declared_effects == "
        "('raise:builtins.ValueError',)",
    )
    assert result.returncode == 0, result.stderr


def test_higher_order_pure_function_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "higher_order.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "higher_order",
        "assert higher_order.run(1) == 2; assert higher_order.run_answer() == 42",
    )
    assert result.returncode == 0, result.stderr


def test_local_mutable_list_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "local_mutation.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "local_mutation",
        "assert local_mutation.total(3) == 9",
    )
    assert result.returncode == 0, result.stderr


def test_unified_pure_decorator_accepts_pure_records() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "pure_record.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "pure_record",
        "assert pure_record.shift(pure_record.Point(1, 2), 3) == pure_record.Point(4, 5)",
    )
    assert result.returncode == 0, result.stderr


def test_pure_module_initialization_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "module_initialization.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "module_initialization",
        "assert module_initialization.identity(3) == 3; "
        "assert module_initialization._efct is __import__('efct').pure",
    )
    assert result.returncode == 0, result.stderr


def test_uncontracted_module_initialization_passes_cli_and_library_modes() -> None:
    root = Path(__file__).parent
    source = root / "accepted" / "uncontracted_module.py"

    assert main(["check", str(source)]) == 0
    result = _import_from(
        source.parent,
        "uncontracted_module",
        "assert uncontracted_module.started; "
        "assert uncontracted_module.identity(3) == 3",
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == "ordinary module\n"


@pytest.mark.parametrize(
    "source",
    _REJECTED_SOURCES,
    ids=lambda source: str(source.relative_to(_REJECTED_ROOT)),
)
def test_rejected_examples_fail_cli_and_library_modes(source: Path) -> None:
    assert main(["check", str(source)]) == 1
    result = _import_from(
        source.parent,
        source.stem,
        "raise AssertionError('the module must not import successfully')",
    )
    assert result.returncode != 0
    assert "EfctStartupError" in result.stderr
