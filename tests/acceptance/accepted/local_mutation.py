import efct


@efct.pure
def total(value: int) -> int:
    values = [1, 2]
    alias = values
    alias.append(value)
    return sum(values) + len(alias)
