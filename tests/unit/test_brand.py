from importlib import metadata, util

import efct
from efct import _core


def test_efct_is_the_only_public_package_name() -> None:
    assert util.find_spec("efct") is not None
    assert util.find_spec("purepy") is None


def test_efct_is_the_only_console_command() -> None:
    commands = {
        entry_point.name: entry_point.value
        for entry_point in metadata.entry_points(group="console_scripts")
        if entry_point.dist is not None and entry_point.dist.name == "efct"
    }
    assert commands == {"efct": "efct.cli:main"}


def test_public_version_matches_the_native_package_version() -> None:
    assert efct.__version__ == _core.runtime_versions()[1] == metadata.version("efct")
