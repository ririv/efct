from efct import partial, pure


@pure(partial.Raise(ValueError))
def require_non_negative(value: int) -> int:
    if value < 0:
        raise ValueError("value must be non-negative")
    return value
