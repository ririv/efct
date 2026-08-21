from efct import pure


@pure()
def response_size(body: str) -> int:
    return len(body)
