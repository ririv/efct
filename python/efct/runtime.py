from __future__ import annotations

import types

from . import _core

PureFunction = _core.PureFunction
EffectFunction = _core.EffectFunction


def verify_pure(
    function: types.FunctionType,
    declared_partials: tuple[str, ...] | None,
) -> PureFunction:
    """调用原生启动验证器并返回已验证纯函数。"""
    return _core.verify_pure(function, declared_partials)


def verify_effect(
    function: types.FunctionType,
    declared_effects: tuple[str, ...] | None,
) -> EffectFunction:
    """调用原生启动验证器并返回已验证效果函数。"""
    return _core.verify_effect(function, declared_effects)


def verify_record(record: type[object]) -> type[object]:
    """调用原生启动验证器登记纯记录。"""
    return _core.verify_record(record)
