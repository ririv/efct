import efct


@efct.pure()
def build(first: str, second: str) -> efct.FrozenMap[str, int]:
    return efct.FrozenMap(((first, 1), (second, 2)))
