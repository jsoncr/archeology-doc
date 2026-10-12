# Arqueology Doc para VS Code

Interfaz visual sobre el CLI [`adoc`](../README.md): ver lo generado y ejecutar tareas sin recordar
comandos. Funciona también en editores basados en VS Code (Cursor, Windsurf, Devin Desktop).

## Requisitos

- El CLI instalado: `cargo install --path crates/arqueology-cli` (o la ruta en `arqueologyDoc.path`).
- Una carpeta con `arqueology-doc.toml` (o `adoc init` desde la propia extensión).

## Qué ofrece

**Proyecto activo**: se toma del registro global `~/.arqueology-doc/` (lo que muestra
`adoc projects list`). Funciona aunque abras una subcarpeta del proyecto, la carpeta padre o un monorepo
con varios proyectos; la vista **Proyectos** los lista todos (progreso, agente, hallazgos) y un clic
cambia el activo. La elección se recuerda por workspace.

**Panel "Arqueology"** (barra de actividad):

| Vista | Contenido | Acciones |
|-------|-----------|----------|
| Proyectos | Registrados en `~/.arqueology-doc/`; el activo en verde | clic: usar ese proyecto; abrir en ventana nueva |
| Tareas | Agrupadas por estado: pregunta, fallidas, pendientes, manuales, hechas | ▶ ejecutar, ejecutar interactivo, responder pregunta, marcar hecha, ver log, copiar comando |
| Preguntas | Del agente (clic: responder), para el equipo técnico y FAQ | botón: preguntar al agente |
| Hallazgos | Por severidad; cada evidencia `ruta:línea` abre el código en esa línea | clic: abre la nota |
| Conocimiento | Notas por tipo; las desactualizadas se marcan | clic: abre la nota |
| Specs | Requisitos de OpenSpec con su estado de cumplimiento | clic: abre la evidencia |

**Dashboard** (clic en la barra de estado, botón en Tareas/Proyectos o `adoc: Abrir dashboard`), con pestañas:

| Pestaña | Contenido |
|---------|-----------|
| Resumen | Progreso, tareas por hacer, hallazgos por severidad, preguntas pendientes, funcionalidades sin tests, tiempo de agente y estimación restante; "Necesita atención" (preguntas, fallidas, hallazgos altos, notas desactualizadas); agente y specs; extracto del overview |
| Tareas | Estado, módulos, duración, intentos y última ejecución de cada tarea; ejecutar, sesión interactiva, responder y log |
| Hallazgos | Filtros por severidad y estado; evidencias que abren el código en la línea |
| Documentación | Notas por tipo, desactualizadas y funcionalidades sin tests; ver sitio y reporte |
| Preguntas | Preguntar al agente; responder en el panel a las del agente; dudas para el equipo; FAQ |
| Grafo | El grafo del conocimiento |

**Grafo del conocimiento** (pestaña del dashboard, botón en la vista Conocimiento o `adoc: Grafo del conocimiento`): como el
grafo de Obsidian. Notas coloreadas por tipo (hallazgos por severidad, desactualizadas con borde ámbar),
aristas por wikilinks, `related`, módulos y tests; los archivos de código citados como evidencia se
pueden mostrar con su filtro. Zoom con la rueda, arrastrar para mover, pasar el cursor resalta los
vecinos y un clic abre la nota (o el código en la línea citada).

**Barra de estado**: `adoc 43% · devin ? 1 ✗ 2` — progreso, agente activo, preguntas y fallidas.
Clic: abre el dashboard (el menú de acciones está en `adoc: Acciones…`).

**Editor**: las líneas citadas por un hallazgo abierto llevan una marca en el margen; al pasar el cursor
se ve el hallazgo con un enlace a su nota (`arqueologyDoc.decorations` para desactivarlo).

**Paleta** (`adoc: …`): ver resultado (preview), reporte HTML, preguntar al agente, escanear, planificar, ejecutar, reintentar, validar, sync, sitio, doctor y
cambiar agente o modelo.

Los comandos largos se ejecutan en el terminal integrado (`adoc`), con su progreso visible; el panel se
actualiza solo cuando cambian las tareas o el vault.

## Desarrollo

```bash
cd extension
npm install
npm run compile      # o npm run watch
npm run package      # genera arqueology-doc-<versión>.vsix
code --install-extension arqueology-doc-0.1.0.vsix
```

Para depurar: abrir `extension/` en VS Code y pulsar F5.
