# Evaluación de Xavier para un despacho de abogados

Inicio: 2026-09-18. Cierre y controles adicionales: 2026-09-19. Checkout inicial evaluado: `f28e01e30cbe0417eb7603b848f4c6e7df7e98b5`. Al retomar, HEAD era `4f2cef98e5f839e2d8af19840d5355f8e9b19298`, con cambios de otro trabajo en curso. Los hashes de todos los módulos `src/documents/*.rs` siguen coincidiendo con los evaluados.

## Dictamen

**La implementación actual no demuestra preparación para operar como cerebro documental de un despacho jurídico.** Hay componentes útiles de memoria, recuperación híbrida, segmentación jurídica y controles de seguridad, pero faltan conexiones y garantías esenciales. Reconocer un archivo y calcular su hash no equivale a comprenderlo ni a poder citarlo.

Esta evaluación distingue ejecución de componentes, prueba HTTP del servicio instalado y revisión estática del checkout. El binario activo no se reconstruyó ni se desplegó durante la auditoría. No se usaron documentos de clientes, ni se certifica cumplimiento normativo.

## Pruebas reproducibles

### Extractores originales

```bash
python scripts/eval/legal_rag_readiness.py --output /tmp/xavier-legal-rag-audit
```

El programa compila directamente `src/documents/mod.rs` y sus módulos originales en un paquete Cargo pequeño y aislado. Usa las funciones hexadecimales originales de `src/crypto/mod.rs`, dependencias disponibles localmente, modo offline, un solo trabajo de compilación y los codecs PNG/JPEG de `image`. Ejecuta Cargo mediante `xavier exec`. No utiliza la base de datos de Xavier.

Genera PDF con objetos, árbol de páginas y tabla xref válidos; un contrato de texto; PNG/JPEG con un importe rasterizado; y un PDF con ese raster. No simula que metadatos sean OCR. Las imágenes son fixtures sintéticos claros, no fotografías de campo: no se ha medido rendimiento frente a desenfoque, perspectiva, sombras o manuscritos.

El JSON registra cada expectativa, resultado observado y hashes SHA-256 de las fuentes y fixtures. Se vuelve a abrir el informe desde disco. **El código de salida 1 significa criterios de aceptación incumplidos**, no que todas las pruebas hayan pasado.

**Resultado: 2/10 criterios aprobados, 8/10 incumplidos.** Es una batería de aceptación pequeña, no un porcentaje global de calidad de Xavier.

| Criterio | Resultado observado |
|---|---|
| PDF nativo sencillo | Recupera correctamente `7500000` |
| Una página con dos streams | Informa dos páginas: cita incorrecta |
| Orden del árbol de páginas | Invierte los textos al seguir el orden físico de los objetos |
| Tildes WinAnsi con escape octal | Devuelve `CL301USULA` e `indemnizaci363n` |
| PDF inválido con cabecera | Lo acepta en vez de rechazarlo |
| PNG con importe visible | Sin OCR; solo metadata |
| JPEG con importe visible | Sin OCR; solo metadata |
| PDF con importe rasterizado | Aviso de OCR pendiente, sin importe |
| Segmentador jurídico explícito | Conserva importe, excepción y números de cláusula |
| Extractor genérico de contrato | No produce anclas de cláusula |

Evidencia: `/tmp/xavier-legal-rag-audit-20260918/report.json`; originales sintéticos en su subdirectorio `fixtures/`. Los hashes se verificaron de nuevo al retomar el 19 de septiembre; todos coinciden. La imagen se inspeccionó visualmente: contiene `7500000` claramente legible.

También se ejecutaron los **30 tests unitarios originales de los módulos documentales: 30 aprobados, 0 ignorados**. Se ejecutaron con `cargo test --manifest-path /tmp/xavier-legal-rag-audit-20260918/probe/Cargo.toml` mediante el proxy. Estos resultados y los ocho criterios fallidos son compatibles: los tests existentes no cubren las exigencias documentales de esta evaluación.

