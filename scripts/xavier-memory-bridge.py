#!/usr/bin/env python3
"""
Xavier Memory Bridge — Dogfooding tool.
Conecta Hermes (este agente) con Xavier como backend de memoria alternativa.
Uso: python3 xavier-memory-bridge.py <comando> [args]

Comandos:
  add <content> [path] [metadata_json]   — Guardar memoria
  search <query> [limit]                 — Buscar memorias
  stats                                  — Estadísticas de memoria
  session-save [session_id]              — Guardar resumen de sesión actual
  health                                 — Health check del servidor
  login <email> <password>               — Obtener JWT
"""
import json
import os
import sys
import urllib.request
import urllib.error
from datetime import datetime

# Configuración
CONFIG = {
    "base_url": os.environ.get("XAVIER_URL", "http://192.168.1.2:8006"),
    "token": os.environ.get("XAVIER_TOKEN", ""),
    "jwt": os.environ.get("XAVIER_JWT", ""),
    "auth_email": os.environ.get("XAVIER_AUTH_EMAIL", "bela@swal.dev"),
    "auth_password": os.environ.get("XAVIER_AUTH_PASSWORD", "dev-token"),
}

# Cache de JWT
_jwt_token = CONFIG["jwt"]

def api(method, path, data=None, use_auth=True):
    """Llamada a la API de Xavier."""
    url = CONFIG["base_url"] + path
    headers = {"Content-Type": "application/json"}

    if use_auth:
        # Priority: root token (XAVIER_TOKEN) > JWT > fail
        if CONFIG["token"]:
            headers["X-Xavier-Token"] = CONFIG["token"]
        elif _jwt_token:
            headers["Authorization"] = f"Bearer {_jwt_token}"

    body = json.dumps(data).encode() if data else None
    req = urllib.request.Request(url, data=body, headers=headers, method=method)

    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.status, json.loads(resp.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())
    except Exception as e:
        return 0, {"error": str(e)}


def login():
    """Obtener JWT token de Xavier."""
    global _jwt_token
    status, data = api("POST", "/auth/login", {
        "email": CONFIG["auth_email"],
        "password": CONFIG["auth_password"],
    }, use_auth=False)

    if status == 200 and "access_token" in data:
        _jwt_token = data["access_token"]
        # También guardar en env var para próximas llamadas
        os.environ["XAVIER_JWT"] = _jwt_token
        print(f"✅ Login OK — JWT obtenido ({len(_jwt_token)} chars)")
        return _jwt_token
    else:
        print(f"❌ Login falló: {data}")
        return None


def ensure_auth():
    """Asegurar que tenemos un JWT válido."""
    if not _jwt_token:
        return login()
    return _jwt_token


def cmd_health():
    """Health check del servidor."""
    status, data = api("GET", "/health", use_auth=False)
    if status == 200:
        print(f"✅ Xavier HEALTH: {data.get('status', 'unknown')}")
        print(f"   Uptime: {data.get('system', {}).get('uptime_secs', '?')}s")
        print(f"   DB pages: {data.get('database', {}).get('page_count', '?')}")
        print(f"   Embedding: {data.get('embedding', {}).get('status', '?')}")
        print(f"   TGD: {data.get('tgd_consolidation', {}).get('status', '?')} "
              f"({data.get('tgd_consolidation', {}).get('processed', 0)}/"
              f"{data.get('tgd_consolidation', {}).get('total', '?')})")
    else:
        print(f"❌ Health check falló: {data}")


def cmd_add(content, path=None, metadata=None):
    """Añadir memoria a Xavier."""
    ensure_auth()
    payload = {"content": content}
    if path:
        payload["path"] = path
    if metadata:
        payload["metadata"] = metadata

    status, data = api("POST", "/memory/add", payload)
    if status in (200, 201):
        result_path = data.get("path", path or "(auto)")
        print(f"✅ Memoria guardada → {result_path}")
        return data
    else:
        print(f"❌ Error al guardar: {data}")
        return None


def cmd_search(query, limit=10):
    """Buscar memorias en Xavier."""
    ensure_auth()
    status, data = api("POST", "/memory/search", {
        "query": query,
        "limit": limit,
    })
    if status == 200:
        results = data.get("results", data.get("memories", []))
        print(f"🔍 '{query}' → {len(results)} resultados:")
        for i, r in enumerate(results[:limit], 1):
            content = r.get("content", r.get("text", ""))[:120]
            r_path = r.get("path", r.get("id", "?"))
            score = r.get("score", r.get("relevance", "?"))
            print(f"  {i}. [{score}] {r_path}")
            print(f"     {content}...")
        return results
    else:
        print(f"❌ Error en búsqueda: {data}")
        return []


def cmd_stats():
    """Estadísticas de memoria."""
    ensure_auth()
    status, data = api("GET", "/memory/stats")
    if status == 200:
        print(f"📊 Memory Stats:")
        for k, v in data.items():
            print(f"   {k}: {v}")
    else:
        print(f"❌ Error: {data}")


def cmd_session_save(session_id=None):
    """Guardar metadata de la sesión actual en Xavier."""
    ensure_auth()
    session_info = {
        "agent": "Hermes (deepseek-v4-flash)",
        "timestamp": datetime.now().isoformat(),
        "session_id": session_id or datetime.now().strftime("%Y%m%d_%H%M%S"),
        "project": "xavier-dogfooding",
        "description": "Sesión de desarrollo y dogfooding con Xavier",
    }
    path = f"session/hermes/{session_info['session_id']}"
    status, data = api("POST", "/memory/add", {
        "content": json.dumps(session_info),
        "path": path,
        "metadata": {"type": "session", "source": "hermes"},
    })
    if status in (200, 201):
        print(f"✅ Sesión guardada → {path}")
    else:
        print(f"❌ Error: {data}")


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    cmd = sys.argv[1]

    if cmd == "health":
        cmd_health()
    elif cmd == "login":
        email = sys.argv[2] if len(sys.argv) > 2 else CONFIG["auth_email"]
        password = sys.argv[3] if len(sys.argv) > 3 else CONFIG["auth_password"]
        CONFIG["auth_email"] = email
        CONFIG["auth_password"] = password
        login()
    elif cmd == "add":
        content = sys.argv[2] if len(sys.argv) > 2 else input("Content: ")
        path = sys.argv[3] if len(sys.argv) > 3 else None
        metadata = json.loads(sys.argv[4]) if len(sys.argv) > 4 else None
        cmd_add(content, path, metadata)
    elif cmd == "search":
        query = sys.argv[2] if len(sys.argv) > 2 else input("Query: ")
        limit = int(sys.argv[3]) if len(sys.argv) > 3 else 10
        cmd_search(query, limit)
    elif cmd == "stats":
        cmd_stats()
    elif cmd == "session-save":
        session_id = sys.argv[2] if len(sys.argv) > 2 else None
        cmd_session_save(session_id)
    else:
        print(f"❌ Comando desconocido: {cmd}")
        print(__doc__)
        sys.exit(1)


if __name__ == "__main__":
    main()
