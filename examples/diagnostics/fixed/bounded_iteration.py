from efct import pure


@pure()
def count_steps(limit: int) -> int:
    steps = 0
    for _ in range(limit):
        steps += 1
    return steps
