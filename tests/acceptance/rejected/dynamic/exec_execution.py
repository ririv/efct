import efct


@efct.pure
def execute(source: str) -> None:
    exec(source)
