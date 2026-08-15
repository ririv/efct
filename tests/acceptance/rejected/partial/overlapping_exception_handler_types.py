import efct


@efct.pure()
def invalid(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except (LookupError, IndexError):
        return 0
