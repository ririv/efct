from pathlib import Path

import efct


@efct.pure
def read_file(path: str) -> None:
    Path(path).read_text()
