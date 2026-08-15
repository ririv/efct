import efct
import os


@efct.pure
def remove_file(path: str) -> None:
    os.remove(path)
