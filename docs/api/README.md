# Xavier REST API - User Guide & Documentation

Bienvenido a la documentación oficial de la API REST de **Xavier**, el motor centralizado de contexto semántico, integración de memoria persistente y red mesh federada P2P.

Este directorio contiene las especificaciones y guías necesarias para integrarse con Xavier:
- [Especificación OpenAPI 3.1.0 (YAML)](./openapi.yaml)
- [Postman Collection v2.1.0](./xavier.postman_collection.json)

---

## 1. Conceptos Globales

### Base URL
Por defecto, el servidor HTTP de Xavier corre en:
```http
http://localhost:8006
```
El puerto puede cambiarse al arrancar el servidor usando:
```bash
xavier http <puerto>
```

### Versionado de API
Las rutas canónicas de Xavier utilizan el prefijo `/v1/` para garantizar la estabilidad y compatibilidad futura de las integraciones. Ejemplo: `/v1/memories`.
Las rutas heredadas (legacy) como `/memory/add` siguen soportadas para compatibilidad con la CLI pero se recomienda migrar a `/v1/`.

### Variables de Entorno de Configuración
- `XAVIER_PORT`: Puerto HTTP (por defecto `8006`).
- `XAVIER_TOKEN`: Token estático para la protección básica de los endpoints.
- `XAVIER_JWT_SECRET`: Clave secreta para la generación y validación de tokens JWT en el flujo de sesión de usuarios.
- `XAVIER_STATE_DIR`: Directorio persistente donde se guardan las bases de datos `auth.db`, `auth_store.db` y `memory.db`.

---

## 2. Protocolo de Autenticación

Xavier soporta dos mecanismos de autenticación independientes dependiendo del caso de uso:

### A. Autenticación Básica por Token de Servidor (`X-Xavier-Token`)
Diseñado para integraciones directas entre agentes locales, CLI y scripts.
- **Header Requerido:** `X-Xavier-Token: <token_estatico>`
- **Comportamiento:** Si el header no coincide con la variable `XAVIER_TOKEN` configurada en el servidor, se denegará el acceso con un error `401 Unauthorized`.

### B. Autenticación Completa basada en Sesiones JWT + 2FA
Diseñado para interfaces de usuario avanzadas (como `panel-ui`) y usuarios finales. El flujo completo comprende:

1. **Registro de Usuario (`POST /auth/register`)**
   - Registra el correo y contraseña de manera segura. Las contraseñas se hashean usando algoritmos criptográficos robustos y se guardan de forma aislada en `auth.db`.
2. **Inicio de Sesión (`POST /v1/auth/login`)**
   - Retorna un estado inicial. Si el usuario tiene habilitado el segundo factor (MFA/TOTP), la respuesta indicará `totp_required: true` y no entregará el token final aún.
3. **Verificación de 2FA TOTP (`POST /v1/auth/totp/verify`)**
   - El cliente envía el código de un solo uso.
   - **Nota de Compatibilidad (TOTP Double-Division Bug):** El servidor implementa una doble división por 30 de la marca de tiempo Unix (`timestamp / 30 / 30`) para validar códigos TOTP. Los clientes deben generar los códigos teniendo esto en cuenta para evitar fallos de sincronización horaria.
   - Retorna el token JWT definitivo en caso de éxito.
4. **Semilla de Recuperación y Códigos de Backup**
   - En el setup inicial se exponen endpoints para ver y verificar la semilla criptográfica (`/auth/recovery/seed/show` y `/auth/recovery/seed/verify`) o generar códigos de backup de emergencia (`/auth/recovery/backup-codes`) para recuperar el acceso si se pierde el dispositivo 2FA.
5. **Gestión de Sesiones Activas**
   - `GET /v1/auth/sessions`: Lista los tokens y dispositivos con sesiones vigentes.
   - `DELETE /v1/auth/sessions/{id}`: Revoca y destruye una sesión activa de forma inmediata.

---

## 3. Rate Limiting (Control de Flujo)

Para mitigar ataques de fuerza bruta y abusos de recursos, Xavier cuenta con un middleware dinámico de control de tasa de peticiones (Rate Limiting) y registro de auditoría en base de datos.

### Control de Fuerza Bruta en Inicio de Sesión
- **Ruta Protegida:** `/v1/auth/login`
- **Regla:** Máximo de **5 intentos fallidos dentro de una ventana de 15 minutos**.
- **Comportamiento al Exceder:** El servidor registra el evento `login_failed` en el log de auditoría interna, bloquea temporalmente la dirección IP solicitante y retorna un error `429 Too Many Requests`.

### Headers de Respuesta de Tasa de Flujo
Cuando una petición es procesada, el servidor añade los siguientes headers para ayudar a los clientes a regular su frecuencia:
- `X-RateLimit-Limit`: Cantidad máxima de peticiones permitidas en la ventana de tiempo.
- `X-RateLimit-Remaining`: Cantidad de peticiones disponibles restantes.
- `X-RateLimit-Reset`: Tiempo Unix en segundos para el reinicio del límite.

---

## 4. Estructura de Errores y Seguridad

Todos los errores retornados por la API REST de Xavier siguen un estándar estructurado en formato JSON:

```json
{
  "error": {
    "code": <codigo_numerico>,
    "message": "<descripcion_del_error>",
    "details": "<informacion_tecnica_adicional_u_opcional>"
  }
}
```

### Códigos de Estado HTTP Comunes
- `400 Bad Request`: Payload malformado o faltan parámetros requeridos.
- `401 Unauthorized`: Token de acceso inválido, expirado o faltante.
- `403 Forbidden`: Privilegios insuficientes para la operación (ACL del nodo mesh restrictiva).
- `429 Too Many Requests`: Se excedió el límite de llamadas permitido.
- `500 Internal Server Error`: Error inesperado en el backend o fallo en la base de datos de vectores.

### Mitigación de Prompt Injection (Código de Error Especial)
Xavier cuenta con un detector interno de inyecciones de prompts maliciosas (`PromptInjectionDetector`).
- Si se detecta un patrón sospechoso, evasión en español, leetspeak (ej. `1->i`, `3->e`), stripping de marcas de acento o codificación en Base64, la petición se bloquea de inmediato.
- **Código de Error Devuelto:** `-32000` (`XAVIER_ERROR_SECURITY`) con estado HTTP `500` o `400`.

---

## 5. Cómo Usar las Herramientas Proporcionadas

### Uso de la Colección de Postman
1. Abre Postman e importa el archivo [xavier.postman_collection.json](./xavier.postman_collection.json).
2. En las propiedades de la colección, ajusta las variables en la pestaña **Variables**:
   - `baseUrl`: Por defecto `http://localhost:8006`.
   - `token`: Tu clave secreta `XAVIER_TOKEN` (por defecto `change-me`).
3. Ejecuta peticiones de prueba como **Get System Health** para verificar que tu servidor está en línea.

### Visualización de OpenAPI / Swagger Spec
Puedes cargar [openapi.yaml](./openapi.yaml) en [Swagger Editor](https://editor.swagger.io/) o cualquier visor de OpenAPI integrado en tu IDE para visualizar la documentación interactiva y generar clientes automatizados en múltiples lenguajes de programación.
