from __future__ import annotations

import locale
import os
import re
import string
from dataclasses import dataclass
from enum import Enum
from typing import Any

from .error_catalog import ERROR_MESSAGES
from .message_catalog import (
    DIAGNOSTIC_MESSAGES,
    OPERATION_MESSAGES,
    SUGGESTION_MESSAGES,
    MessageTemplate,
)

_DICT_TYPE = dict
_ISINSTANCE = isinstance
_LIST_TYPE = list
_MAX = max
_SET = set
_STR_TYPE = str


class Language(Enum):
    ENGLISH = "en"
    SIMPLIFIED_CHINESE = "zh-CN"


class InterfaceMessage(Enum):
    EFFECT_SOURCE = "effect_source"
    SUGGESTION = "suggestion"
    CHECK_FAILED = "check_failed"
    STARTUP_REJECTED = "startup_rejected"
    CHECK_HELP = "check_help"
    RUN_HELP = "run_help"
    TRUST_HELP = "trust_help"
    TRUST_FINGERPRINT_HELP = "trust_fingerprint_help"
    TRUST_FAILED = "trust_failed"


_INTERFACE_MESSAGES = {
    InterfaceMessage.EFFECT_SOURCE: {
        Language.ENGLISH: "Effect source:",
        Language.SIMPLIFIED_CHINESE: "效果来源：",
    },
    InterfaceMessage.SUGGESTION: {
        Language.ENGLISH: "Suggestion: {suggestion}",
        Language.SIMPLIFIED_CHINESE: "建议：{suggestion}",
    },
    InterfaceMessage.CHECK_FAILED: {
        Language.ENGLISH: "Efct check failed: {error}",
        Language.SIMPLIFIED_CHINESE: "Efct 检查失败：{error}",
    },
    InterfaceMessage.STARTUP_REJECTED: {
        Language.ENGLISH: "Module {path} did not pass Efct validation",
        Language.SIMPLIFIED_CHINESE: "模块 {path} 未通过 Efct 验证",
    },
    InterfaceMessage.CHECK_HELP: {
        Language.ENGLISH: "Check a Python file or directory",
        Language.SIMPLIFIED_CHINESE: "检查 Python 文件或目录",
    },
    InterfaceMessage.RUN_HELP: {
        Language.ENGLISH: "Verify and run a Python entry module",
        Language.SIMPLIFIED_CHINESE: "验证并运行 Python 入口模块",
    },
    InterfaceMessage.TRUST_HELP: {
        Language.ENGLISH: "Inspect explicit trust inputs",
        Language.SIMPLIFIED_CHINESE: "检查显式信任输入",
    },
    InterfaceMessage.TRUST_FINGERPRINT_HELP: {
        Language.ENGLISH: "Fingerprint an installed distribution",
        Language.SIMPLIFIED_CHINESE: "生成已安装 distribution 的指纹",
    },
    InterfaceMessage.TRUST_FAILED: {
        Language.ENGLISH: "Efct trust inspection failed: {error}",
        Language.SIMPLIFIED_CHINESE: "Efct 信任检查失败：{error}",
    },
}

_HAN_PATTERN = re.compile("[\u3400-\u9fff]")


class LocalizationError(RuntimeError):
    """Raised when a user-visible message has no complete translation."""


@dataclass(frozen=True, slots=True)
class LocalizedText:
    text: str


@dataclass(frozen=True, slots=True)
class _CompiledPattern:
    expression: re.Pattern[str]
    specificity: int


@dataclass(frozen=True, slots=True)
class _CompiledTemplate:
    message: MessageTemplate
    patterns: tuple[_CompiledPattern, _CompiledPattern]


def system_language() -> Language:
    """Return the supported language selected by the system message locale."""
    locale_name = _system_locale_name()
    normalized = locale_name.replace("_", "-").lower()
    if normalized == "zh" or normalized.startswith(("zh-cn", "zh-hans")):
        return Language.SIMPLIFIED_CHINESE
    return Language.ENGLISH


def interface_message(
    message: InterfaceMessage,
    language: Language,
    **arguments: object,
) -> str:
    return _INTERFACE_MESSAGES[message][language].format_map(arguments)


