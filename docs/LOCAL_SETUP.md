# Configuración Local de Xavier

Esta guía describe cómo configurar Xavier para operar de forma 100% local, garantizando la privacidad y el funcionamiento sin conexión a internet.

## Requisitos Previos

- Rust (última versión estable)
- SQLite 3.x

## Vector Store (Almacenamiento Vectorial)

Xavier utiliza un sistema de almacenamiento vectorial 100% local basado en **SQLite** y la extensión **sqlite-vec**. Esto elimina la necesidad de servicios cloud como Pinecone o bases de datos externas como Qdrant.

- **Ubicación de datos**: Por defecto, los vectores se guardan en `vec-store.sqlite3`.
- **Privacidad**: Tus embeddings nunca salen de tu máquina.
- **Rendimiento**: Optimizado para búsquedas rápidas mediante el modo WAL de SQLite y mapeo de memoria (mmap).

Para más detalles técnicos sobre esta decisión arquitectónica, consulta el [ADR-006: 100% Local Vector Store with SQLite-Vec](./ADR/006-vector-store-local-sqlite-vec.md).

## Embeddings Locales

(Sección a completar en futuros pasos del proyecto LOCAL1)

## Modelos de Lenguaje Locales (LLMs)

(Sección a completar en futuros pasos del proyecto LOCAL1)
