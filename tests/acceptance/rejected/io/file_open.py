import efct


@efct.pure
def read_file(path: str) -> None:
    open(path)
