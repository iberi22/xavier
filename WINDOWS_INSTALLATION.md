# Xavier - Instalación y Configuración en Windows

Este documento explica cómo Xavier debe instalarse y ejecutarse en sistemas Windows.

## 🎯 Principio Fundamental

**En Windows, Xavier SIEMPRE debe iniciarse con el Panel UI (interfaz gráfica), nunca en modo servidor solo.**

### ¿Por Qué?

- ✅ **Experiencia de usuario**: Icono en bandeja del sistema para acceso fácil
- ✅ **Integración con Windows**: Inicio automático, accesos directos, notificaciones
- ✅ **Funcionalidad completa**: Dashboard visual, gestión de memoria, configuración gráfica
- ✅ **Auto-contenido**: El Panel UI incluye el servidor backend embebido

El modo "solo servidor" (`xavier http`) está diseñado únicamente para:
- 🐧 Servidores Linux sin GUI
- 🐳 Contenedores Docker
- 🔧 Desarrollo y debugging avanzado

## 📦 Instalación Correcta

### Opción A: Usando el Instalador (Recomendado)

1. **Construir el instalador**:
   ```powershell
   cd installer
   .\build-installer.ps1
   ```

2. **Ejecutar el instalador**:
   ```powershell
   .\Output\XavierSetup.exe
   ```

3. **Durante la instalación**:
   - ✅ Marca "Start Xavier on login" (recomendado)
   - ✅ Marca "Add to PATH" (opcional, útil para CLI)
   - ✅ Marca "Create desktop shortcut" (opcional)

4. **Resultado**:
   - Xavier Panel UI se inicia automáticamente
   - Icono aparece en la bandeja del sistema
   - Servidor backend se inicia automáticamente dentro de Tauri

### Opción B: Instalación Manual

Si no puedes usar el instalador:

1. **Construir componentes**:
   ```powershell
   # Backend
   cargo build --release --features cli-interactive
   
   # Panel UI
   cd panel-ui
   pnpm install
   pnpm tauri build
   ```

2. **Copiar binarios**:
   ```powershell
   # Crear directorio de instalación
   mkdir "C:\Program Files\Xavier"
   
   # Copiar Panel UI (principal)
   copy "panel-ui\src-tauri\target\release\xavier.exe" "C:\Program Files\Xavier\xavier-panel.exe"
   
   # Copiar binarios adicionales
   copy "target\release\xavier.exe" "C:\Program Files\Xavier\xavier-server.exe"
   copy "target\release\xavier-tui.exe" "C:\Program Files\Xavier\xavier-tui.exe"
   ```

3. **Crear acceso directo en Startup**:
   - Presiona `Win + R`
   - Escribe `shell:startup` y presiona Enter
   - Crea un acceso directo a `C:\Program Files\Xavier\xavier-panel.exe`

4. **Iniciar**:
   ```powershell
   & "C:\Program Files\Xavier\xavier-panel.exe"
   ```

## 🔧 Migración desde Modo Servidor

Si tienes Xavier corriendo en modo servidor (`xavier http`), usa el script de corrección:

```powershell
cd e:\scripts-python\xavier
.\scripts\fix-windows-installation.ps1
```

Este script:
1. ✅ Detecta tu instalación actual
2. ✅ Construye el Panel UI si no existe
3. ✅ Detiene el servidor actual
4. ✅ Instala el Panel UI
5. ✅ Configura inicio automático (opcional)
6. ✅ Inicia el Panel UI

## 🚀 Inicio y Uso

### Iniciar Xavier

**Método 1 - Automático** (si configuraste auto-inicio):
- Xavier se inicia automáticamente al hacer login en Windows
- Busca el icono en la bandeja del sistema

**Método 2 - Menú Inicio**:
- Presiona la tecla Windows
- Escribe "Xavier"
- Selecciona "Xavier" (abre Panel UI)

**Método 3 - PowerShell**:
```powershell
& "C:\Users\belal\bin\xavier-panel.exe"
```

### Encontrar el Icono en la Bandeja del Sistema

El icono de Xavier aparece en la **esquina inferior derecha** de tu pantalla:
1. Mira junto al reloj de Windows
2. Si no lo ves, haz clic en la flecha "^" para mostrar iconos ocultos
3. El icono de Xavier debería estar ahí

**Funciones del icono**:
- 🖱️ Clic izquierdo: Abrir/restaurar ventana principal
- 🖱️ Clic derecho: Menú contextual
  - Ver estadísticas
  - Configuración
  - Salir

## 📊 Verificar Instalación

Para verificar que Xavier está corriendo correctamente:

```powershell
# Ver procesos
Get-Process xavier

# Verificar API
curl http://localhost:8006/health

# Ver estado detallado
.\scripts\status-xavier.ps1
```

