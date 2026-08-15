import efct

_efct = efct.pure
print("module initialization must be pure")


@efct.pure
def identity(value: int) -> int:
    return value
