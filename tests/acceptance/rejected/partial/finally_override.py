import efct


@efct.pure()
def invalid() -> None:
    try:
        raise ValueError("value")
    finally:
        raise TypeError("cleanup")
