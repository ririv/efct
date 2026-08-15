import efct


@efct.pure()
def invalid() -> int:
    try:
        raise ValueError("value")
    except ValueError:
        print("reachable")
        return 0
