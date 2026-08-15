from __future__ import annotations

from dataclasses import dataclass
from typing import Generic, Iterator, Never, TypeVar

from .i18n import localize_error_text

_T_co = TypeVar("_T_co", covariant=True)
_E_co = TypeVar("_E_co", covariant=True)
_K = TypeVar("_K")
_V = TypeVar("_V")
_PURE_RECORD_FIELDS: dict[type[object], tuple[str, ...]] = {}


class Result(Generic[_T_co, _E_co]):
    """Result type marker for use in annotations."""

    __slots__ = ()

    def __new__(cls, *args: object, **kwargs: object) -> Result[_T_co, _E_co]:
        if cls is Result:
            raise TypeError(localize_error_text("Result must be constructed with Ok or Err"))
        return object.__new__(cls)


@dataclass(frozen=True, slots=True)
class Ok(Result[_T_co, Never], Generic[_T_co]):
    """The successful Result variant."""

    value: _T_co


@dataclass(frozen=True, slots=True)
class Err(Result[Never, _E_co], Generic[_E_co]):
    """The failed Result variant."""

    error: _E_co


class FrozenMap(Generic[_K, _V]):
    """An immutable insertion-ordered mapping that rejects duplicate keys."""

    __slots__ = ("__items",)

    def __init__(self, items: tuple[tuple[_K, _V], ...]) -> None:
        if type(items) is not tuple:
            raise TypeError(localize_error_text("The FrozenMap constructor only accepts a tuple"))
        checked: list[tuple[_K, _V]] = []
        for item in items:
            if type(item) is not tuple or len(item) != 2:
                raise TypeError(localize_error_text("Each FrozenMap item must be a two-element tuple"))
            key, value = item
            if not _is_pure_value(key) or not _is_pure_value(value):
                raise TypeError(localize_error_text("FrozenMap keys and values must be deeply immutable pure values"))
            if any(existing_key == key for existing_key, _ in checked):
                raise ValueError(localize_error_text("FrozenMap does not allow duplicate keys"))
            checked.append((key, value))
        self.__items = tuple(checked)

    def __len__(self) -> int:
        return len(self.__items)

    def __getitem__(self, key: _K) -> _V:
        for existing_key, value in self.__items:
            if existing_key == key:
                return value
        raise KeyError(key)

    def __iter__(self) -> Iterator[_K]:
        return (key for key, _ in self.__items)

    def __eq__(self, other: object) -> bool:
        return type(other) is FrozenMap and self.__items == other.__items

    def __hash__(self) -> int:
        return hash(self.__items)


def _is_pure_value(value: object) -> bool:
    if type(value) in (type(None), bool, int, str, bytes):
        return True
    if type(value) is tuple or type(value) is frozenset:
        return all(_is_pure_value(item) for item in value)
    if type(value) is Ok:
        return _is_pure_value(value.value)
    if type(value) is Err:
        return _is_pure_value(value.error)
    if type(value) is FrozenMap:
        return all(_is_pure_value(key) and _is_pure_value(item) for key, item in value.__items)
    fields = _PURE_RECORD_FIELDS.get(type(value))
    if fields is not None:
        return all(_is_pure_value(getattr(value, field)) for field in fields)
    return False


def _register_pure_record(record: type[object], fields: tuple[str, ...]) -> None:
    if record in _PURE_RECORD_FIELDS:
        raise TypeError(localize_error_text(f"Pure record {record.__name__} is already registered"))
    _PURE_RECORD_FIELDS[record] = fields


def _pure_record_fields(record: type[object]) -> tuple[str, ...] | None:
    return _PURE_RECORD_FIELDS.get(record)
