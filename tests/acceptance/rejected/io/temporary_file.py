import efct
import tempfile


@efct.pure
def create_temporary_file() -> None:
    tempfile.NamedTemporaryFile()