def localize_diagnostics(
    diagnostics: list[dict[str, Any]],
    language: Language,
) -> None:
    for diagnostic in diagnostics:
        message = diagnostic.get("message")
        if not _ISINSTANCE(message, _STR_TYPE):
            raise LocalizationError("A diagnostic message must be a string")
        diagnostic["message"] = _localize_text(
            message,
            language,
            _COMPILED_DIAGNOSTICS,
            strict=True,
        )
        suggestion = diagnostic.get("suggestion")
        if suggestion is not None:
            if not _ISINSTANCE(suggestion, _STR_TYPE):
                raise LocalizationError("A diagnostic suggestion must be a string")
            diagnostic["suggestion"] = _localize_text(
                suggestion,
                language,
                _COMPILED_SUGGESTIONS,
                strict=True,
            )
        effect_trace = diagnostic.get("effect_trace", [])
        if not _ISINSTANCE(effect_trace, _LIST_TYPE):
            raise LocalizationError("A diagnostic effect trace must be a list")
        for frame in effect_trace:
            if not _ISINSTANCE(frame, _DICT_TYPE) or not _ISINSTANCE(
                frame.get("operation"),
                _STR_TYPE,
            ):
                raise LocalizationError("An effect trace frame must contain an operation")
            frame["operation"] = _localize_text(
                frame["operation"],
                language,
                _COMPILED_OPERATIONS,
                strict=True,
            )


def localize_error_text(text: str, language: Language | None = None) -> str:
    return _localize_text(
        text,
        language or system_language(),
        _COMPILED_ERRORS + _COMPILED_DIAGNOSTICS,
    )


def _system_locale_name() -> str:
    for category in ("LC_ALL", "LC_MESSAGES", "LANG"):
        value = os.environ.get(category)
        if value:
            return value
    detected, _ = locale.getlocale(locale.LC_MESSAGES)
    return detected or "C"


def _compile(template: MessageTemplate) -> _CompiledTemplate:
    return _CompiledTemplate(
        template,
        (_compile_pattern(template.en), _compile_pattern(template.zh_cn)),
    )


def _compile_pattern(source: str) -> _CompiledPattern:
    parts: list[str] = []
    seen: set[str] = set()
    specificity = 0
    for literal, field, _, _ in string.Formatter().parse(source):
        parts.append(re.escape(literal))
        specificity += len(literal)
        if field is None:
            continue
        if field in seen:
            parts.append(f"(?P={field})")
        else:
            parts.append(f"(?P<{field}>.+?)")
            seen.add(field)
    return _CompiledPattern(
        re.compile("".join(parts), re.DOTALL),
        specificity,
    )


def _localize_text(
    text: str,
    language: Language,
    catalog: tuple[_CompiledTemplate, ...],
    *,
    strict: bool = False,
) -> str:
    localized = _match_catalog(text, language, catalog)
    if localized is not None:
        return localized
    if not strict and _HAN_PATTERN.search(text) is None:
        return text
    raise LocalizationError(f"Missing translation for user-visible message: {text}")


def _match_catalog(
    text: str,
    language: Language,
    catalog: tuple[_CompiledTemplate, ...],
) -> str | None:
    candidates: list[tuple[int, MessageTemplate, re.Match[str]]] = []
    for entry in catalog:
        matches = (
            (pattern.specificity, result)
            for pattern in entry.patterns
            if (result := pattern.expression.fullmatch(text)) is not None
        )
        candidates.extend(
            (specificity, entry.message, matched)
            for specificity, matched in matches
        )
    if not candidates:
        return None

    highest_specificity = _MAX(item[0] for item in candidates)
    rendered: set[str] = _SET()
    for specificity, message, matched in candidates:
        if specificity != highest_specificity:
            continue
        template = (
            message.zh_cn
            if language is Language.SIMPLIFIED_CHINESE
            else message.en
        )
        arguments = {
            name: _localize_argument(name, value, language)
            for name, value in matched.groupdict().items()
        }
        rendered.add(template.format_map(arguments))
    selected = rendered.pop()
    if rendered:
        raise LocalizationError(f"Ambiguous translation for user-visible message: {text}")
    return selected


def _localize_argument(name: str, value: str, language: Language) -> str:
    if language is Language.ENGLISH and name in {"error", "message"}:
        for catalog in (_COMPILED_DIAGNOSTICS, _COMPILED_ERRORS):
            localized = _match_catalog(value, language, catalog)
            if localized is not None:
                return localized
    return _localize_embedded(value, language)


def _localize_embedded(value: str, language: Language) -> str:
    if language is Language.SIMPLIFIED_CHINESE:
        return value.replace("<dynamic type>", "<动态类型>").replace(
            "local list[",
            "局部列表[",
        )
    return value.replace("<动态类型>", "<dynamic type>").replace(
        "局部列表[",
        "local list[",
    )


_COMPILED_DIAGNOSTICS = tuple(_compile(item) for item in DIAGNOSTIC_MESSAGES)
_COMPILED_SUGGESTIONS = tuple(_compile(item) for item in SUGGESTION_MESSAGES)
_COMPILED_OPERATIONS = tuple(_compile(item) for item in OPERATION_MESSAGES)
_COMPILED_ERRORS = tuple(_compile(item) for item in ERROR_MESSAGES)
