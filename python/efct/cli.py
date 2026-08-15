from __future__ import annotations

import argparse
import hashlib
import io
import json
import sys
import tokenize
from pathlib import Path
from typing import Any, Sequence

from . import __version__, _core
from .errors import EfctStartupError
from .frontend import (
    CheckTarget,
    TrustPolicy,
    analyze_prepared_target,
    inspect_prepared_imports,
    prepare_module,
)
from .i18n import (
    InterfaceMessage,
    Language,
    interface_message,
    localize_diagnostics,
    localize_error_text,
    system_language,
)

_EXCLUDED_DIRECTORIES = frozenset({".git", ".venv", "__pycache__", "build", "dist"})


def _discover(target: Path) -> list[Path]:
    if not target.exists():
        raise FileNotFoundError(f"Path does not exist: {target}")
    if target.is_file():
        if target.suffix != ".py":
            raise ValueError(f"Only .py files are supported: {target}")
        return [target]
    if target.is_symlink():
        raise ValueError(f"A directory target cannot be a symbolic link: {target}")

    files = [
        path
        for path in target.rglob("*.py")
        if not path.is_symlink()
        and not any(part in _EXCLUDED_DIRECTORIES for part in path.relative_to(target).parts)
    ]
    return sorted(files, key=lambda path: path.as_posix())


def _module_name(root: Path, path: Path) -> str:
    relative = path.relative_to(root)
    parts = list(relative.with_suffix("").parts)
    if parts[-1] == "__init__":
        parts.pop()
    if not parts:
        raise ValueError("A project root cannot contain only __init__.py")
    if not all(part.isidentifier() for part in parts):
        raise ValueError(f"The module path is not a valid Python qualified name: {relative}")
    return ".".join(parts)


def _check_target(
    root: Path,
    paths: list[Path],
    prepared: dict[Path, _core._PreparedModule],
    *,
    target: CheckTarget,
    policy: TrustPolicy,
) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    modules: dict[str, _core._PreparedModule] = {}
    for path in paths:
        name = _module_name(root, path)
        if name in modules:
            raise ValueError(f"Module name {name} is defined by multiple source files")
        modules[name] = prepared[path]
    result = analyze_prepared_target(
        modules,
        root,
        target,
        policy,
    )
    return _decode_target_check(result)


def _decode_target_check(
    result: str,
) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    document: object = json.loads(result)
    if not isinstance(document, dict) or set(document) != {
        "diagnostics",
        "trusted_boundaries",
    }:
        raise RuntimeError("The Rust core returned an invalid target check structure")
    diagnostics = document["diagnostics"]
    boundaries = document["trusted_boundaries"]
    if not isinstance(diagnostics, list) or not isinstance(boundaries, list):
        raise RuntimeError("The Rust core returned an invalid target check structure")
    for diagnostic in diagnostics:
        if not isinstance(diagnostic, dict):
            raise RuntimeError("The Rust core returned an invalid diagnostic structure")
        _attach_display_spans(diagnostic)
    valid_boundaries = all(
        isinstance(boundary, dict)
        and (
            (
                boundary.get("trust") == "audited"
                and set(boundary) == {"path", "trust", "owner", "boundary_id"}
            )
            or (
                boundary.get("trust") == "unsafe"
                and set(boundary) == {"path", "trust", "reason"}
            )
        )
        for boundary in boundaries
    )
    if not valid_boundaries:
        raise RuntimeError("The Rust core returned an invalid trust report structure")
    return diagnostics, boundaries


def _display_span(path: Path, span: dict[str, Any]) -> dict[str, int]:
    raw = path.read_bytes()
    encoding, _ = tokenize.detect_encoding(io.BytesIO(raw).readline)
    lines = raw.decode(encoding).splitlines()

    def column(line_number: int, utf8_byte: int) -> int:
        if line_number < 1 or line_number > len(lines):
            raise RuntimeError("The diagnostic line is outside the source range")
        encoded = lines[line_number - 1].encode("utf-8")
        if utf8_byte < 0 or utf8_byte > len(encoded):
            raise RuntimeError("The diagnostic byte column is outside the source range")
        prefix = encoded[:utf8_byte].decode("utf-8")
        return len(prefix.expandtabs(4)) + 1

    start_line = int(span["start_line"])
    end_line = int(span["end_line"])
    return {
        "start_line": start_line,
        "start_column": column(start_line, int(span["start_utf8_byte"])),
        "end_line": end_line,
        "end_column": column(end_line, int(span["end_utf8_byte"])),
    }


def _attach_display_spans(diagnostic: dict[str, Any]) -> None:
    span = diagnostic.get("span")
    filename = diagnostic.get("filename")
    if isinstance(span, dict) and isinstance(filename, str) and filename != "<efct-project>":
        diagnostic["display_span"] = _display_span(Path(filename), span)
    effect_trace = diagnostic.get("effect_trace")
    if not isinstance(effect_trace, list):
        return
    for frame in effect_trace:
        if not isinstance(frame, dict):
            raise RuntimeError("The effect source contains an invalid frame")
        frame_span = frame.get("span")
        frame_filename = frame.get("filename")
        if not isinstance(frame_span, dict) or not isinstance(frame_filename, str):
            raise RuntimeError("The effect source is missing a file or location")
        frame["display_span"] = _display_span(Path(frame_filename), frame_span)


