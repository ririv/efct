import json
from pathlib import Path

import efct._core as core
import pytest
from efct.errors import EfctStartupError


def _write_console_program(path: Path, message: str) -> None:
    path.write_text(
        f'''import efct

_efct = efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
print("{message}")
''',
        encoding="utf-8",
    )


def test_verified_run_target_executes_captured_source_once(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    source = tmp_path / "app.py"
    _write_console_program(source, "captured")
    target = core.prepare_run_target(str(source))

    result = json.loads(core.verify_run_target(target))
    assert result["diagnostics"] == []
    _write_console_program(source, "replaced")

    core.run_verified_target(target, [str(source)])
    assert capsys.readouterr().out == "captured\n"
    with pytest.raises(ValueError, match="already been consumed"):
        core.run_verified_target(target, [str(source)])


def test_run_target_must_be_verified_before_execution(tmp_path: Path) -> None:
    source = tmp_path / "app.py"
    _write_console_program(source, "verified")
    target = core.prepare_run_target(str(source))

    with pytest.raises(ValueError, match="must be verified"):
        core.run_verified_target(target, [str(source)])

    assert json.loads(core.verify_run_target(target))["diagnostics"] == []


def test_rejected_run_target_cannot_be_executed(tmp_path: Path) -> None:
    source = tmp_path / "app.py"
    source.write_text(
        '''import efct

_efct = efct.pure
print("rejected")
''',
        encoding="utf-8",
    )
    target = core.prepare_run_target(str(source))

    result = json.loads(core.verify_run_target(target))
    assert any(item["severity"] == "Error" for item in result["diagnostics"])
    with pytest.raises(ValueError, match="rejected run target"):
        core.run_verified_target(target, [str(source)])


def test_run_target_rejects_changed_trust_manifest(tmp_path: Path) -> None:
    source = tmp_path / "app.py"
    _write_console_program(source, "verified")
    manifest = tmp_path / "efct-trust.toml"
    manifest.write_text(
        '''schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.value"
signature = "() -> int"
effects = []
partials = []
reason = "pending audit"
''',
        encoding="utf-8",
    )
    target = core.prepare_run_target(str(source))
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace("() -> int", "(int) -> int"),
        encoding="utf-8",
    )

    with pytest.raises(EfctStartupError, match="trust manifest changed"):
        core.verify_run_target(target)
