from pathlib import Path

from .errors import EfctStartupError
from .i18n import (
    InterfaceMessage,
    Language,
    LocalizedText,
    interface_message,
    system_language,
)


def format_startup_diagnostics(
    path: Path,
    diagnostics: list[dict[str, object]],
) -> LocalizedText:
    language = system_language()
    lines = [
        interface_message(
            InterfaceMessage.STARTUP_REJECTED,
            language,
            path=path,
        )
    ]
    for diagnostic in diagnostics:
        lines.append(f"{diagnostic.get('code')}: {diagnostic.get('message')}")
        effect_trace = diagnostic.get("effect_trace", [])
        if not isinstance(effect_trace, list):
            raise EfctStartupError("The effect source is not a list")
        diagnostic_function = diagnostic.get("function")
        for frame in effect_trace:
            if not isinstance(frame, dict):
                raise EfctStartupError("The effect source contains an invalid frame")
            span = frame.get("span")
            filename = frame.get("filename")
            operation = frame.get("operation")
            function = frame.get("function")
            if (
                not isinstance(span, dict)
                or not isinstance(filename, str)
                or not isinstance(operation, str)
                or not isinstance(function, str)
            ):
                raise EfctStartupError(
                    "An effect source frame is missing required fields"
                )
            line = span.get("start_line")
            byte = span.get("start_utf8_byte")
            if not isinstance(line, int) or not isinstance(byte, int):
                raise EfctStartupError("The effect source location is invalid")
            show_function = len(effect_trace) > 1 or function != diagnostic_function
            if not show_function:
                lines.append(f"  {filename}:{line}:{byte + 1} {operation}")
            elif language is Language.SIMPLIFIED_CHINESE:
                lines.append(
                    f"  {filename}:{line}:{byte + 1} {operation}（{function}）"
                )
            else:
                lines.append(f"  {filename}:{line}:{byte + 1} {operation} ({function})")
    return LocalizedText("\n".join(lines))
