import efct


@efct.pure()
def invalid() -> None:
    try:
        value = 1
    except ValueError:
        pass
    else:
        raise ValueError("else")
