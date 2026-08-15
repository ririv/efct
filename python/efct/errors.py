from .i18n import LocalizedText, localize_error_text


class EfctError(Exception):
    """Base class for all Efct errors."""

    def __init__(self, message: str | LocalizedText) -> None:
        text = message.text if isinstance(message, LocalizedText) else localize_error_text(message)
        super().__init__(text)


class EfctStartupError(EfctError):
    """Raised when library-mode startup validation cannot complete."""


class EfctContractError(EfctError):
    """Raised when a call does not satisfy a verified runtime contract."""


class EfctIntegrityError(EfctError):
    """Raised when verified code or dependency integrity is invalidated."""
