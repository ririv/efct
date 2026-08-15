import efct
import os


@efct.pure
def reflect(name: str) -> None:
    getattr(os, name)
