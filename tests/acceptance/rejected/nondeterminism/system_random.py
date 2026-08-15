import efct
import os


@efct.pure
def random_bytes() -> None:
    os.urandom(16)
