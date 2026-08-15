import efct


@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    return values[index]
