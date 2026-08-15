import pytest


@pytest.fixture(autouse=True)
def _stabilize_default_message_locale(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("LC_ALL", "C.UTF-8")
