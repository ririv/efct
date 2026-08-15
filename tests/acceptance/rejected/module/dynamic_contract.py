import efct

CONTRACT: str = "pure"
_efct = CONTRACT


@efct.pure
def identity(value: int) -> int:
    return value
