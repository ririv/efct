import efct


@efct.effects(efct.effect.File.Read())
def invalid(path: str) -> None:
    with open(path):
        pass
