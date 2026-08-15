import efct


@efct.pure
def load_module(name: str) -> None:
    __import__(name)
