from efct import pure


@pure()
def require_non_negative(value: int) -> int:
    if value < 0:
        raise ValueError("value must be non-negative")
    return value
