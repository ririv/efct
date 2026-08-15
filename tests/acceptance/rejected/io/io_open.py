import efct
import io


@efct.pure
def read_file(path: str) -> None:
    io.open(path)
