import efct
import ssl


@efct.pure
def create_context() -> None:
    ssl.create_default_context()
