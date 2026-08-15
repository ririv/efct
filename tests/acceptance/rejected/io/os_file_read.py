import efct
import os


@efct.pure
def list_directory(path: str) -> None:
    os.listdir(path)
