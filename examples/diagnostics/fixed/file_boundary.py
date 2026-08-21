from efct import effect, effects, partial


@effects(
    effect.File.Read(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def probe_file(path: str) -> None:
    open(path)
