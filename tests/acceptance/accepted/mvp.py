import efct


@efct.pure
def add(x: int, y: int) -> int:
    return x + y


@efct.pure
def normalize(text: str) -> str:
    return text.strip().lower()


@efct.pure
def total(values: tuple[int, ...]) -> int:
    result = 0
    for value in values:
        result += value
    return result

