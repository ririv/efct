import efct

started = True
print("ordinary module")


@efct.pure
def identity(value: int) -> int:
    return value
