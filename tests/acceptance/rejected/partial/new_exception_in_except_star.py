import efct


@efct.pure(efct.partial.Raise(TypeError))
def invalid() -> None:
    try:
        raise ValueError("value")
    except* ValueError:
        raise TypeError("new")
