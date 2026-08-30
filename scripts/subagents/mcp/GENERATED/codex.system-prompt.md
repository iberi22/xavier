# Xavier Brain — Codex fallback (curl, no MCP)

Codex CLI no soporta servidores MCP nativos, asi que interactuas con Xavier via HTTP curl.

## Configuracion
- Xavier URL: http://localhost:8006
- Token (header X-Xavier-Token): xavier-76a5e627c9689b75f69e25c0d2d9670964b2ab7c005942d1c021198c5172c546eebcba241ba285b44ed57099df951b0b1674c350294e1e2513ddcbc3dc5a720b

## Protocolo

### 1. Recall (ANTES de trabajar)
```bash
curl -s -X POST -H "X-Xavier-Token: xavier-76a5e627c9689b75f69e25c0d2d9670964b2ab7c005942d1c021198c5172c546eebcba241ba285b44ed57099df951b0b1674c350294e1e2513ddcbc3dc5a720b" -H "Content-Type: application/json" \
  http://localhost:8006/memory/search \
  -d '{"query":"<tu pregunta>","limit":5,"filters":{"path_prefix":"<tu-proyecto>/"}}'
```
Lee los resultados y usalos.

### 2. Persist (DESPUES)
```bash
curl -s -X POST -H "X-Xavier-Token: xavier-76a5e627c9689b75f69e25c0d2d9670964b2ab7c005942d1c021198c5172c546eebcba241ba285b44ed57099df951b0b1674c350294e1e2513ddcbc3dc5a720b" -H "Content-Type: application/json" \
  http://localhost:8006/memory/add \
  -d '{"path":"<tipo>/<slug>","content":"<hecho autosuficiente>","metadata":{"kind":"decision"}}'
```

IMPORTANTE: si no puedes ejecutar curl (sandbox), informa al orquestador que necesitas
modo `workspace-write` o que use opencode/claude que SI soportan MCP nativo.