### Recuperación HTTP real

`scripts/eval/legal_rag_http_eval.py` siembra seis documentos sintéticos bajo un prefijo UUID, verifica lectura por ID, mide cinco consultas y elimina únicamente los registros creados. No consulta el corpus global. El distractor pertenece a otro expediente sintético.

**Resultado en ambas ejecuciones, 18 y 19 de septiembre:** 6/6 documentos guardados y leídos con contenido y ruta correctos; 0/5 consultas recuperaron la referencia esperada entre los tres primeros resultados de `/v1/memories/search` (recall@3=0, MRR@3=0). Las respuestas fueron HTTP 200 y `degraded=false`, con resultados vacíos.

El control del día 19 buscó el importe literal `12500` con el mismo filtro de expediente: `/v1/memories/search` no encontró nada, mientras `/memory/search` sí devolvió el documento esperado. **Esto demuestra una discrepancia entre rutas para la ingesta reciente; no demuestra que toda la recuperación de Xavier sea incapaz de encontrar texto.** No se determinó la causa raíz ni se evaluó un eventual plazo de consistencia del índice.

En las dos ejecuciones documentadas, los seis registros se eliminaron y se verificó su ausencia por ID. Solo se eliminaron registros sintéticos creados por la prueba, reproducibles desde el script. Evidencias: `/tmp/xavier-legal-rag-http-20260918.json` y `/tmp/xavier-legal-rag-http-20260919.json`. Las cinco consultas del día 19 tardaron entre 0,711 y 3,400 segundos; la muestra no permite caracterizar latencia de producción.

Reproducción: `python scripts/eval/legal_rag_http_eval.py --output /tmp/xavier-legal-rag-http.json`. El script lee la credencial local sin mostrarla y termina con código 1 si falla la aceptación o la limpieza.

Esto mide recuperación de texto ya extraído. No mide extracción de archivos a través de HTTP, generación LLM, fidelidad de respuestas, aislamiento entre identidades ni calidad jurídica. La ausencia de un distractor mediante un filtro enviado por el cliente no demuestra autorización por expediente.

### Suite general

Se intentó `xavier exec "cargo test --test multimodal_rag_pipeline_e2e --no-default-features --features ci-safe"`. Terminó con código 137 sin resultados de tests. La causa no quedó determinada; **no se cuenta como validación satisfactoria**.

## Hallazgos priorizados

| Prioridad | Evidencia del checkout | Consecuencia práctica |
|---|---|---|
| P0 | `src/documents/image_extractor.rs:167` asigna `ocr_text: None`; `src/documents/img.rs:97` describe resolución/hash, no el contenido | Una foto de un contrato no aporta sus cláusulas ni importes a la búsqueda |
| P0 | `src/documents/pdf.rs:248` sustituye escaneados por aviso de que necesitan OCR | El PDF escaneado se reconoce, pero no se transcribe |
| P0 | `src/documents/pdf.rs:216` recorre streams y aumenta `page_counter` por stream con texto | Una página con dos streams aparece como dos páginas; el orden físico de objetos puede alterar las citas |
| P0 | `src/server/v1_api.rs:1153` filtra clasificación en search; `:1420` y `:1785` no aplican ese mismo control en package/get | Riesgo de acceso por otra ruta a contenido que search oculta. Hallazgo estático, no explotación con otra identidad demostrada |
| P0 | `src/enterprise/rbac.rs:111` autoriza siempre; `src/cli/server.rs:1593` inyecta un workspace común | Namespaces/filtros no prueban barreras obligatorias entre abogados, clientes o expedientes |
| P1 | Prueba HTTP del 19: alta y GET correctos, búsqueda V1 vacía, búsqueda legacy positiva para el mismo importe/filtro | Debe resolverse la consistencia entre rutas antes de confiar en que un contrato recién cargado sea consultable |
| P1 | En el snapshot inicial no se encontraron llamadas productivas a los extractores fuera de sus módulos; el 19 apareció `src/collections/indexer.rs` con una llamada nueva al extractor genérico | Existe trabajo nuevo de integración; todavía no se ha validado su flujo completo ni su despliegue |
| P1 | `src/documents/pdf.rs:22` y `:52` no implementan las codificaciones tipográficas PDF completas | Las tildes, octales y fuentes codificadas pueden corromper palabras jurídicas |
| P1 | `src/memory/qmd/search/hybrid.rs:111` reconstruye candidatos desde `HashMap` y trunca antes de ordenar | Con más de 32 candidatos puede descartar evidencia relevante de forma dependiente del orden del mapa |
| P1 | `src/memory/qmd/search/hybrid.rs:53` y `src/search/hybrid.rs:221` filtran después del top-k vectorial | Documentos fuera del expediente pueden consumir candidatos y reducir recall. Esto no demuestra por sí solo fuga de datos |
| P1 | `src/memory/qmd/search/scoring.rs:475` aplica decaimiento, nuevamente multiplicado en `hybrid.rs:166` | Un contrato antiguo vigente puede perder prioridad frente a un documento reciente. Vigencia jurídica y fecha de creación no son equivalentes |
| P1 | `src/memory/query_engine.rs:140` activa `HybridSearcher` solo en ciertas opciones | La configuración del reranker no garantiza su uso en todas las rutas; debe medirse por endpoint |
| P2 | `src/retrieval/eval.rs:22` usa una sola referencia esperada; `:236` acepta coincidencia substring | No basta para medir cobertura de excepciones, referencias cruzadas, citas exactas o abstención |

