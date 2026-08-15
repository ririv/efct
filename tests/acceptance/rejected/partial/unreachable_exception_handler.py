import efct


@efct.pure()
def recover(message: str) -> int:
    try:
        raise ValueError(message)
    except Exception:
        return 0
    except ValueError:
        return 1
