import efct
import io


@efct.effects("file.read")
def read_file(path: str, mode: str) -> None:
    io.open(path, mode)
