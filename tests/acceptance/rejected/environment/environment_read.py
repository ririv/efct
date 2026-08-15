import efct
import os


@efct.pure
def read_environment(name: str) -> str:
    return os.getenv(name, "")
