import json
import os
import subprocess
import sys
from pathlib import Path
from string import Formatter

import efct
import pytest
from efct.cli import main
from efct.error_catalog import ERROR_MESSAGES
from efct.i18n import (
    Language,
    LocalizationError,
    localize_diagnostics,
    localize_error_text,
    system_language,
)
from efct.message_catalog import (
    DIAGNOSTIC_MESSAGES,
    OPERATION_MESSAGES,
    SUGGESTION_MESSAGES,
    MessageTemplate,
)


def _render_template(template: str) -> str:
    fields = {
        field: field.upper()
        for _, field, _, _ in Formatter().parse(template)
        if field is not None
    }
    return template.format_map(fields)


def test_unsupported_system_locale_uses_english(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("LC_ALL", "fr_FR.UTF-8")

    assert system_language() is Language.ENGLISH


def test_simplified_chinese_system_locale_uses_chinese(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("LC_ALL", "zh_CN.UTF-8")

    assert system_language() is Language.SIMPLIFIED_CHINESE


def test_cli_switches_complete_diagnostic_for_system_locale(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
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
    monkeypatch.setenv("LC_ALL", "zh_CN.UTF-8")

    assert main(["check", str(source)]) == 1
    output = capsys.readouterr().out
    assert "函数 bad 包含未声明效果 console" in output
    assert "效果来源：" in output
    assert "调用 builtins.print" in output
    assert "调用 builtins.print（bad）" not in output
    assert "建议：" in output


def test_cli_keeps_function_context_for_indirect_effect_trace(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    source = tmp_path / "indirect.py"
    source.write_text(
        """import efct

@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)

@efct.pure
def bad(value: int) -> None:
    show(value)
""",
        encoding="utf-8",
    )
    monkeypatch.setenv("LC_ALL", "zh_CN.UTF-8")

    assert main(["check", str(source)]) == 1
    output = capsys.readouterr().out

    assert "调用 show（bad）" in output
    assert "调用 builtins.print（show）" in output


@pytest.mark.parametrize(
    ("locale_name", "expected"),
    (
        ("zh_CN.UTF-8", "Efct 检查失败：效果来源包含无效帧"),
        ("fr_FR.UTF-8", "Efct check failed: The effect source contains an invalid frame"),
    ),
)
def test_cli_localizes_internal_failures_at_the_user_boundary(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    monkeypatch: pytest.MonkeyPatch,
    locale_name: str,
    expected: str,
) -> None:
    source = tmp_path / "valid.py"
    source.write_text("value = 1\n", encoding="utf-8")
    malformed = [
        {
            "effect_trace": [None],
            "filename": str(source),
        }
    ]
    monkeypatch.setattr(
        "efct.cli.analyze_prepared_target",
        lambda *_: json.dumps(
            {"diagnostics": malformed, "trusted_boundaries": []}
        ),
    )
    monkeypatch.setenv("LC_ALL", locale_name)

    assert main(["check", str(source)]) == 2
    assert expected in capsys.readouterr().err


def test_library_errors_and_public_docstrings_follow_language_boundary(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("LC_ALL", "zh_CN.UTF-8")

    with pytest.raises(TypeError, match="只能用于类型标注"):
        efct.PureCallable()
    assert efct.pure.__doc__ is not None
    assert "Verify inferred partial behavior" in efct.pure.__doc__


def test_library_startup_diagnostic_switches_for_system_locale(tmp_path: Path) -> None:
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
    environment = os.environ.copy()
    environment["LC_ALL"] = "zh_CN.UTF-8"

    result = subprocess.run(
        [sys.executable, str(source)],
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )

    assert result.returncode != 0
    assert "函数 bad 包含未声明效果 console" in result.stderr
    assert "调用 builtins.print" in result.stderr
    assert "调用 builtins.print（bad）" not in result.stderr


def test_english_output_rejects_missing_translation() -> None:
    diagnostics: list[dict[str, object]] = [
        {
            "message": "尚未登记的界面消息",
            "suggestion": None,
            "effect_trace": [],
        }
    ]

    with pytest.raises(LocalizationError, match="Missing translation"):
        localize_diagnostics(diagnostics, Language.ENGLISH)


@pytest.mark.parametrize(
    ("templates", "field"),
    (
        (DIAGNOSTIC_MESSAGES, "message"),
        (SUGGESTION_MESSAGES, "suggestion"),
        (OPERATION_MESSAGES, "operation"),
    ),
)
def test_every_diagnostic_catalog_entry_translates_in_both_directions(
    templates: tuple[MessageTemplate, ...],
    field: str,
) -> None:
    for template in templates:
        english = _render_template(template.en)
        chinese = _render_template(template.zh_cn)
        diagnostic: dict[str, object] = {
            "message": english if field == "message" else "Function sample contains undeclared effect VALUE",
            "suggestion": english if field == "suggestion" else None,
            "effect_trace": [{"operation": english}] if field == "operation" else [],
        }

        localize_diagnostics([diagnostic], Language.SIMPLIFIED_CHINESE)

        if field == "operation":
            assert diagnostic["effect_trace"] == [{"operation": chinese}]
        else:
            assert diagnostic[field] == chinese

        localize_diagnostics([diagnostic], Language.ENGLISH)

        if field == "operation":
            assert diagnostic["effect_trace"] == [{"operation": english}]
        else:
            assert diagnostic[field] == english


def test_every_error_catalog_entry_translates_in_both_directions() -> None:
    for template in ERROR_MESSAGES:
        english = _render_template(template.en)
        chinese = _render_template(template.zh_cn)

        assert localize_error_text(english, Language.SIMPLIFIED_CHINESE) == chinese
        assert localize_error_text(chinese, Language.ENGLISH) == english
