# ADR-006: 100% Local Vector Store with SQLite-Vec

*Status: ACCEPTED | Date: 2026-05-10*

---

## Contexto

Xavier requiere una capa de almacenamiento y recuperación de embeddings (vectores) para soportar su memoria semántica y capacidades de RAG (Retrieval-Augmented Generation). El objetivo principal del proyecto es ser "100% local-first", permitiendo que el agente funcione de forma privada, offline y sin dependencias de servicios en la nube para el almacenamiento de datos sensibles.

Tradicionalmente, los sistemas de IA utilizan bases de datos vectoriales dedicadas o servicios gestionados, lo que introduce complejidad operativa y dependencias externas.

---

## Decisión

Hemos decidido utilizar **SQLite** junto con la extensión **sqlite-vec** para todas las necesidades de almacenamiento vectorial.

- **Tecnología**: SQLite + `sqlite-vec` (extensión de búsqueda vectorial extremadamente ligera y rápida).
- **Modo**: 100% local, embebido en el binario de Xavier.
- **Formato**: Los vectores se almacenan en tablas virtuales `vec0` dentro de una base de datos SQLite estándar.

Esta decisión se alinea con la meta de Xavier de tener "cero configuración" y ser totalmente portable.

---

## Alternativas Consideradas

1.  **Pinecone / Milvus / Qdrant (Cloud)**:
    - *Pros*: Escalabilidad infinita, gestión delegada.
    - *Contras*: Requieren conexión a internet, introducen latencia de red, comprometen la privacidad "local-first" y tienen costes asociados. **Rechazado** por violar los principios de Xavier.
2.  **pgvector (PostgreSQL)**:
    - *Pros*: Muy maduro y potente.
    - *Contras*: Requiere correr un servidor de base de datos externo (Docker o instalación local), lo que rompe la experiencia de "un solo binario". **Rechazado** por complejidad operativa.
3.  **In-Memory (Faiss / HNSWlib)**:
    - *Pros*: Velocidad máxima.
    - *Contras*: Los datos se pierden al cerrar la aplicación o requieren procesos complejos de serialización/deserialización a disco. Sincronizar metadatos relacionales con vectores es propenso a errores. **Rechazado** por persistencia y sincronización.

---

## Consecuencias

### Positivas (+)
- **Cero dependencias externas**: No requiere Docker ni servicios cloud.
- **Portabilidad**: Toda la memoria semántica es un simple archivo `.sqlite3`. Fácil de mover, copiar o respaldar.
- **Recuperación Híbrida**: Permite combinar SQL tradicional, búsqueda de texto completo (FTS5) y búsqueda vectorial en una sola base de datos atómica.
- **Rendimiento**: Latencia sub-25ms para búsquedas en colecciones de tamaño local (hasta cientos de miles de vectores).

### Negativas (-)
- **Escala limitada**: No está diseñado para billones de vectores como Milvus o Pinecone (aunque es más que suficiente para un agente personal/profesional).
- **Escritor único (Single-writer)**: Como toda base de datos SQLite, tiene limitaciones en escrituras concurrentes masivas (mitigado por el modo WAL).

---

## Mapeo al Código

- **Implementación**: `src/memory/sqlite_vec_store/` (módulos `mod.rs`, `vector.rs`, `search.rs`).
- **Persistencia**: El archivo de base de datos se genera por defecto como `vec-store.sqlite3` en el directorio de datos de Xavier.
- **Esquema**: Utiliza la tabla virtual `vec0` para el almacenamiento de embeddings de alta dimensionalidad.

---

## Referencias
- [DevLog: Why SQLite-Vec?](../devlog/2026-05-10-why-sqlite-vec.md)
- [ADR-001: QmdMemory como dominio central](./001-memory-domain.md)
