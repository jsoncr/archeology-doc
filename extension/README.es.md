# Archeology Doc para VS Code

*[English](README.md) · Español*

Interfaz visual sobre el CLI [`adoc`](../README.md): ver lo generado y ejecutar tareas sin recordar
comandos. Funciona también en editores basados en VS Code (Cursor, Windsurf, Devin Desktop).

## Requisitos

- El CLI `adoc`. Si no está, la extensión ofrece instalarlo al iniciar (instalador oficial de
  `https://archeology-doc.com/download`, con verificación SHA-256; la URL se cambia con
  `archeologyDoc.downloadUrl`). Para instalarlo o actualizarlo después, ejecuta *adoc: Instalar
  herramientas* (`adoc.installTools`) o vuelve a correr el instalador. También se puede indicar la ruta en
  `archeologyDoc.path`; desde un clon del repositorio, `cargo install --path crates/archeology-cli --force`.
- Para el sitio de documentación: git, Node 18+ y VitePress. La comprobación al iniciar ofrece
  `adoc site setup` o enlaza la descarga de lo que falte (comando *adoc: Instalar herramientas*;
  se desactiva con `archeologyDoc.checkToolsOnStartup`).
- Una carpeta con `archeology-doc.toml` (o `adoc init` desde la propia extensión).

## Qué ofrece

**Proyecto activo**: se toma del registro global `~/.archeology-doc/` (lo que muestra
`adoc projects list`). Funciona aunque abras una subcarpeta del proyecto, la carpeta padre o un monorepo
con varios proyectos; la vista **Proyectos** los lista todos (progreso, agente, hallazgos) y un clic
cambia el activo. La elección se recuerda por workspace.

**Panel "Archeology"** (barra de actividad):

| Vista | Contenido | Acciones |
|-------|-----------|----------|
| Proyectos | Registrados en `~/.archeology-doc/`; el activo en verde | clic: usar ese proyecto; abrir en ventana nueva |
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
pueden mostrar con su filtro. Búsqueda (Enter centra la primera coincidencia), mostrar u ocultar tipos
desde la leyenda, zoom con la rueda o `+`/`−`, arrastrar para mover; pasar el cursor resalta los vecinos y
un clic abre la nota (o el código en la línea citada). El renderizador es compartido con el CLI
(`adoc graph --open`, el reporte HTML y el sitio): su única copia está en `crates/archeology-core/assets/`
y `npm run compile` la copia a `media/`.

**Barra de estado**: `adoc 43% · devin ? 1 ✗ 2` — progreso, agente activo, preguntas y fallidas.
Clic: abre el dashboard (el menú de acciones está en `adoc: Acciones…`).

**Editor**: las líneas citadas por un hallazgo abierto llevan una marca en el margen; al pasar el cursor
se ve el hallazgo con un enlace a su nota (`archeologyDoc.decorations` para desactivarlo).

**Paleta** (`adoc: …`): ver resultado (preview), reporte HTML, preguntar al agente, escanear, planificar, ejecutar, reintentar, validar, sync, sitio, doctor y
cambiar agente o modelo.

Los comandos largos se ejecutan en el terminal integrado (`adoc`), con su progreso visible; el panel se
actualiza solo cuando cambian las tareas o el vault.

## Idioma

La extensión está en inglés y en español. El ajuste `archeologyDoc.language` (`auto` | `en` | `es`,
por defecto `auto`) decide el idioma; `auto` usa el idioma de VS Code (español si empieza por `es`,
si no, inglés). Al cambiarlo se actualizan al momento las vistas, la barra de estado y el dashboard.

- **Siguen el ajuste**: avisos, quick picks, vistas del panel, barra de estado, dashboard y grafo, y los
  mensajes del CLI: la extensión pasa `ADOC_LANG=<idioma>` a cada `adoc` que ejecuta (en segundo plano y
  en el terminal; si cambias el idioma, el siguiente comando abre un terminal nuevo).
- **Siguen el idioma de VS Code**: los títulos de comandos, menús y vistas, y las descripciones de los
  ajustes (vienen de `package.nls*.json`; VS Code no permite cambiarlos desde una extensión).
- **Idioma de la documentación**: `adoc: Inicializar proyecto` pregunta en qué idioma se genera la
  documentación (`adoc init --lang en|es`), independiente del idioma de la interfaz.

## Desarrollo

```bash
cd extension
npm install
npm run compile      # o npm run watch
npm run package      # genera archeology-doc-<versión>.vsix
code --install-extension archeology-doc-0.1.0.vsix
```

Para depurar: abrir `extension/` en VS Code y pulsar F5.

**Añadir un texto**: la misma clave en `en` y `es` de `src/strings.ts` (TypeScript exige las dos) y usarla
con `t('clave', { parámetro })` (en `media/*.js`, las claves con prefijo `web.`, `kind.` u `origin.`
llegan al webview). Títulos de `package.json`: `%clave%` en `package.nls.json` (inglés) y
`package.nls.es.json`. `npm run compile` ejecuta `scripts/check-i18n.js`, que comprueba que ambos idiomas
tienen las mismas claves y marcadores.
