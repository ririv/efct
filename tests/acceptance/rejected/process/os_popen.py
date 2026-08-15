import efct
import os


@efct.pure
def run(command: str) -> None:
    os.popen(command)
