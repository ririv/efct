from __future__ import annotations

from dataclasses import dataclass
from typing import Never, TypeGuard, assert_never, final

from .i18n import localize_error_text


@dataclass(frozen=True, slots=True)
class Console:
    """Console input or output."""


@final
class File:
    """Namespace for precise file effects."""

    __slots__ = ()

    def __new__(cls) -> Never:
        raise TypeError(
            localize_error_text(
                f"Effect namespace {cls.__name__} cannot be instantiated"
            )
        )

    @dataclass(frozen=True, slots=True)
    class Read:
        """File-system reads."""

    @dataclass(frozen=True, slots=True)
    class Write:
        """File-system writes."""


@dataclass(frozen=True, slots=True)
class Network:
    """Network access."""


@dataclass(frozen=True, slots=True)
class Clock:
    """Clock access."""


@dataclass(frozen=True, slots=True)
class Random:
    """Randomness access."""


@dataclass(frozen=True, slots=True)
class Environment:
    """Environment access."""


@dataclass(frozen=True, slots=True)
class Process:
    """Process execution."""


@final
class State:
    """Namespace for precise global-state effects."""

    __slots__ = ()

    def __new__(cls) -> Never:
        raise TypeError(
            localize_error_text(
                f"Effect namespace {cls.__name__} cannot be instantiated"
            )
        )

    @dataclass(frozen=True, slots=True)
    class Read:
        """Global-state reads."""

    @dataclass(frozen=True, slots=True)
    class Write:
        """Global-state writes."""


@dataclass(frozen=True, slots=True)
class Unsafe:
    """An explicitly unsafe operation."""


type Effect = (
    Console
    | File.Read
    | File.Write
    | Network
    | Clock
    | Random
    | Environment
    | Process
    | State.Read
    | State.Write
    | Unsafe
)

_EFFECT_TYPES = (
    Console,
    File.Read,
    File.Write,
    Network,
    Clock,
    Random,
    Environment,
    Process,
    State.Read,
    State.Write,
    Unsafe,
)


def _is_effect(value: object) -> TypeGuard[Effect]:
    """Return whether a value is a typed effect declaration."""
    return type(value) in _EFFECT_TYPES


def _canonical_name(value: Effect) -> str:
    """Convert a typed effect declaration to its stable string identifier."""
    match value:
        case Console():
            return "console"
        case File.Read():
            return "file.read"
        case File.Write():
            return "file.write"
        case Network():
            return "network"
        case Clock():
            return "clock"
        case Random():
            return "random"
        case Environment():
            return "environment"
        case Process():
            return "process"
        case State.Read():
            return "global.read"
        case State.Write():
            return "global.write"
        case Unsafe():
            return "unsafe"
        case _ as unreachable:
            assert_never(unreachable)


__all__ = [
    "Clock",
    "Console",
    "Effect",
    "Environment",
    "File",
    "Network",
    "Process",
    "Random",
    "State",
    "Unsafe",
]
