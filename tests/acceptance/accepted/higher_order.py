import efct


@efct.pure
def apply(function: efct.PureCallable[[int], int], value: int) -> int:
    selected = function
    return selected(value)


@efct.pure()
def increment(value: int) -> int:
    return value + 1


@efct.pure
def run(value: int) -> int:
    return apply(increment, value)


@efct.pure()
def answer() -> int:
    return 42


@efct.pure
def invoke(function: efct.PureCallable[[], int]) -> int:
    return function()


@efct.pure
def run_answer() -> int:
    return invoke(answer)