def _prepare_sources(
    root: Path,
    paths: list[Path],
) -> tuple[
    dict[Path, _core._PreparedModule],
    list[dict[str, object]],
    list[dict[str, Any]],
]:
    prepared: dict[Path, _core._PreparedModule] = {}
    report: list[dict[str, object]] = []
    syntax_diagnostics: list[dict[str, Any]] = []
    for path in paths:
        raw = path.read_bytes()
        try:
            module, _ = prepare_module(raw, str(path))
            prepared[path] = module
            dependencies = _prepared_dependencies(module)
        except SyntaxError as error:
            dependencies = frozenset()
            syntax_diagnostics.append(_syntax_diagnostic(path, error))
        report.append(
            {
                "name": _module_name(root, path),
                "filename": str(path),
                "source_sha256": hashlib.sha256(raw).hexdigest(),
                "dependencies": sorted(dependencies),
            }
        )
    return (
        prepared,
        sorted(report, key=lambda item: str(item["name"])),
        syntax_diagnostics,
    )


def _prepared_dependencies(module: _core._PreparedModule) -> frozenset[str]:
    document: object = json.loads(inspect_prepared_imports(module))
    if not isinstance(document, list) or not all(
        type(item) is str for item in document
    ):
        raise RuntimeError("The Rust module import plan is invalid")
    return frozenset(document)


def _render_text(
    diagnostics: list[dict[str, Any]],
    language: Language,
) -> str:
    lines: list[str] = []
    for diagnostic in diagnostics:
        span = diagnostic.get("display_span")
        location = diagnostic["filename"]
        if span is not None:
            location += f":{span['start_line']}:{span['start_column']}"
        lines.append(f"{diagnostic['code']} {location}")
        lines.append(diagnostic["message"])
        effect_trace = diagnostic.get("effect_trace", [])
        if effect_trace:
            lines.append(interface_message(InterfaceMessage.EFFECT_SOURCE, language))
            diagnostic_function = diagnostic.get("function")
            for frame in effect_trace:
                frame_span = frame["display_span"]
                frame_location = (
                    f"{frame['filename']}:{frame_span['start_line']}:"
                    f"{frame_span['start_column']}"
                )
                function = frame["function"]
                show_function = len(effect_trace) > 1 or function != diagnostic_function
                if not show_function:
                    lines.append(f"  {frame_location} {frame['operation']}")
                elif language is Language.SIMPLIFIED_CHINESE:
                    lines.append(f"  {frame_location} {frame['operation']}（{function}）")
                else:
                    lines.append(f"  {frame_location} {frame['operation']} ({function})")
        suggestion = diagnostic.get("suggestion")
        if suggestion:
            lines.append(
                interface_message(
                    InterfaceMessage.SUGGESTION,
                    language,
                    suggestion=suggestion,
                )
            )
    return "\n".join(lines)


def _syntax_diagnostic(path: Path, error: SyntaxError) -> dict[str, Any]:
    line = (error.text or "").rstrip("\r\n")
    start_character = max((error.offset or 1) - 1, 0)
    end_character = max((error.end_offset or error.offset or 1) - 1, start_character)
    start_byte = len(line[:start_character].encode("utf-8"))
    end_byte = len(line[:end_character].encode("utf-8"))
    line_number = error.lineno or 1
    end_line = error.end_lineno or line_number
    return {
        "code": "P1401",
        "severity": "Error",
        "filename": str(path),
        "span": {
            "start_line": line_number,
            "start_utf8_byte": start_byte,
            "end_line": end_line,
            "end_utf8_byte": end_byte,
        },
        "display_span": {
            "start_line": line_number,
            "start_column": start_character + 1,
            "end_line": end_line,
            "end_column": end_character + 1,
        },
        "function": None,
        "message": f"Python syntax error: {error.msg}",
        "trace": [],
        "suggestion": "Fix the Python syntax error first",
    }


