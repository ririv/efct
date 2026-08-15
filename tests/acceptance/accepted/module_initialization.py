import efct

_efct = efct.pure


@efct.pure
def identity(value: int) -> int:
    return value
