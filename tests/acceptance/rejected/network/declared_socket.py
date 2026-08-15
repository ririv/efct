import efct
import socket


@efct.effects("network")
def connect(host: str) -> None:
    socket.create_connection((host, 443))