def build_parser() -> argparse.ArgumentParser:
    language = system_language()
    parser = argparse.ArgumentParser(prog="efct")
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    subparsers = parser.add_subparsers(dest="command", required=True)
    check_parser = subparsers.add_parser(
        "check",
        help=interface_message(InterfaceMessage.CHECK_HELP, language),
    )
    check_parser.add_argument("target", type=Path)
    check_parser.add_argument("--format", choices=("text", "json"), default="text")
    policy = check_parser.add_mutually_exclusive_group()
    policy.add_argument("--deny-unsafe", action="store_true")
    policy.add_argument("--verified-only", action="store_true")
    run_parser = subparsers.add_parser(
        "run",
        help=interface_message(InterfaceMessage.RUN_HELP, language),
    )
    run_parser.add_argument("target", type=Path)
    run_parser.add_argument("arguments", nargs=argparse.REMAINDER)
    trust_parser = subparsers.add_parser(
        "trust",
        help=interface_message(InterfaceMessage.TRUST_HELP, language),
    )
    trust_subparsers = trust_parser.add_subparsers(
        dest="trust_command",
        required=True,
    )
    fingerprint_parser = trust_subparsers.add_parser(
        "fingerprint",
        help=interface_message(InterfaceMessage.TRUST_FINGERPRINT_HELP, language),
    )
    fingerprint_parser.add_argument("distribution")
    fingerprint_parser.add_argument(
        "--format",
        choices=("toml", "json"),
        default="toml",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    language = system_language()
    arguments = build_parser().parse_args(argv)
    if arguments.command == "run":
        return _run(arguments.target, arguments.arguments, language)
    if arguments.command == "trust":
        if arguments.trust_command != "fingerprint":
            raise AssertionError("argparse must reject unknown trust commands")
        return _fingerprint_distribution(
            arguments.distribution,
            arguments.format,
            language,
        )
    if arguments.command != "check":
        raise AssertionError("argparse must reject unknown commands")
    policy: TrustPolicy = (
        "verified_only"
        if arguments.verified_only
        else "deny_unsafe"
        if arguments.deny_unsafe
        else "default"
    )

    try:
        diagnostics: list[dict[str, Any]] = []
        trusted_boundaries: list[dict[str, str]] = []
        paths = _discover(arguments.target)
        report_root = arguments.target.parent if arguments.target.is_file() else arguments.target
        prepared, report_modules, syntax_diagnostics = _prepare_sources(
            report_root,
            paths,
        )
        diagnostics.extend(syntax_diagnostics)
        if not syntax_diagnostics:
            trust_root = arguments.target.parent if arguments.target.is_file() else arguments.target
            target: CheckTarget = "file" if arguments.target.is_file() else "project"
            target_diagnostics, trusted_boundaries = _check_target(
                trust_root,
                paths,
                prepared,
                target=target,
                policy=policy,
            )
            diagnostics.extend(target_diagnostics)
    except (OSError, EfctStartupError, ValueError, RuntimeError, UnicodeError) as error:
        print(
            interface_message(
                InterfaceMessage.CHECK_FAILED,
                language,
                error=localize_error_text(str(error), language),
            ),
            file=sys.stderr,
        )
        return 2

    localize_diagnostics(diagnostics, language)

    if arguments.format == "json":
        print(
            json.dumps(
                {
                    "version": 1,
                    "policy": policy,
                    "runtime": {
                        "python": list(sys.version_info[:3]),
                        "protocol": _core.runtime_versions()[0],
                        "core": _core.runtime_versions()[1],
                        "registry": _core.runtime_versions()[2],
                    },
                    "modules": report_modules,
                    "trusted_boundaries": trusted_boundaries,
                    "diagnostics": diagnostics,
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
    elif diagnostics:
        print(_render_text(diagnostics, language))

    return 1 if any(item.get("severity") == "Error" for item in diagnostics) else 0


def _fingerprint_distribution(
    name: str,
    output_format: str,
    language: Language,
) -> int:
    try:
        version, digest = _core.fingerprint_distribution(name)
    except (OSError, EfctStartupError, ValueError, RuntimeError, UnicodeError) as error:
        print(
            interface_message(
                InterfaceMessage.TRUST_FAILED,
                language,
                error=localize_error_text(str(error), language),
            ),
            file=sys.stderr,
        )
        return 2
    if output_format == "json":
        print(
            json.dumps(
                {
                    "name": name,
                    "version": version,
                    "installation_sha256": digest,
                },
                ensure_ascii=False,
                separators=(",", ":"),
            )
        )
    else:
        print(f"name = {json.dumps(name, ensure_ascii=False)}")
        print(f"version = {json.dumps(version, ensure_ascii=False)}")
        print(f'installation_sha256 = "{digest}"')
    return 0


def _run(target: Path, arguments: list[str], language: Language) -> int:
    try:
        run_target = _core.prepare_run_target(str(target))
        diagnostics, _ = _decode_target_check(_core.verify_run_target(run_target))
    except SyntaxError as error:
        filename = error.filename if isinstance(error.filename, str) else str(target)
        diagnostics = [_syntax_diagnostic(Path(filename), error)]
        localize_diagnostics(diagnostics, language)
        print(_render_text(diagnostics, language))
        return 1
    except (OSError, EfctStartupError, ValueError, RuntimeError, UnicodeError) as error:
        print(
            interface_message(
                InterfaceMessage.CHECK_FAILED,
                language,
                error=localize_error_text(str(error), language),
            ),
            file=sys.stderr,
        )
        return 2

    localize_diagnostics(diagnostics, language)
    if diagnostics:
        print(_render_text(diagnostics, language))
    if any(item.get("severity") == "Error" for item in diagnostics):
        return 1

    entry = target.resolve()
    program_arguments = list(arguments)
    if program_arguments[:1] == ["--"]:
        program_arguments.pop(0)
    try:
        _core.run_verified_target(run_target, [str(entry), *program_arguments])
    except SystemExit as error:
        if error.code is None:
            return 0
        if type(error.code) is int:
            return error.code
        print(error.code, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
