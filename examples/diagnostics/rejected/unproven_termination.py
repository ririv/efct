from efct import pure


@pure()
def wait_forever() -> None:
    while True:
        pass
