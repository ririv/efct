from efct import pure


@pure()
def probe_file(path: str) -> None:
    open(path)
