# Workflows de n8n para Xavier

Este directorio contiene exports JSON de n8n para monitorear Xavier en `http://localhost:8006`.

## Variables requeridas

Configura estas variables en el entorno donde corre n8n:

```bash
XAVIER_URL=http://localhost:8006
XAVIER_TOKEN=tu_token_de_xavier
```

`XAVIER_URL` tiene fallback a `http://localhost:8006` en los workflows. `XAVIER_TOKEN` se envia como cabecera `X-Xavier-Token` en los nodos HTTP Request.

## Importar en n8n

1. Abre n8n.
2. Ve a **Workflows**.
3. Selecciona **Import from file**.
4. Importa uno de estos archivos:
   - `health-check.json`
   - `memory-monitor.json`
   - `daily-report.json`
5. Revisa que las variables `XAVIER_URL` y `XAVIER_TOKEN` existan en el proceso de n8n.
6. Ejecuta una prueba manual.
7. Activa el workflow cuando la prueba responda correctamente.

## Workflows incluidos

### Xavier Health Check

Archivo: `health-check.json`

Ejecuta un GET a:

```text
{{XAVIER_URL}}/health
```

Frecuencia: cada 5 minutos.

Uso: verificar que el servicio Xavier este activo y responda con estado saludable.

### Xavier Memory Monitor

Archivo: `memory-monitor.json`

Ejecuta un GET autenticado a:

```text
{{XAVIER_URL}}/memory/stats
```

Frecuencia: cada 15 minutos.

Uso: consultar estadisticas de memoria de Xavier usando la cabecera `X-Xavier-Token`.

### Xavier Daily Report

Archivo: `daily-report.json`

Ejecuta una vez al dia a las 08:00 en la zona horaria `America/Bogota`.

El workflow consulta:

```text
{{XAVIER_URL}}/health
{{XAVIER_URL}}/memory/stats
```

Luego combina ambas respuestas y genera un objeto JSON con:

- `title`
- `generatedAt`
- `xavierUrl`
- `health`
- `memoryStats`
- `summary`

Este reporte queda como salida del nodo `Generate Daily Report` para conectarlo despues a email, Slack, Telegram, base de datos u otro destino.
