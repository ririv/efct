from __future__ import annotations

from dataclasses import dataclass
from typing import TypeGuard, assert_never

from .i18n import localize_error_text


@dataclass(frozen=True, slots=True)
class Raise:
    """Raising the specified exception type."""

    exception: type[BaseException]

    def __post_init__(self) -> None:
        if not isinstance(self.exception, type) or not issubclass(
            self.exception, BaseException
        ):
            raise TypeError(
                localize_error_text("Raise requires an exception class")
            )


@dataclass(frozen=True, slots=True)
class RaiseGroup:
    """Raising an exception group containing the specified exception type."""

    exception: type[Exception]

    def __post_init__(self) -> None:
        if not isinstance(self.exception, type) or not issubclass(
            self.exception, Exception
        ):
            raise TypeError(
                localize_error_text("RaiseGroup requires an Exception class")
            )


@dataclass(frozen=True, slots=True)
class Diverge:
    """A computation that may not terminate."""


type Partial = Raise | RaiseGroup | Diverge

_PARTIAL_TYPES = (Raise, RaiseGroup, Diverge)


def _is_partial(value: object) -> TypeGuard[Partial]:
    """Return whether a value is a typed partial declaration."""
    return type(value) in _PARTIAL_TYPES


def _canonical_name(value: Partial) -> str:
    """Convert a typed partial declaration to its stable string identifier."""
    match value:
        case Raise(exception):
            name = f"{exception.__module__}.{exception.__qualname__}"
            if not _is_qualified_identifier(name):
                raise ValueError(
                    localize_error_text(
                        f"Invalid exception partial name raise:{name}"
                    )
                )
            return f"raise:{name}"
        case RaiseGroup(exception):
            name = f"{exception.__module__}.{exception.__qualname__}"
            if not _is_qualified_identifier(name):
                raise ValueError(
                    localize_error_text(
                        f"Invalid exception group partial name raise-group:{name}"
                    )
                )
            return f"raise-group:{name}"
        case Diverge():
            return "diverge"
        case _ as unreachable:
            assert_never(unreachable)


def _is_canonical_name(value: str) -> bool:
    if value == "diverge":
        return True
    prefix = next(
        (
            candidate
            for candidate in ("raise:", "raise-group:")
            if value.startswith(candidate)
        ),
        "",
    )
    name = value[len(prefix) :] if prefix else ""
    return "." in name and _is_qualified_identifier(name)


def _is_qualified_identifier(value: str) -> bool:
    return bool(value) and all(segment.isidentifier() for segment in value.split("."))


__all__ = ["Diverge", "Partial", "Raise", "RaiseGroup"]
