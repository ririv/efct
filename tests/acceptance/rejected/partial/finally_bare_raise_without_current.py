import efct


@efct.pure()
def invalid() -> None:
    try:
        pass
    finally:
        raise
