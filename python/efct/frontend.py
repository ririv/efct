from __future__ import annotations

import hashlib
import io
import json
import sys
import tokenize
from pathlib import Path
from typing import Any, Literal, TypeAlias

from . import _core
from .i18n import localize_diagnostics, localize_error_text, system_language

PROTOCOL_VERSION = int(_core.runtime_versions()[0])
CheckTarget: TypeAlias = Literal["file", "project"]
TrustPolicy: TypeAlias = Literal["default", "deny_unsafe", "verified_only"]


def decode_source(raw: bytes) -> str:
    encoding, _ = tokenize.detect_encoding(io.BytesIO(raw).readline)
    return raw.decode(encoding)


def encode_source(raw: bytes, filename: str) -> bytes:
    source = decode_source(raw)
    source_sha256 = hashlib.sha256(raw).hexdigest()
    return bytes(_core.encode_source(source, filename, source_sha256))


def encode_file(path: Path) -> bytes:
    return encode_source(path.read_bytes(), str(path))


def check_source(raw: bytes, filename: str) -> list[dict[str, object]]:
    source = decode_source(raw)
    source_sha256 = hashlib.sha256(raw).hexdigest()
    diagnostics = json.loads(_core.check_source(source, filename, source_sha256))
    if not isinstance(diagnostics, list):
        raise RuntimeError(
            localize_error_text(
                "The Rust core returned an invalid diagnostic structure"
            )
        )
    localize_diagnostics(diagnostics, system_language())
    return diagnostics


def prepare_module(
    raw: bytes,
    filename: str,
) -> tuple[_core._PreparedModule, str]:
    source = decode_source(raw)
    source_sha256 = hashlib.sha256(raw).hexdigest()
    return (
        _core.prepare_module(source, filename, source_sha256),
        source,
    )


def inspect_prepared_imports(module: _core._PreparedModule) -> str:
    return _core.prepared_module_imports(module)


def analyze_prepared_runtime(module: _core._PreparedModule) -> str:
    return _core.check_prepared_runtime(module)


def analyze_prepared_runtime_project(
    modules: dict[str, _core._PreparedModule],
    root: Path,
    external_symbols: list[dict[str, object]],
) -> str:
    return _core.check_prepared_runtime_project(
        modules,
        str(root),
        json.dumps(
            external_symbols,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        ),
    )


def analyze_prepared_target(
    modules: dict[str, _core._PreparedModule],
    root: Path,
    target: CheckTarget,
    policy: TrustPolicy,
) -> str:
    return _core.check_prepared_target(
        modules,
        str(root),
        target,
        policy,
    )


def encode_project(
    modules: dict[str, Path],
    root: Path,
    *,
    policy: str = "default",
    external_symbols: list[dict[str, object]] | None = None,
    envelope_payloads: dict[str, bytes] | None = None,
) -> bytes:
    encoded_modules: list[dict[str, Any]] = []
    for name, path in sorted(modules.items()):
        payload = (
            envelope_payloads[name]
            if envelope_payloads is not None
            else encode_file(path)
        )
        envelope = json.loads(payload)
        encoded_modules.append({"name": name, "envelope": envelope})
    project = {
        "protocol_version": PROTOCOL_VERSION,
        "language": {
            "kind": "python",
            "implementation": "cpython",
            "version": list(sys.version_info[:3]),
        },
        "root": str(root),
        "modules": encoded_modules,
        "policy": policy,
        "external_symbols": external_symbols or [],
    }
    return json.dumps(
        project,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
