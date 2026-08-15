import efct


@efct.pure
def compile_source(source: str) -> None:
    compile(source, "<dynamic>", "exec")
