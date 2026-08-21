import requests
from efct import pure


@pure()
def download(url: str) -> None:
    requests.get(url)
