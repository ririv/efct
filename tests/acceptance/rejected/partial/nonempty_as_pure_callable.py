import efct


@efct.pure()
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    return function(value)


@efct.pure(efct.partial.Raise(ValueError))
def reject(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value


@efct.pure
def run(value: int) -> int:
    return apply(reject, value)
