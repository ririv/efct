import efct
import os


@efct.pure
def write_environment(name: str, value: str) -> None:
    os.putenv(name, value)
