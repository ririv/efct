import efct


@efct.pure()
def item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        raise
