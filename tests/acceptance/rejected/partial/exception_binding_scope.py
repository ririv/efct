import efct


@efct.pure()
def invalid(message: str) -> str:
    try:
        raise ValueError(message)
    except ValueError as error:
        pass
    return str(error)  # pyright: ignore[reportUnboundVariable]
