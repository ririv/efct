import efct


@efct.pure
def bad(value: int) -> int:
    print(value)
    return value
