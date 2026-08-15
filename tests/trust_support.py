from __future__ import annotations

import hashlib
import importlib.metadata
import platform
import sys
from pathlib import Path


def installation_fingerprint(name: str) -> tuple[str, str]:
    distribution = importlib.metadata.distribution(name)
    files = distribution.files
    if files is None:
        raise AssertionError(f"Distribution {name} has no installed file record")
    installed: list[tuple[str, bytes]] = []
    for item in files:
        logical = str(item).replace("\\", "/")
        if not logical or logical.endswith(".pyc"):
            continue
        path = Path(item.locate())
        if path.is_symlink() or not path.is_file():
            raise AssertionError(f"Distribution file {path} is not a regular file")
        installed.append((logical, path.read_bytes()))
    return distribution.version, installation_digest(installed)


def installation_digest(installed: list[tuple[str, bytes]]) -> str:
    digest = hashlib.sha256(b"efct-installation-v1\0")
    for logical, content in sorted(installed):
        encoded = logical.encode()
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return digest.hexdigest()


def python_manifest_header() -> str:
    return f'''schema = 1

[python]
implementation = "{sys.implementation.name}"
version = "{platform.python_version()}"
cache_tag = "{sys.implementation.cache_tag}"
'''


def write_fixture_distribution(
    root: Path,
    name: str,
    version: str,
    files: dict[str, str | bytes],
) -> str:
    installed: list[tuple[str, bytes]] = []
    for logical, content in files.items():
        payload = content.encode() if isinstance(content, str) else content
        path = root / logical
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(payload)
        installed.append((logical, payload))
    dist_info = root / f"{name.replace('-', '_')}-{version}.dist-info"
    dist_info.mkdir()
    metadata_logical = f"{dist_info.name}/METADATA"
    metadata = f"Metadata-Version: 2.1\nName: {name}\nVersion: {version}\n".encode()
    (root / metadata_logical).write_bytes(metadata)
    installed.append((metadata_logical, metadata))
    record_logical = f"{dist_info.name}/RECORD"
    record = "".join(
        f"{logical},,\n" for logical in sorted([*files, metadata_logical, record_logical])
    ).encode()
    (root / record_logical).write_bytes(record)
    installed.append((record_logical, record))
    return installation_digest(installed)
