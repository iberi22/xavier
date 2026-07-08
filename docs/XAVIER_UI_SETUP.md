# Xavier - Configuración de Interfaces de Usuario

Este documento explica las diferentes interfaces disponibles para Xavier y cómo iniciarlas.

## 📊 Estado Actual del Sistema

Tu instalación actual de Xavier tiene:
- ✅ **Servidor Backend HTTP** corriendo en `http://localhost:8006` (PID 17504)
- ❌ **Panel UI (Tauri)** no construido/no iniciado
- ❌ **Icono de bandeja del sistema** no disponible (requiere Panel UI o modo GUI)

## 🎯 Interfaces Disponibles

Xavier ofrece tres tipos de interfaces de usuario:

### 1. **Servidor HTTP Solo** (Modo Actual)
- **Qué es**: Servidor backend sin interfaz gráfica
- **Uso**: APIs REST + MCP Server
- **Inicio**: `xavier.exe http 8006`
- **Estado**: ✅ Ya corriendo

### 2. **TUI Dashboard** (Terminal UI)
- **Qué es**: Interfaz interactiva en la terminal usando ratatui
- **Características**:
  - Dashboard de métricas en tiempo real
  - Visualización de memoria y estadísticas
  - Navegación con teclado
  - No requiere construcción adicional de frontend
- **Inicio**: 
  ```powershell
  .\scripts\start-xavier-tui.ps1
  ```
- **Binario**: `target\release\xavier-tui.exe`
- **Requisito**: Feature `cli-interactive` (ya incluida por defecto)

### 3. **Panel UI** (Interfaz Gráfica con Tauri)
- **Qué es**: Aplicación de escritorio completa con icono en bandeja del sistema
- **Características**:
  - Interfaz React moderna
  - Icono en la bandeja del sistema de Windows
  - Visualización de grafos de memoria
  - Dashboard de métricas avanzadas
  - Gestión de workspaces
- **Inicio**: 
  ```powershell
  .\scripts\start-xavier-with-ui.ps1
  ```
- **Ubicación**: `panel-ui/` (requiere construcción)

## 🚀 Guía de Inicio Rápido

### Opción A: Iniciar con TUI (Más Rápido)

El TUI ya está construido y listo para usar:

```powershell
# Desde la raíz de xavier/
.\scripts\start-xavier-tui.ps1
```

Esto iniciará una interfaz interactiva en tu terminal actual.

**Controles del TUI:**
- `q` - Salir
- `↑↓` - Navegar
- `Tab` - Cambiar entre paneles

---

### Opción B: Construir e Iniciar Panel UI Completo

Si necesitas el icono en la bandeja del sistema y la interfaz gráfica completa:

#### Paso 1: Instalar Dependencias de Node

```powershell
cd panel-ui
pnpm install
```

Si no tienes `pnpm`, instálalo primero:
```powershell
npm install -g pnpm
```

#### Paso 2: Construir el Panel UI

```powershell
# Desde panel-ui/
pnpm tauri build
```

Esto tomará varios minutos la primera vez. Compilará:
- Frontend React/Vite
- Backend Rust de Tauri
- Empaquetará todo como aplicación de escritorio

#### Paso 3: Iniciar con UI Completo

```powershell
# Desde la raíz de xavier/
.\scripts\start-xavier-with-ui.ps1
```

Este script:
1. Verifica que el servidor backend esté corriendo
2. Si no está corriendo, lo inicia automáticamente
3. Lanza el Panel UI de Tauri
4. El icono aparecerá en la bandeja del sistema

---

### Opción C: Modo Desarrollo (Panel UI con Hot-Reload)

Para desarrollo del Panel UI con hot-reload:

```powershell
.\scripts\start-xavier-with-ui.ps1 -DevMode
```

Esto iniciará:
- Servidor Vite en modo dev (hot-reload)
- Tauri en modo dev conectado al servidor Vite
- Los cambios en React se reflejarán instantáneamente

---

## 🛑 Detener el Sistema

### Detener Todo (Servidor + UI)

```powershell
.\scripts\stop-xavier-all.ps1
```

### Detener Solo el Servidor

```powershell
.\scripts\stop-xavier.ps1
```

### Detener TUI

Simplemente presiona `q` en la interfaz del TUI.

---

## 🔧 Verificar Estado

Para ver el estado actual del sistema:

```powershell
.\scripts\status-xavier.ps1
```

Mostrará:
- Estado del proceso
- Respuesta del API /health
- Estadísticas de memoria
- Versión

---

## ❓ Solución de Problemas

### "No se encuentra el icono en la bandeja del sistema"

**Causa**: El Panel UI de Tauri no está corriendo.

**Soluciones**:
1. Verifica que construiste el Panel UI: `pnpm tauri build`
2. Inicia con el script completo: `.\scripts\start-xavier-with-ui.ps1`
3. Verifica que el proceso de Tauri esté corriendo:
   ```powershell
   Get-Process | Where-Object { $_.ProcessName -eq "xavier" }
   ```

### "El servidor no responde"

**Causa**: Xavier backend no está corriendo.

**Solución**:
```powershell
.\scripts\start-xavier.ps1
```

### "Error al construir el Panel UI"

**Causas comunes**:
1. Node.js no instalado o versión antigua (requiere ≥22.12.0)
2. pnpm no instalado
3. Tauri CLI no instalado

**Soluciones**:
```powershell
# Verificar versión de Node
node --version

# Instalar pnpm
npm install -g pnpm

# Reinstalar dependencias
cd panel-ui
pnpm install

# Construir de nuevo
pnpm tauri build
```

---

## 📝 Notas Técnicas

### Arquitectura del Sistema

```
┌─────────────────────────────────────┐
│   Panel UI (Tauri)                  │
│   - React Frontend                  │
│   - System Tray Icon                │
│   - puerto: 4174 (dev)              │
└──────────────┬──────────────────────┘
               │ HTTP/WebSocket
               ▼
┌─────────────────────────────────────┐
│   Xavier Backend                    │
│   - HTTP Server (Axum)              │
│   - MCP Server                      │
│   - puerto: 8006                    │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│   Storage Layer                     │
│   - SQLite + sqlite-vec             │
│   - data/xavier_memory.db           │
└─────────────────────────────────────┘
```

### Puertos Utilizados

- **8006**: Xavier Backend HTTP API
- **4174**: Vite dev server (solo en modo desarrollo)

### Features de Cargo

El proyecto tiene features opcionales para diferentes modos:

```toml
default = ["cli-interactive"]
cli-interactive = ["ratatui", "crossterm"]  # TUI Dashboard
tauri = ["dep:tauri"]                       # Panel UI
```

---

## 🎯 Recomendación

**Para uso diario**: Usa el TUI (`start-xavier-tui.ps1`)
- Más ligero
- Inicio instantáneo
- Toda la funcionalidad necesaria
- No requiere construcción de frontend

**Para presentaciones o demo**: Usa el Panel UI completo
- Interfaz visual moderna
- Icono en bandeja del sistema
- Gráficos y visualizaciones avanzadas

---

**Última actualización**: 2026-07-07
**Versión de Xavier**: 0.12.0
