from typing import ParamSpec, Protocol, TypeVar

_P = ParamSpec("_P")
_R_co = TypeVar("_R_co", covariant=True)


class EffectSet: ...


_E_co = TypeVar("_E_co", bound=EffectSet, covariant=True)


class PureCallable(Protocol[_P, _R_co]):
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _R_co: ...


class EffectCallable(Protocol[_P, _R_co, _E_co]):
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _R_co: ...
