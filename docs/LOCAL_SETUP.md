# Guía Xavier Local-First (Ollama)

Esta guía explica cómo configurar Xavier para funcionar de manera 100% local utilizando [Ollama](https://ollama.com/).

## Requisitos Previos

1.  **Ollama**: Descarga e instala Ollama desde [ollama.com](https://ollama.com/).
2.  **Modelos**: Una vez instalado Ollama, descarga los modelos necesarios ejecutando:
    ```bash
    ollama pull qwen3-coder
    ollama pull embeddinggemma
    ```

## Configuración paso a paso

1.  **Configuración de variables de entorno**:
    Copia el archivo de ejemplo `.env.example` a `.env`:
    ```bash
    cp .env.example .env
    ```

2.  **Activar modo local** (Opcional si usas los defaults de `config/xavier.config.json`):
    Asegúrate de que las siguientes variables estén configuradas (o descomentadas) en tu `.env` para forzar el modo local:
    ```env
    XAVIER_MODEL_PROVIDER=local
    XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1
    XAVIER_LOCAL_LLM_MODEL=qwen3-coder
    XAVIER_EMBEDDING_PROVIDER_MODE=local
    XAVIER_EMBEDDING_MODEL=embeddinggemma
    ```

3.  **Iniciar Xavier**:
    Ejecuta el servidor de Xavier:
    ```bash
    cargo run -- serve
    ```
    Deberías ver en los logs que se están utilizando el LLM local y los embeddings locales.

## Solución de Problemas (Troubleshooting)

### Error: Puerto 11434 ocupado
Ollama utiliza el puerto `11434` por defecto. Si recibes un error indicando que el puerto está en uso, asegúrate de que no haya otra instancia de Ollama ejecutándose o cambia el puerto en la configuración de Ollama y actualiza `XAVIER_LOCAL_LLM_URL`.

### Error: Modelo no encontrado
Si Xavier reporta que no encuentra el modelo, verifica que el nombre coincida exactamente con el descargado en Ollama:
```bash
ollama list
```
Los nombres por defecto configurados son `qwen3-coder` para chat y `embeddinggemma` para embeddings.

### OLLAMA_HOST
Si estás ejecutando Xavier dentro de un contenedor Docker o en una máquina diferente a Ollama, asegúrate de configurar `OLLAMA_HOST=0.0.0.0` en tu entorno y actualizar las URLs de `localhost` a la IP correspondiente.
