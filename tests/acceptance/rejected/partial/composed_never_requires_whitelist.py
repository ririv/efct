import efct


@efct.pure()
def invalid() -> int:
    return ()[0] + print("unreachable")
