# Xavier Windows Installer

Este directorio contiene los archivos para crear el instalador profesional de Xavier para Windows.

## 🎯 Qué Instala

El instalador de Xavier incluye:

- **Xavier Panel UI** (Tauri) - Interfaz gráfica principal con icono en bandeja del sistema
- **Xavier Backend Server** - Servidor HTTP/MCP standalone
- **Xavier TUI Dashboard** - Dashboard interactivo de terminal
- Accesos directos en el menú inicio
- Opción de inicio automático con Windows
- Opción de agregar a PATH del sistema

## 📋 Requisitos Previos

### Para Construir el Instalador

**Herramienta de Instalación** (una de las siguientes):
- **Inno Setup v6+** (Recomendado): [Descargar Inno Setup](https://jrsoftware.org/isdl.php)
- **WiX Toolset v3.11+**: [Descargar WiX](https://wixtoolset.org/releases/)

**Herramientas de Desarrollo**:
- **Rust**: Para compilar los binarios de Xavier
- **Node.js v22+**: Para construir el Panel UI
- **pnpm**: `npm install -g pnpm`

## 🚀 Instrucciones de Construcción

### Opción A: Todo Automático (Recomendado)

1. Abre PowerShell (no requiere Admin)
2. Navega al directorio installer:
   ```powershell
   cd installer
   ```
3. Ejecuta el script de build:
   ```powershell
   .\build-installer.ps1
   ```

El script automáticamente:
- ✅ Construye los binarios de backend si no existen
- ✅ Construye el Panel UI de Tauri si no existe
- ✅ Detecta si tienes Inno Setup o WiX
- ✅ Compila el instalador

### Opción B: Build Manual (Paso a Paso)

Si prefieres construir los componentes manualmente:

#### 1. Construir Backend
```powershell
cd xavier
cargo build --release --features cli-interactive
```

#### 2. Construir Panel UI (Tauri)
```powershell
cd panel-ui
pnpm install
pnpm tauri build
```

#### 3. Construir Instalador
```powershell
cd installer
.\build-installer.ps1
```

## 📦 Salida

### Con Inno Setup
- **Archivo**: `installer/Output/XavierSetup.exe`
- **Tipo**: Instalador ejecutable (.exe)
- **Tamaño**: ~40-60 MB (incluye todos los componentes)

### Con WiX Toolset
- **Archivo**: `installer/XavierInstaller.msi`
- **Tipo**: Windows Installer Package (.msi)
- **Tamaño**: ~40-60 MB

## ✨ Características del Instalador

### Durante la Instalación

- 📁 Instala en `C:\Program Files\SouthWest AI Labs\Xavier`
- 🔧 Opción: Agregar Xavier a PATH del sistema
- 🖥️ Opción: Crear icono en el escritorio
- 🚀 Opción: Iniciar Xavier automáticamente con Windows

### Accesos Directos Creados

En el Menú Inicio > Xavier:
- **Xavier** - Abre el Panel UI (interfaz gráfica principal)
- **Xavier TUI Dashboard** - Abre el dashboard de terminal
- **Xavier Server** - Inicia solo el servidor backend

### Auto-Inicio

Si seleccionas la opción de inicio automático:
- Xavier Panel UI se iniciará automáticamente al hacer login
- El icono aparecerá en la bandeja del sistema
- El servidor backend se iniciará automáticamente en segundo plano

## 🔍 Solución de Problemas

### "Binary not found" durante build

**Causa**: Los binarios de Rust no están compilados.

**Solución**: El script los construirá automáticamente. Si falla:
```powershell
cd ..
cargo build --release --features cli-interactive
```

### "Panel UI build failed"

**Causas comunes**:
1. Node.js no instalado o versión antigua (requiere ≥22.12.0)
2. pnpm no instalado
3. Dependencias no instaladas

**Soluciones**:
```powershell
# Verificar versiones
node --version  # Debe ser ≥22.12.0
pnpm --version  # Si no existe: npm install -g pnpm

# Reinstalar dependencias
cd panel-ui
pnpm install --force
pnpm tauri build
```

### "Neither WiX nor Inno Setup found"

**Causa**: No tienes instalada ninguna herramienta de construcción de instaladores.

**Solución**: Instala Inno Setup (más fácil):
1. Descarga de https://jrsoftware.org/isdl.php
2. Instala con opciones por defecto
3. Reinicia PowerShell
4. Ejecuta `.\build-installer.ps1` de nuevo

## 📝 Notas Técnicas

### Arquitectura del Instalador

```
XavierSetup.exe / XavierInstaller.msi
├── xavier-panel.exe       (Panel UI Tauri - APP PRINCIPAL)
│   └── Incluye servidor backend embebido
├── xavier-server.exe      (Servidor standalone)
├── xavier-tui.exe         (TUI Dashboard)
└── xavier.config.json     (Configuración)
```

### Por Qué Panel UI es la App Principal

En instalaciones de Windows, Xavier **siempre** debe iniciar con la interfaz gráfica (Panel UI):
- ✅ Icono en bandeja del sistema para acceso fácil
- ✅ Interfaz visual moderna
- ✅ El servidor backend se inicia automáticamente dentro de Tauri
- ✅ Mejor experiencia de usuario en desktop

El modo "solo servidor" (`xavier-server.exe`) es solo para:
- Instalaciones de servidor sin GUI
- Desarrollo y debugging
- Uso avanzado vía CLI

### Estructura Post-Instalación

```
C:\Program Files\SouthWest AI Labs\Xavier\
├── xavier-panel.exe        # APP PRINCIPAL (Tauri)
├── xavier-server.exe       # Servidor standalone
├── xavier-tui.exe          # TUI
├── xavier.config.json      # Config
└── Uninstaller.exe         # Desinstalador

%APPDATA%\com.swalsystems.xavier\
└── data\                   # Datos de usuario
    └── xavier_memory.db    # Base de datos de memoria

Menú Inicio > Xavier\
├── Xavier                  # Inicia Panel UI
├── Xavier TUI Dashboard    # Inicia TUI
└── Xavier Server           # Inicia solo servidor

Startup (opcional)\
└── Xavier                  # Auto-inicio
```

## 🎯 Para Usuarios Finales

Si eres un usuario final que quiere instalar Xavier:

1. Descarga `XavierSetup.exe` de la página de releases
2. Ejecuta el instalador
3. Sigue las instrucciones en pantalla
4. ✅ Selecciona "Start Xavier on login" para que inicie automáticamente
5. Al finalizar, Xavier se iniciará con su icono en la bandeja del sistema

**Ubicación del icono**: Busca en la bandeja del sistema (esquina inferior derecha de Windows, junto al reloj)

---

**Última actualización**: 2026-07-07  
**Versión**: 0.12.0
