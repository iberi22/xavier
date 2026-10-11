def fetch(url, timeout):
    if timeout <= 0:
        return None
    data = get(url)
    return data if data else ""
