import efct


@efct.pure()
def item(mapping: efct.FrozenMap[str, int], key: str) -> int:
    return mapping[key]
