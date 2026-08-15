from collections.abc import Callable
from typing import Generic, ParamSpec, TypeVar

from .certificates import VerificationCertificate

_P = ParamSpec("_P")
_R = TypeVar("_R")
_R_co = TypeVar("_R_co", covariant=True)
_T = TypeVar("_T")


class PureFunction(Generic[_P, _R_co]):
    @property
    def certificate(self) -> VerificationCertificate: ...

    @property
    def __name__(self) -> str: ...

    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _R_co: ...


class EffectFunction(Generic[_P, _R_co]):
    @property
    def certificate(self) -> VerificationCertificate: ...

    @property
    def __name__(self) -> str: ...

    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _R_co: ...


def verify_pure(
    function: Callable[_P, _R],
    declared_partials: tuple[str, ...] | None,
) -> PureFunction[_P, _R]: ...
def verify_effect(
    function: Callable[_P, _R],
    declared_effects: tuple[str, ...] | None,
) -> EffectFunction[_P, _R]: ...
def verify_record(record: type[_T]) -> type[_T]: ...
