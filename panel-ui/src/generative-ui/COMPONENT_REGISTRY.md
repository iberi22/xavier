# Component Registry — Generative UI Interno para Xavier2

> Sistema declarativo de componentes UI para el agente interno.
> El agente genera JSON → el renderer convierte a HTML/CSS interactivo.

---

## 🎯 Filosofía

**El agente NO genera HTML crudo.** Genera estructuras JSON declarativas que el renderer convierte a componentes visuales. Esto garantiza:

- ✅ Seguridad (no XSS)
- ✅ Consistencia visual
- ✅ Validación estricta
- ✅ Tamaño mínimo de respuesta (JSON compacto)

---

## 📦 Componentes Disponibles

### 1. `text-response`
Respuesta simple con markdown.

```json
{
  "component": "text-response",
  "content": "Los proyectos activos son:",
  "style": "heading"
}
```

### 2. `data-table`
Tabla de datos con columnas, filas y acciones.

```json
{
  "component": "data-table",
  "title": "Proyectos Activos",
  "columns": [
    {"key": "name", "label": "Nombre", "width": "30%"},
    {"key": "status", "label": "Estado", "width": "20%"},
    {"key": "progress", "label": "Progreso", "width": "20%"},
    {"key": "actions", "label": "Acciones", "width": "30%"}
  ],
  "rows": [
    {"name": "Xavier2", "status": "ACTIVE", "progress": 85, "actions": ["Ver", "Editar"]},
    {"name": "OrionHealth", "status": "IN_PROGRESS", "progress": 30, "actions": ["Ver"]}
  ],
  "sortable": true,
  "searchable": true
}
```

### 3. `info-card`
Tarjeta informativa con icono, valor y color.

```json
{
  "component": "info-card",
  "title": "Proyectos",
  "value": "12",
  "icon": "folder",
  "color": "blue",
  "trend": "+2 esta semana"
}
```

### 4. `form-input`
Formulario para recolectar datos del usuario.

```json
{
  "component": "form-input",
  "title": "Nuevo Proyecto",
  "fields": [
    {"name": "project_name", "label": "Nombre", "type": "text", "required": true},
    {"name": "description", "label": "Descripción", "type": "textarea", "required": false},
    {"name": "priority", "label": "Prioridad", "type": "select", "options": ["Baja", "Media", "Alta"]}
  ],
  "submit_label": "Crear Proyecto",
  "cancel_label": "Cancelar"
}
```

### 5. `progress-bar`
Indicador de progreso con etiqueta.

```json
{
  "component": "progress-bar",
  "label": "Xavier2 Completion",
  "percent": 85,
  "status": "ACTIVE",
  "show_label": true
}
```

### 6. `code-block`
Bloque de código con syntax highlighting.

```json
{
  "component": "code-block",
  "language": "rust",
  "code": "fn main() {\n    println!(\"Hello Xavier2\");\n}",
  "collapsible": true,
  "filename": "main.rs"
}
```

### 7. `timeline`
Línea de tiempo de eventos.

```json
{
  "component": "timeline",
  "title": "Historial del Proyecto",
  "events": [
    {"date": "2026-05-20", "title": "Inicio", "description": "Setup inicial", "status": "completed"},
    {"date": "2026-05-24", "title": "Investigación", "description": "Generative UI research", "status": "completed"},
    {"date": "2026-05-25", "title": "Implementación", "description": "MVP renderer", "status": "in_progress"}
  ]
}
```

### 8. `confirm-dialog`
Diálogo de confirmación.

```json
{
  "component": "confirm-dialog",
  "message": "¿Eliminar el proyecto 'Xavier2'?",
  "description": "Esta acción no se puede deshacer.",
  "confirm_label": "Eliminar",
  "confirm_style": "danger",
  "cancel_label": "Cancelar"
}
```

### 9. `status-badge`
Badge de estado con variantes de color.

```json
{
  "component": "status-badge",
  "text": "ACTIVE",
  "variant": "success"
}
```

### 10. `chart-bar`
Gráfico de barras simple (canvas).

```json
{
  "component": "chart-bar",
  "title": "Uso de Recursos",
  "labels": ["CPU", "RAM", "Disk", "Network"],
  "values": [65, 82, 45, 30],
  "colors": ["blue", "green", "yellow", "purple"]
}
```

### 11. `list-group`
Lista de items con acciones.

```json
{
  "component": "list-group",
  "title": "Tareas Pendientes",
  "items": [
    {"label": "Revisar PR #190", "status": "pending", "actions": ["Ver", "Aprobar"]},
    {"label": "Merge a develop", "status": "blocked", "actions": ["Ver"]}
  ]
}
```

---

## 🔧 JSON Schema Base

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["component"],
  "properties": {
    "component": {
      "type": "string",
      "enum": [
        "text-response", "data-table", "info-card", "form-input",
        "progress-bar", "code-block", "timeline", "confirm-dialog",
        "status-badge", "chart-bar", "list-group"
      ]
    }
  }
}
```

Cada componente tiene su propio schema específico que extiende este base.

---

## 🎨 Prompt del Agente

Cuando el agente detecta que una respuesta requiere UI, debe generar un bloque JSON:

```
Cuando el usuario solicite información estructurada, datos tabulados, 
formularios, o cualquier interacción que no sea texto plano, DEBES responder 
con un bloque JSON que siga el Component Registry de Xavier2.

Ejemplo:
Usuario: "Muestra los proyectos activos"
Respuesta del agente:
{
  "component": "data-table",
  "title": "Proyectos Activos",
  "columns": [...],
  "rows": [...]
}

El renderer se encargará de convertir esto a HTML interactivo.
```

---

## 📁 Archivos del Sistema

```
src/generative-ui/
├── COMPONENT_REGISTRY.md      # Este archivo - documentación
├── schemas/
│   ├── base.schema.json       # Schema base
│   ├── data-table.schema.json # Schema específico
│   ├── form-input.schema.json
│   └── ...
├── renderer.js                # Motor de renderizado (300-500 loc)
├── components.css             # Estilos de componentes
└── index.js                   # Entry point
```

---

*Generative UI System v1.0 | Xavier2*
