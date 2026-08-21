from pathlib import Path

import pytest
from efct.cli import main

PROJECT_ROOT = Path(__file__).parents[3]
CHECKOUT_ROOT = PROJECT_ROOT / "examples" / "checkout"


def test_checkout_example_passes_project_check(
    capsys: pytest.CaptureFixture[str],
) -> None:
    assert main(["check", str(CHECKOUT_ROOT)]) == 0
    assert capsys.readouterr().out == ""


def test_checkout_example_runs_with_explicit_boundaries(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setenv("EFCT_CHECKOUT_REGION", "standard")

    assert main(["run", str(CHECKOUT_ROOT / "main.py")]) == 0
    assert capsys.readouterr().out == (
        "Order: EFCT-MUG\n"
        "Region: standard\n"
        "Subtotal (cents): 7500\n"
        "Discount (cents): 750\n"
        "Tax (cents): 556\n"
        "Total (cents): 7306\n"
    )