## 🛑 Detener Xavier

**Desde el icono de bandeja**:
- Clic derecho → Salir

**Desde PowerShell**:
```powershell
Stop-Process -Name xavier -Force
```

**Con script**:
```powershell
.\scripts\stop-xavier-all.ps1
```

## 🔍 Solución de Problemas

### No veo el icono en la bandeja del sistema

**Posibles causas**:

1. **Xavier no está corriendo**:
   ```powershell
   Get-Process xavier
   # Si no devuelve nada, Xavier no está corriendo
   ```
   **Solución**: Inicia Xavier Panel UI

2. **Estás usando el modo servidor en lugar del Panel UI**:
   ```powershell
   Get-Process xavier | Select-Object Path
   # Si muestra "xavier.exe" en lugar de "xavier-panel.exe"
   ```
   **Solución**: Ejecuta `.\scripts\fix-windows-installation.ps1`

3. **El icono está oculto**:
   - Haz clic en la flecha "^" en la bandeja del sistema
   - Busca el icono de Xavier
   - Arrástralo al área visible

4. **Windows escondió el icono**:
   - Configuración de Windows → Personalización
   - Barra de tareas → Seleccionar qué iconos se muestran en la barra de tareas
   - Activa Xavier

### Xavier no inicia automáticamente

**Verificar acceso directo**:
```powershell
# Abrir carpeta de Startup
explorer shell:startup

# Debería haber un acceso directo llamado "Xavier.lnk"
```

**Si no existe**:
```powershell
# Crear manualmente
$WScriptShell = New-Object -ComObject WScript.Shell
$Shortcut = $WScriptShell.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\Xavier.lnk")
$Shortcut.TargetPath = "C:\Users\belal\bin\xavier-panel.exe"
$Shortcut.Save()
```

### Error "Panel UI not built"

**Construir el Panel UI**:
```powershell
cd panel-ui
pnpm install
pnpm tauri build
```

**Si falla con error de pnpm**:
```powershell
npm install -g pnpm
```

**Si falla con error de Node**:
- Actualiza Node.js a versión ≥22.12.0
- Descarga desde: https://nodejs.org/

### El servidor responde pero no hay UI

Esto significa que estás ejecutando `xavier.exe` en modo servidor en lugar del Panel UI.

**Solución rápida**:
```powershell
# Detener servidor actual
Stop-Process -Name xavier -Force

# Iniciar Panel UI
& "C:\Users\belal\bin\xavier-panel.exe"
```

**Solución permanente**:
```powershell
.\scripts\fix-windows-installation.ps1
```

## 📁 Ubicaciones de Archivos

### Binarios

```
C:\Users\belal\bin\
├── xavier-panel.exe       ← APP PRINCIPAL (Tauri UI)
├── xavier.exe             ← Servidor backend standalone
└── xavier-tui.exe         ← TUI Dashboard
```

O en instalación completa:

```
C:\Program Files\SouthWest AI Labs\Xavier\
├── xavier-panel.exe       ← APP PRINCIPAL
├── xavier-server.exe      ← Servidor standalone
└── xavier-tui.exe         ← TUI Dashboard
```

### Datos de Usuario

```
%APPDATA%\com.swalsystems.xavier\
└── data\
    ├── xavier_memory.db   ← Base de datos principal
    ├── metrics.db         ← Métricas
    └── logs\              ← Logs de aplicación
```

### Configuración

```
e:\scripts-python\xavier\
├── .env                   ← Variables de entorno
└── config\
    └── xavier.config.json ← Configuración
```

## 🎨 Interfaces Disponibles

| Interfaz | Comando | Uso Recomendado |
|----------|---------|-----------------|
| **Panel UI** (Tauri) | `xavier-panel.exe` | ✅ **Windows Desktop** (Principal) |
| TUI Dashboard | `xavier-tui.exe` | Terminal interactiva |
| Solo Servidor | `xavier.exe http` | Linux servers, Docker, desarrollo |

## 🔐 Variables de Entorno

Xavier carga automáticamente las variables desde `.env`:

```env
XAVIER_TOKEN=...
XAVIER_EMBEDDING_PROVIDER_MODE=cloud
XAVIER_EMBEDDING_API_KEY=...
XAVIER_DATA_DIR=...
```

No necesitas configurar estas variables manualmente cuando usas el Panel UI.

## 📚 Recursos Adicionales

- **Documentación completa**: `docs/XAVIER_UI_SETUP.md`
- **Instalador**: `installer/README.md`
- **Script de corrección**: `scripts/fix-windows-installation.ps1`
- **Arquitectura**: `.gitcore/ARCHITECTURE.md`

---

**Última actualización**: 2026-07-07  
**Versión**: 0.12.0  
**Sistema**: Windows
