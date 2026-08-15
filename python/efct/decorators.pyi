from collections.abc import Callable
from types import FunctionType
from typing import ParamSpec, Protocol, TypeVar, overload

from .effect import Effect
from .partial import Partial
from .runtime import EffectFunction, PureFunction

_P = ParamSpec("_P")
_R = TypeVar("_R")
_T = TypeVar("_T")
_F = TypeVar("_F", bound=FunctionType)


class _EffectDecorator(Protocol):
    def __call__(
        self,
        function: Callable[_P, _R],
        /,
    ) -> EffectFunction[_P, _R]: ...


class _PureDecorator(Protocol):
    def __call__(
        self,
        function: Callable[_P, _R],
        /,
    ) -> PureFunction[_P, _R]: ...


@overload
def pure(record: type[_T], /) -> type[_T]: ...


@overload
def pure(function: _F, /) -> _F: ...


@overload
def pure() -> _PureDecorator: ...


@overload
def pure(
    first_partial: str,
    /,
    *additional_partials: str,
) -> _PureDecorator: ...


@overload
def pure(
    first_partial: Partial,
    /,
    *additional_partials: Partial,
) -> _PureDecorator: ...


@overload
def effects(function: Callable[_P, _R], /) -> EffectFunction[_P, _R]: ...


@overload
def effects() -> _EffectDecorator: ...


@overload
def effects(
    first_effect: str,
    /,
    *additional_effects: str,
) -> _EffectDecorator: ...


@overload
def effects(
    first_effect: Effect,
    /,
    *additional_effects: Effect | Partial,
) -> _EffectDecorator: ...


@overload
def effects(
    first_effect: Partial,
    /,
    *additional_effects: Effect | Partial,
) -> _EffectDecorator: ...
