import efct
import sqlite3


@efct.pure
def connect(path: str) -> None:
    sqlite3.connect(path)
