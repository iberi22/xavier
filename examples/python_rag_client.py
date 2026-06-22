import requests
import os

XAVIER_URL = os.getenv("XAVIER_URL", "http://localhost:8006")
XAVIER_TOKEN = os.getenv("XAVIER_TOKEN", "your-secret-token")

headers = {
    "X-Xavier-Token": XAVIER_TOKEN,
    "Content-Type": "application/json"
}

def add_memory(text, user_id="example-user", metadata=None):
    url = f"{XAVIER_URL}/v1/memories"
    payload = {
        "text": text,
        "user_id": user_id,
        "metadata": metadata or {}
    }
    response = requests.post(url, json=payload, headers=headers)
    return response.json()

def search_memory(query, limit=5):
    url = f"{XAVIER_URL}/v1/memories/search"
    payload = {
        "query": query,
        "limit": limit
    }
    response = requests.post(url, json=payload, headers=headers)
    return response.json()

if __name__ == "__main__":
    # Ingestar algo de conocimiento
    print("Guardando memoria...")
    print(add_memory("Xavier es un backend RAG diseñado para agentes autónomos.", metadata={"topic": "documentation"}))

    # Realizar búsqueda
    print("\nBuscando...")
    results = search_memory("¿Qué es Xavier?")
    for res in results.get("results", []):
        print(f"[{res['id']}] {res['memory']}")
