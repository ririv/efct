import efct


@efct.pure()
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value
