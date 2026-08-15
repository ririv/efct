import efct
import socket


@efct.pure
def connect(host: str) -> None:
    socket.create_connection((host, 443))
