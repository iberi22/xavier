# Puentes de LLM Local en Xavier

Xavier permite desacoplar la inteligencia del sistema de los proveedores cloud (OpenAI, Anthropic) mediante el uso de "puentes" locales. Esto permite ejecutar modelos en tu propia máquina o vía CLIs que actúan como proveedores locales.

Tras la Ola 2 de desarrollo, los puentes locales y de redundancia están integrados por defecto de manera nativa y robusta.

## Comparativa de Vías Locales

| Vía | Proceso externo | 100% offline | Setup |
| :--- | :--- | :--- | :--- |
| **Ollama (Default)** | sí (`ollama serve`) | **Sí** | `ollama pull <modelo>` |
| **LM Studio** | sí (servidor HTTP) | **Sí** | Descargar GUI y activar servidor |
| **opencode CLI bridge** | sí (binario `opencode`) | **Depende** | `npm install -g @opencode/cli` |

---

## 1. Ollama (Preconfigurado y por Defecto)

El puente compatible con la API de OpenAI hacia Ollama está **cableado por defecto en la arquitectura de Xavier (no es experimental)**. Tras la Ola 2, el chat del panel se conecta y realiza consultas directamente a este puente local si se selecciona como proveedor principal o como parte de la cadena de fallbacks automáticos ante la caída de servicios cloud.

### Contrato del API Bridge
Para comunicarse con Ollama, Xavier implementa un cliente HTTP optimizado que consume el endpoint compatible con OpenAI expuesto nativamente por Ollama:

*   **URL Base (`XAVIER_LOCAL_LLM_URL`):** `http://localhost:11434/v1` (apunta al puerto de escucha por defecto de Ollama en su ruta de compatibilidad de API v1).
*   **Autenticación (`XAVIER_LOCAL_LLM_API_KEY`):** No se requiere clave de autenticación (sin auth/vacío).
*   **Modelo de Chat (`XAVIER_LOCAL_LLM_MODEL`):** `qwen3-coder` (un modelo de lenguaje altamente eficiente y especializado en generación de código, depuración y razonamiento de sistemas).

### Configuración en `.env`
Para forzar la inicialización de este puente como el proveedor por defecto, las siguientes variables deben estar configuradas:
```env
XAVIER_MODEL_PROVIDER=local
XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1
XAVIER_LOCAL_LLM_MODEL=qwen3-coder
```

---

## 2. LM Studio

LM Studio proporciona una interfaz gráfica excelente para experimentar con modelos y expone un servidor local HTTP compatible con la API de OpenAI.

- **Configuración**: `XAVIER_MODEL_PROVIDER=local`
- **Variables**:
  - `XAVIER_LOCAL_LLM_URL`: Apuntar al puerto expuesto por LM Studio (ej. `http://localhost:1234/v1`)
  - `XAVIER_LOCAL_LLM_MODEL`: El nombre exacto del modelo que tienes actualmente cargado y activo en la interfaz de LM Studio.
  - `XAVIER_LOCAL_LLM_API_KEY`: No requerida por defecto.

---

## 3. opencode CLI Bridge

El puente `opencode` es una vía alternativa de "capa agentic local". En lugar de llamar a un endpoint HTTP de forma directa, Xavier lanza el binario `opencode` como un subproceso local.

### ¿Es 100% local?
**No necesariamente.** Por defecto, el CLI de `opencode` actúa como un puente hacia los servicios de inferencia de OpenCode.ai (Cloud). Aunque Xavier lo trata como un proveedor "local" porque se ejecuta como un binario en la máquina, el tráfico de inferencia suele salir a internet.

### Configuración
- **Configuración**: `XAVIER_MODEL_PROVIDER=opencode`
- **Variables Requeridas**:
  - `OPENCODE_API_KEY`: Tu clave de API de OpenCode (o `ZAI_API_KEY`).
  - `XAVIER_OPENCODE_MODEL`: Default `opencode/deepseek-v4-flash`.

### Instalación
Si no tienes el binario instalado y configuras `XAVIER_MODEL_PROVIDER=opencode`, Xavier validará la presencia del comando en el PATH al arrancar y detendrá el proceso de inicio de manera segura mostrando instrucciones de instalación detalladas. Puedes instalarlo vía npm:
```bash
npm install -g @opencode/cli
```

---

## Rol en la Capa Agentic (Post-Ola 2/3)

En el diseño de Xavier, el puente `opencode` y los modelos locales robustecen la **resiliencia táctica**. Si Ollama no está disponible o el modelo local no tiene suficiente capacidad para una tarea de razonamiento compleja de varios pasos, Xavier puede delegar de manera controlada y transparente la inferencia a través del CLI de `opencode` para aprovechar modelos más potentes (como DeepSeek V4) con una mínima configuración, manteniendo la interfaz de ejecución basada en procesos en lugar de hooks HTTP complejos.
