import efct


class ConfigError(ValueError):
    pass


@efct.pure
def inferred(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value


@efct.pure()
def exact(value: int) -> int:
    return value + 1


@efct.pure(efct.partial.Raise(ValueError))
def bounded(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value


@efct.pure()
def handled(message: str) -> int:
    try:
        raise ValueError(message)
    except ValueError:
        return 0


@efct.pure(efct.partial.Raise(ConfigError))
def custom(message: str) -> None:
    raise ConfigError(message)


@efct.pure()
def recover_custom(message: str) -> int:
    try:
        custom(message)
    except ConfigError:
        return 0
    return 1


@efct.pure("raise:partial_contracts.ConfigError")
def custom_string(message: str) -> None:
    raise ConfigError(message)


@efct.pure()
def recover_custom_string(message: str) -> int:
    try:
        custom_string(message)
    except ValueError:
        return 0
    return 1


@efct.pure(efct.partial.Raise(AssertionError))
def require(condition: bool) -> int:
    assert condition, "required"
    return 1


@efct.pure()
def recover_assertion(condition: bool) -> str:
    try:
        require(condition)
    except AssertionError as error:
        return str(error)
    return "valid"


@efct.pure()
def divide_by_literal(value: int) -> int:
    return value // 2


@efct.pure(efct.partial.Raise(ZeroDivisionError))
def quotient(value: int, divisor: int) -> int:
    return value // divisor


@efct.pure()
def recover_division(value: int, divisor: int) -> int:
    try:
        return value % divisor
    except ArithmeticError:
        return 0


@efct.pure()
def last_pair(values: tuple[int, str]) -> str:
    return values[-1]


@efct.pure(efct.partial.Raise(IndexError))
def tuple_item(values: tuple[int, ...], index: int) -> int:
    return values[index]


@efct.pure()
def recover_index(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        return 0


@efct.pure(efct.partial.Raise(KeyError))
def map_item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]


@efct.pure()
def recover_key(mapping: efct.FrozenMap[str, int], key: str) -> int:
    try:
        return mapping[key]
    except LookupError:
        return 0


@efct.pure()
def distinct_map() -> efct.FrozenMap[str, int]:
    return efct.FrozenMap((("left", 1), ("right", 2)))


@efct.pure(efct.partial.Raise(ValueError))
def make_map(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))


@efct.pure()
def recover_duplicate(key: str) -> efct.FrozenMap[str, int]:
    try:
        return make_map(key, key)
    except ValueError:
        return efct.FrozenMap(((key, 0),))


@efct.pure()
def recover_static_index() -> int:
    try:
        return ()[0] + print("unreachable")  # pyright: ignore[reportGeneralTypeIssues]
    except IndexError:
        return 0


@efct.pure(efct.partial.Raise(IndexError))
def conditional_index(flag: bool) -> int:
    return ()[0] if flag else 1  # pyright: ignore[reportGeneralTypeIssues]
