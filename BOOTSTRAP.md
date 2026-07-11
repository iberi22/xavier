# BOOTSTRAP.md — Instrucciones de Primera Ejecución

> Guía para un agente nuevo en el proyecto Xavier.
> Qué leer, qué configurar, qué verificar antes de empezar a trabajar.

---

## 1. 🧠 Conoce el Proyecto (Lectura Obligatoria)

Lee estos archivos EN ESTE ORDEN:

### Identidad y Contexto
| Archivo | Qué contiene | Tiempo estimado |
|---------|-------------|-----------------|
| `SOUL.md` | Quién eres (Xavier CEO de SWAL) | 2 min |
| `USER.md` | Quién es BELA (el humano) | 2 min |
| `AGENTS.md` | Cómo funciona el workspace | 5 min |
| `README.md` | Vista general del proyecto | 3 min |
| `MEMORY.md` | Decisiones pasadas y lecciones | 5 min |

### Tarea Actual
| Archivo | Propósito |
|---------|-----------|
| `CLAUDECODE_TASK.md` | Tarea activa del sprint actual |
| `TASK.md` | Plantilla de seguimiento |

### Técnico
```bash
# Feature tracking
cat .gitcore/features.json

# Architecture
cat docs/ARCHITECTURE.md

# API Reference
cat docs/API.md
```

---

## 2. ⚙️ Verifica el Entorno

```bash
# 1. ¿Xavier está corriendo?
curl http://localhost:8006/health

# 2. ¿Compila?
cargo check --workspace

# 3. ¿Tests pasan?
cargo test --lib

# 4. ¿Token configurado?
echo $XAVIER_TOKEN
# Si no: export XAVIER_TOKEN="$(cat .env | grep XAVIER_TOKEN | cut -d= -f2)"

# 5. ¿Git actualizado?
git status
git pull

# 6. ¿Docker corriendo?
docker compose ps
```

### Si algo falla:
| Problema | Solución |
|----------|----------|
| Xavier no responde | `./start-xavier-rag.ps1` o `docker compose up -d xavier` |
| `cargo check` falla | Revisar errores de compilación, probablemente falta crate |
| Token no existe | `xavier token new` para generar |
| Tests fallan | Revisar si son pre-existentes (consultar MEMORY.md) |

---

## 3. 🧠 Conecta con Xavier (Memoria Central)

Xavier es tu cerebro persistente. Todo agente nuevo debe:

```bash
# 1. Buscar contexto antes de trabajar
curl -X POST http://localhost:8006/v1/memories/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"current state of project", "limit":10}'

# 2. Entender qué features están activos
# Revisar .gitcore/features.json para features con status stable/beta

# 3. Leer los últimos devlogs
ls docs/devlog/ | tail -5
```

---

## 4. 🔄 Workflow Diario

### Inicio de Sesión
1. Leer `SOUL.md` + `USER.md` + `AGENTS.md` + `MEMORY.md`
2. Verificar servidor Xavier
3. Buscar contexto en Xavier
4. Revisar `CLAUDECODE_TASK.md`
5. Verificar `cargo check` pasa
6. Empezar a trabajar

### Durante el Trabajo
- Usa `POST /memory/search` frecuentemente para contexto
- Persiste decisiones en `POST /memory/add`
- `cargo check` cada ~30 min
- Commits atómicos con mensajes convencionales

### Fin de Sesión
1. Persistir resultados en Xavier
2. Actualizar `MEMORY.md` si hay nuevas decisiones
3. Actualizar `TASK.md` con progreso
4. `git commit` + `git push`
5. Si aplica, crear GitHub Issue para bugs/features

---

## 5. 📚 Referencias Rápidas

### Comandos más usados
```bash
cargo check                    # Check compilación
cargo test --lib               # Tests unitarios
cargo clippy -- -D warnings    # Lint
cargo run -- http 8006         # Iniciar servidor
curl localhost:8006/health     # Health check
```

### Archivos que NO debes modificar
- `.gitcore/` — Protocolo interno (solo lectura)
- `Cargo.lock` — Lo maneja cargo
- `docs/site/` — Generado automáticamente
- `.github/workflows/` — CI/CD pipelines

### Archivos que SÍ debes mantener actualizados
- `MEMORY.md` — Nuevas decisiones
- `TASK.md` — Progreso de tareas
- `CLAUDECODE_TASK.md` — Tarea activa
- Scripts que toques

---

## 6. 🚨 Reglas de Oro

1. **Memory first** — Siempre busca en Xavier antes de decidir
2. **Tests pass** — Nunca rompas tests existentes
3. **Atomic commits** — `feat(scope): message (closes #N)`
4. **Documenta** — Si no está documentado, no existe
5. **Pregunta** — Si no entiendes algo, busca en Xavier o pregunta a BELA

---

## 7. 📞 Contacto

- **BELA:** @BeRi0n3 (Telegram)
- **Repo:** https://github.com/iberi22/xavier
- **Issues:** https://github.com/iberi22/xavier/issues

---

_¡Bienvenido a Xavier! Construyamos el futuro de la memoria para agentes AI._
_Última actualización: 2026-07-09_
