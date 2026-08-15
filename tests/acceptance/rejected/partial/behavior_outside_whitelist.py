import efct


@efct.pure("raise:builtins.TypeError")
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
