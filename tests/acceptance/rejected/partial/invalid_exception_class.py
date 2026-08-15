import efct


class InvalidError(int):
    pass


@efct.pure
def identity(value: int) -> int:
    return value
