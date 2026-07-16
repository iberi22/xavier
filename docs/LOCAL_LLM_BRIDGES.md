# Puentes de LLM Local en Xavier

Xavier permite desacoplar la inteligencia del sistema de los proveedores cloud (OpenAI, Anthropic) mediante el uso de "puentes" locales. Esto permite ejecutar modelos en tu propia máquina o vía CLIs que actúan como proveedores locales.

## Comparativa de Vías Locales

| Vía | Proceso externo | 100% offline | Setup |
| :--- | :--- | :--- | :--- |
| **Ollama (Default)** | sí (`ollama serve`) | **Sí** | `ollama pull <modelo>` |
| **LM Studio** | sí (servidor HTTP) | **Sí** | Descargar GUI y activar servidor |
| **opencode CLI bridge** | sí (binario `opencode`) | **Depende** | `npm install -g @opencode/cli` |

---

## 1. Ollama (Recomendado)

Ollama es la vía principal para ejecución 100% local.

- **Configuración**: `XAVIER_MODEL_PROVIDER=local`
- **Variables**:
  - `XAVIER_LOCAL_LLM_URL`: Default `http://localhost:11434/v1`
  - `XAVIER_LOCAL_LLM_MODEL`: Default `qwen3-coder`

## 2. LM Studio

LM Studio proporciona una interfaz gráfica y un servidor compatible con la API de OpenAI.

- **Configuración**: `XAVIER_MODEL_PROVIDER=local`
- **Variables**:
  - `XAVIER_LOCAL_LLM_URL`: Apuntar al puerto de LM Studio (ej. `http://localhost:1234/v1`)
  - `XAVIER_LOCAL_LLM_MODEL`: El nombre del modelo cargado en LM Studio.

## 3. opencode CLI Bridge

El puente `opencode` es una vía alternativa de "capa agentic local". En lugar de llamar a un endpoint HTTP, Xavier lanza el binario `opencode` como un subproceso.

### ¿Es 100% local?
**No necesariamente.** Por defecto, el CLI de `opencode` actúa como un puente hacia los servicios de inferencia de OpenCode.ai (Cloud). Aunque Xavier lo trata como un proveedor "local" porque se ejecuta como un binario en la máquina, el tráfico de inferencia suele salir a internet.

### Configuración
- **Configuración**: `XAVIER_MODEL_PROVIDER=opencode`
- **Variables Requeridas**:
  - `OPENCODE_API_KEY`: Tu clave de API de OpenCode.
  - `XAVIER_OPENCODE_MODEL`: Default `opencode/deepseek-v4-flash`.

### Instalación
Si no tienes el binario instalado, Xavier fallará con un error descriptivo. Puedes instalarlo vía npm:
```bash
npm install -g @opencode/cli
```

---

## Rol en la Capa Agentic (Ola 2/3)

En futuras versiones de Xavier, el puente `opencode` servirá como un **fallback estratégico**. Si Ollama no está disponible o el modelo local no tiene suficiente capacidad para una tarea específica, Xavier podrá delegar al CLI de `opencode` para aprovechar modelos más potentes (como DeepSeek V4) con una mínima configuración, manteniendo la interfaz de ejecución basada en procesos en lugar de hooks HTTP complejos.
