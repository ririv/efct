import json
from pathlib import Path
from typing import TypedDict, cast

import pytest
from efct.cli import main

PROJECT_ROOT = Path(__file__).parents[3]
DIAGNOSTICS_ROOT = PROJECT_ROOT / "examples" / "diagnostics"
REJECTED_ROOT = DIAGNOSTICS_ROOT / "rejected"
FIXED_ROOT = DIAGNOSTICS_ROOT / "fixed"


class Diagnostic(TypedDict):
    code: str
    message: str


def _check_json(path: Path, capsys: pytest.CaptureFixture[str]) -> list[Diagnostic]:
    assert main(["check", str(path), "--format=json"]) == 1
    output = cast(dict[str, object], json.loads(capsys.readouterr().out))
    return cast(list[Diagnostic], output["diagnostics"])


@pytest.mark.parametrize(
    ("filename", "expected_messages"),
    [
        (
            "hidden_file_read.py",
            ["Function probe_file contains undeclared effect file.read"],
        ),
        (
            "hidden_nondeterminism.py",
            [
                "Function session_marker contains undeclared effect clock",
                "Function session_marker contains undeclared effect random",
            ],
        ),
        (
            "undeclared_exception.py",
            [
                "Function require_non_negative contains undeclared partial behavior "
                "raise:builtins.ValueError"
            ],
        ),
        (
            "uncertified_dependency.py",
            ["Imported module requests is not certified by the MVP"],
        ),
        (
            "unproven_termination.py",
            [
                "Function wait_forever contains undeclared partial behavior diverge"
            ],
        ),
    ],
)
def test_rejected_diagnostic_example_reports_expected_behavior(
    filename: str,
    expected_messages: list[str],
    capsys: pytest.CaptureFixture[str],
) -> None:
    diagnostics = _check_json(REJECTED_ROOT / filename, capsys)
    messages = [diagnostic["message"] for diagnostic in diagnostics]
    for expected in expected_messages:
        assert expected in messages


def test_fixed_diagnostic_examples_pass_together(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert main(["check", str(FIXED_ROOT)]) == 0
    assert capsys.readouterr().out == ""