Las líneas son referencias del checkout revisado, no garantías sobre un binario antiguo. En particular, `/v1/memories/search` utiliza `query_with_embedding_filtered` y tiene un campo `degraded`; los problemas de manejo de errores de `MemoryQueryEngine` no deben atribuirse automáticamente a ese endpoint.

El parser PDF tampoco rechaza un archivo que solo contiene cabecera y basura. La ingesta debería distinguir extracción satisfactoria, contenido no soportado, archivo inválido y extracción pendiente; no convertir todos esos estados en documentos aparentemente indexados.

## Qué sí existe y qué no prueban los tests anteriores

- Búsqueda BM25, vectores, fusión RRF, filtros y metadata de procedencia.
- Segmentador de artículos/cláusulas con jerarquía cuando se invoca explícitamente.
- Hashes SHA-256 y perceptuales de imágenes; útiles para identidad/deduplicación, no sustituyen OCR o comprensión visual.
- Autenticación, clasificación en determinadas lecturas, escaneo de entrada y límites HTTP. No se concluye que el sistema carezca de seguridad; la cobertura entre rutas es inconsistente.
- `tests/e2e/multimodal_rag_pipeline_e2e.rs` llama componentes directamente, usa vectores constantes (`vec![0.1; 1536]`) y SQLite en memoria. Eso no demuestra fotos/PDF → OCR → embeddings reales → búsqueda → respuesta con evidencia.

No encontré integración de conservación probatoria (`legal_hold`) ni consumidores de `default_retention_days` fuera del perfil. No se evaluaron restauración de backups, eliminación en cascada de todos los índices/cachés, firmas manuscritas, DOCX, tablas complejas ni extracción de sellos.

### Trabajo nuevo observado al retomar

Había cambios ajenos a esta auditoría en `src/collections/`, `src/rag/`, `src/gateways/` y `src/server/docbot_routes.rs`, además de `.env.example`, `src/lib.rs` y `src/server/mod.rs`. Se conservaron. El nuevo indexador llama a `GenericDocumentExtractor`; la nueva pipeline contempla una respuesta sin evidencias y fuentes citadas. No se ha certificado ni compilado ese conjunto nuevo.

