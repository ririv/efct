import efct
import multiprocessing


@efct.pure
def start_process() -> None:
    multiprocessing.Process()
