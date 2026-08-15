import json
from pathlib import Path

import pytest
from efct.cli import main

from tests.trust_support import installation_fingerprint, python_manifest_header


def _write_empty_module(root: Path) -> None:
    (root / "app.py").write_text("", encoding="utf-8")


def test_native_trust_check_parses_explicit_unsafe_boundary(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.math.clamp"
signature = "(value: int) -> int"
effects = []
partials = []
reason = "legacy extension has no auditable source"
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["trusted_boundaries"] == [
        {
            "trust": "unsafe",
            "path": "vendor.math.clamp",
            "reason": "legacy extension has no auditable source",
        }
    ]


def test_native_trust_check_accepts_explicit_divergence(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.worker.wait"
signature = "() -> None"
effects = []
partials = ["diverge"]
reason = "the worker may wait indefinitely"
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["trusted_boundaries"][0]["path"] == "vendor.worker.wait"


def test_native_trust_check_rejects_unknown_fields(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.value"
signature = "() -> int"
effects = []
partials = []
reason = "not audited yet"
fallback = true
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert "unknown field" in capsys.readouterr().err


@pytest.mark.parametrize(
    "legacy_field",
    [
        'reviewed_by = ["alice"]',
        "deterministic = true",
        "mutates_arguments = []",
        "retains_arguments = false",
    ],
)
def test_native_audited_boundary_rejects_removed_metadata(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    legacy_field: str,
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        f"""schema = 1

[[symbol]]
trust = "audited"
path = "vendor.value"
owner = "vendor"
implementation = {{ kind = "python", path = "vendor.value" }}
signature = "() -> int"
effects = []
partials = []
{legacy_field}
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert "unknown field" in capsys.readouterr().err


def test_native_trust_check_rejects_invalid_signature(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        """schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.value"
signature = "(list[int]) -> int"
effects = []
partials = []
reason = "not audited yet"
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert "unsupported type" in capsys.readouterr().err


def test_native_audited_boundary_checks_distribution_and_dependency_graph(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    version, installation_hash = installation_fingerprint("pytest")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_pytest.pathlib.fnmatch_ex"
owner = "pytest"
implementation = {{ kind = "python", path = "_pytest.pathlib.fnmatch_ex" }}
signature = "(str, str) -> bool"
effects = []
partials = []
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 0
    report = json.loads(capsys.readouterr().out)
    boundary = report["trusted_boundaries"][0]
    assert boundary["trust"] == "audited"
    assert boundary["path"] == "_pytest.pathlib.fnmatch_ex"
    assert boundary["owner"] == "pytest"
    assert len(boundary["boundary_id"]) == 64


def test_audited_boundary_id_uses_canonical_contract_sets(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    version, installation_hash = installation_fingerprint("pytest")

    def write_manifest(effects: str, partials: str) -> None:
        (tmp_path / "efct-trust.toml").write_text(
            f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_pytest.pathlib.fnmatch_ex"
owner = "pytest"
implementation = {{ kind = "python", path = "_pytest.pathlib.fnmatch_ex" }}
signature = "(str, str) -> bool"
effects = {effects}
partials = {partials}
""",
            encoding="utf-8",
        )

    write_manifest('["console", "network"]', '["diverge", "raise:builtins.ValueError"]')
    assert main(["check", str(tmp_path), "--format=json"]) == 0
    first = json.loads(capsys.readouterr().out)["trusted_boundaries"][0]["boundary_id"]

    write_manifest('["network", "console"]', '["raise:builtins.ValueError", "diverge"]')
    assert main(["check", str(tmp_path), "--format=json"]) == 0
    second = json.loads(capsys.readouterr().out)["trusted_boundaries"][0]["boundary_id"]

    assert second == first


def test_audited_boundary_rejects_module_owned_by_another_distribution(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    version, installation_hash = installation_fingerprint("pytest")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "packaging.tags.parse_tag"
owner = "pytest"
implementation = {{ kind = "python", path = "packaging.tags.parse_tag" }}
signature = "(str) -> frozenset[str]"
effects = []
partials = []
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert (
        "does not resolve to a module owned by distribution pytest"
        in capsys.readouterr().err
    )


@pytest.mark.parametrize(
    ("effects", "partials", "message"),
    [
        ('["diverge"]', "[]", "effects"),
        ("[]", '["console"]', "partials"),
    ],
)
def test_trust_contract_separates_effects_and_partials(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
    effects: str,
    partials: str,
    message: str,
) -> None:
    _write_empty_module(tmp_path)
    (tmp_path / "efct-trust.toml").write_text(
        f"""schema = 1

[[symbol]]
trust = "unsafe"
path = "vendor.value"
signature = "() -> int"
effects = {effects}
partials = {partials}
reason = "not audited"
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert message in capsys.readouterr().err


def test_audited_boundary_rejects_editable_distribution(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    version, installation_hash = installation_fingerprint("efct")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "efct"
version = "{version}"
installation_sha256 = "{installation_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "efct.values.FrozenMap"
owner = "efct"
implementation = {{ kind = "python", path = "efct.values.FrozenMap" }}
signature = "() -> None"
effects = []
partials = []
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path)]) == 2
    assert "cannot be installed in editable mode" in capsys.readouterr().err


def test_audited_implementation_may_resolve_from_declared_dependency_closure(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    _write_empty_module(tmp_path)
    pytest_version, pytest_hash = installation_fingerprint("pytest")
    packaging_version, packaging_hash = installation_fingerprint("packaging")
    (tmp_path / "efct-trust.toml").write_text(
        f"""{python_manifest_header()}
[[distribution]]
name = "pytest"
version = "{pytest_version}"
installation_sha256 = "{pytest_hash}"
dependencies = ["packaging"]

[[distribution]]
name = "packaging"
version = "{packaging_version}"
installation_sha256 = "{packaging_hash}"
dependencies = []

[[symbol]]
trust = "audited"
path = "_pytest.pathlib.fnmatch_ex"
owner = "pytest"
implementation = {{ kind = "python", path = "packaging.tags.parse_tag" }}
signature = "(str, str) -> bool"
effects = []
partials = []
""",
        encoding="utf-8",
    )

    assert main(["check", str(tmp_path), "--format=json"]) == 0
    boundary = json.loads(capsys.readouterr().out)["trusted_boundaries"][0]
    assert boundary["owner"] == "pytest"
