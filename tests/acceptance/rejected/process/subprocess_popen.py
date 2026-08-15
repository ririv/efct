import efct
import subprocess


@efct.pure
def run(command: str) -> None:
    subprocess.Popen((command,))
