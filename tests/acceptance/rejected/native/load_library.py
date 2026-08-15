import ctypes

import efct


@efct.pure
def load_library(path: str) -> None:
    ctypes.CDLL(path)