La lectura puntual muestra que el indexador vuelve a fragmentar el texto y asigna páginas por igualdad de índice entre los fragmentos antiguos y nuevos (`src/collections/indexer.rs`), lo cual necesita una prueba específica de procedencia. Además, reutiliza los extractores sin OCR que fallaron aquí. No se observó una llamada a `docbot_router` desde `src/cli/server.rs` en esta revisión. Por tanto, el código nuevo no invalida los fallos ejecutados ni demuestra por sí mismo que el servicio activo ya tenga una ingesta documental completa.

## Contraste con referencias actuales

No existe una sola certificación universal denominada «último estándar RAG». Se contrastó el código con prácticas de ingeniería y seguridad y con evaluaciones publicadas, consultadas el 2026-09-18:

1. [Docling: modelo documental](https://docling-project.github.io/docling/reference/docling_document/) y [OCR por página](https://github.com/docling-project/docling/blob/main/docs/examples/full_page_ocr.py): conservar estructura, páginas y regiones permite volver desde un fragmento al documento original. Un adaptador local con esta representación es una alternativa a extender el parser artesanal; requiere evaluación, no adopción automática.
2. [Microsoft: diseño y evaluación de RAG](https://learn.microsoft.com/en-us/azure/architecture/ai-ml/guide/rag/rag-solution-design-and-evaluation-guide) y [evaluadores RAG](https://learn.microsoft.com/en-us/azure/foundry/concepts/evaluation-evaluators/rag-evaluators): evaluar recuperación y respuestas fundamentadas por separado. En Xavier faltan pruebas representativas que cubran todo el recorrido documental.
3. [OWASP RAG Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/RAG_Security_Cheat_Sheet.html): permisos heredados por fragmento, autorización en recuperación, separación de tenants, procedencia, eliminación de derivados y tratamiento del contenido recuperado como datos no confiables. Son recomendaciones de seguridad, no una certificación jurídica.
4. [LegalBench-RAG, 2024](https://arxiv.org/abs/2408.10343): evaluación de pasajes jurídicos precisos. [Legal RAG Bench, marzo de 2026](https://arxiv.org/abs/2603.01710): evaluación de recuperación y respuesta con evidencias. Ninguno sustituye un corpus validado para español, documentos y jurisdicción del despacho concreto; no se ejecutaron estos benchmarks aquí.

## Plan de cierre y aceptación propuesta

Los siguientes umbrales son objetivos de ingeniería propuestos, no estándares oficiales ni garantías legales.

1. **Cerrar autorización antes de usar expedientes reales.** Derivar cliente/expediente y permisos desde la identidad autenticada; aplicar el mismo control en search, get, context, export y cachés. Probar cambios de permisos y cero resultados no autorizados con identidades distintas.
2. **Implementar ingesta documental completa.** Extracción nativa y OCR local para escaneados/fotos, con errores explícitos. Preservar original inmutable, hash, versión, página, coordenadas, cláusula y motor de extracción. Confirmar importes, fechas, negaciones y excepciones; escalar baja confianza a revisión humana.
3. **Conectar segmentación y recuperación.** Conservar excepciones junto a obligaciones, referencias cruzadas y tablas; ordenar antes de truncar, filtrar dentro de recuperación y comprobar el reranker real. La vigencia/supersesión debe prevalecer sobre una heurística de recencia de memorias.
4. **Construir corpus jurídico representativo.** Al menos 100 preguntas revisadas por especialistas, con respuestas ausentes, contratos y otrosíes contradictorios, referencias múltiples, escaneos y fotos difíciles. Medir por modalidad y expediente, no solo un promedio global.
5. **Criterios iniciales:** recall@5 de evidencia ≥95%; citas de página/fragmento verificables en el 100% de las afirmaciones fácticas del set aceptado; cero fugas en pruebas de autorización; cero alteraciones silenciosas de importes/fechas del set crítico; abstención explícita si no hay evidencia suficiente. Medir fidelidad, cobertura, latencia p50/p95 y coste por consulta junto con revisión humana de errores.

La recomendación es cerrar primero OCR/ingesta, procedencia y autorización. Añadir más modalidades, GraphRAG o generación agentiva antes de resolver esos puntos no demuestra mejor preparación para el despacho.
