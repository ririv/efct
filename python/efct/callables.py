import types

from .i18n import localize_error_text


class PureCallable:
    """Type marker for a verified pure-function capability."""

    __slots__ = ()

    def __new__(cls) -> "PureCallable":
        raise TypeError(localize_error_text("PureCallable may only be used in type annotations"))

    def __class_getitem__(cls, parameters: object) -> types.GenericAlias:
        return types.GenericAlias(cls, parameters)


class EffectCallable:
    """Type marker for a verified function capability with instantiable effects."""

    __slots__ = ()

    def __new__(cls) -> "EffectCallable":
        raise TypeError(localize_error_text("EffectCallable may only be used in type annotations"))

    def __class_getitem__(cls, parameters: object) -> types.GenericAlias:
        return types.GenericAlias(cls, parameters)


class EffectSet:
    """Kind marker for an effect-generic parameter."""

    __slots__ = ()

    def __new__(cls) -> "EffectSet":
        raise TypeError(localize_error_text("EffectSet may only be used as an effect-generic parameter constraint"))
