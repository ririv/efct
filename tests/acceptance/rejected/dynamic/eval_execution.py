import efct


@efct.pure
def evaluate(source: str) -> None:
    eval(source)
