import json
from pathlib import Path

import pytest
from efct import _core
from efct.frontend import encode_source

_TEST_ROOT = Path(__file__).parent
_REJECTED_ROOT = _TEST_ROOT / "rejected"
_GOLDEN_ROOT = _TEST_ROOT / "golden"
_REJECTED_SOURCES = tuple(sorted(_REJECTED_ROOT.rglob("*.py")))


def test_each_rejected_example_has_exactly_one_mirrored_golden_file() -> None:
    source_keys = {
        path.relative_to(_REJECTED_ROOT).with_suffix(".json")
        for path in _REJECTED_SOURCES
    }
    golden_keys = {
        path.relative_to(_GOLDEN_ROOT) for path in _GOLDEN_ROOT.rglob("*.json")
    }

    assert golden_keys == source_keys


@pytest.mark.parametrize(
    "source_path",
    _REJECTED_SOURCES,
    ids=lambda path: str(path.relative_to(_REJECTED_ROOT)),
)
def test_rejection_diagnostics_remain_stable(source_path: Path) -> None:
    relative_path = source_path.relative_to(_REJECTED_ROOT)
    expected_path = (_GOLDEN_ROOT / relative_path).with_suffix(".json")
    actual = json.loads(
        _core.check_ast(
            encode_source(source_path.read_bytes(), relative_path.as_posix())
        )
    )
    expected = json.loads(expected_path.read_text(encoding="utf-8"))

    assert actual == expected
