from __future__ import annotations

import types
from collections.abc import Callable
from dataclasses import dataclass

from . import effect as effect_api
from . import partial as partial_api
from .i18n import localize_error_text
from .runtime import (
    EffectFunction,
    PureFunction,
    verify_effect,
    verify_pure,
    verify_record,
)

_MISSING = object()


@dataclass(frozen=True, slots=True)
class _PureContract:
    """Verify a function against an explicit partial bound."""

    declared_partials: tuple[str, ...]

    def __call__(
        self,
        function: types.FunctionType,
    ) -> PureFunction[..., object]:
        return verify_pure(function, self.declared_partials)


@dataclass(frozen=True, slots=True)
class _EffectContract:
    declared_effects: tuple[str, ...]

    def __call__(
        self,
        function: types.FunctionType,
    ) -> EffectFunction[..., object]:
        return verify_effect(function, self.declared_effects)


def pure(
    target_or_first_partial: (
        types.FunctionType | type[object] | str | partial_api.Partial | object
    ) = _MISSING,
    /,
    *additional_partials: str | partial_api.Partial,
) -> (
    PureFunction[..., object]
    | type[object]
    | Callable[[types.FunctionType], PureFunction[..., object]]
):
    """Verify inferred partial behavior or declare a concrete partial bound."""
    if type(target_or_first_partial) is types.FunctionType:
        if additional_partials:
            raise TypeError(
                localize_error_text(
                    "Bare @efct.pure does not accept partial arguments"
                )
            )
        return verify_pure(target_or_first_partial, None)
    if type(target_or_first_partial) is type:
        if additional_partials:
            raise TypeError(
                localize_error_text(
                    "Bare @efct.pure does not accept partial arguments"
                )
            )
        return verify_record(target_or_first_partial)
    if target_or_first_partial is _MISSING:
        partials_tuple: tuple[str, ...] = ()
    elif isinstance(target_or_first_partial, str):
        string_partials = [target_or_first_partial]
        for value in additional_partials:
            if not isinstance(value, str):
                raise TypeError(
                    localize_error_text(
                        "String and typed partial declarations cannot be mixed"
                    )
                )
            string_partials.append(value)
        if not all(partial_api._is_canonical_name(value) for value in string_partials):
            raise TypeError(
                localize_error_text(
                    "Pure string declarations must be supported partials"
                )
            )
        partials_tuple = tuple(string_partials)
    elif partial_api._is_partial(target_or_first_partial):
        typed_partials = [target_or_first_partial]
        for value in additional_partials:
            if not partial_api._is_partial(value):
                raise TypeError(
                    localize_error_text(
                        "String and typed partial declarations cannot be mixed"
                    )
                )
            typed_partials.append(value)
        partials_tuple = tuple(
            partial_api._canonical_name(value) for value in typed_partials
        )
    else:
        raise TypeError(
            localize_error_text(
                "Pure arguments must be strings or typed partial declarations"
            )
        )

    return _PureContract(partials_tuple)


def effects(
    first_effect: (
        types.FunctionType | str | effect_api.Effect | partial_api.Partial | object
    ) = _MISSING,
    /,
    *additional_effects: str | effect_api.Effect | partial_api.Partial,
) -> (
    EffectFunction[..., object]
    | Callable[[types.FunctionType], EffectFunction[..., object]]
):
    """Verify inferred effects or return a decorator with a concrete effect bound."""
    if type(first_effect) is types.FunctionType:
        if additional_effects:
            raise TypeError(
                localize_error_text(
                    "Bare @efct.effects does not accept effect arguments"
                )
            )
        return verify_effect(first_effect, None)
    if first_effect is _MISSING:
        effects_tuple: tuple[str, ...] = ()
    elif isinstance(first_effect, str):
        string_effects = [first_effect]
        for value in additional_effects:
            if not isinstance(value, str):
                raise TypeError(
                    localize_error_text(
                        "String and typed effect declarations cannot be mixed"
                    )
                )
            string_effects.append(value)
        effects_tuple = tuple(string_effects)
    elif effect_api._is_effect(first_effect) or partial_api._is_partial(first_effect):
        typed_effects = [first_effect]
        for value in additional_effects:
            if not (effect_api._is_effect(value) or partial_api._is_partial(value)):
                raise TypeError(
                    localize_error_text(
                        "String and typed effect declarations cannot be mixed"
                    )
                )
            typed_effects.append(value)
        effects_tuple = tuple(
            _canonical_declaration(value) for value in typed_effects
        )
    else:
        raise TypeError(
            localize_error_text(
                "Effect arguments must be strings, typed effects, or typed partials"
            )
        )

    return _EffectContract(effects_tuple)


def _canonical_declaration(
    value: effect_api.Effect | partial_api.Partial,
) -> str:
    if effect_api._is_effect(value):
        return effect_api._canonical_name(value)
    if partial_api._is_partial(value):
        return partial_api._canonical_name(value)
    raise AssertionError("Typed declaration validation is inconsistent")
