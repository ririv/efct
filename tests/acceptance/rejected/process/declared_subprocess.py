import efct
import subprocess


@efct.effects("process")
def run(command: str) -> None:
    subprocess.run((command,))
