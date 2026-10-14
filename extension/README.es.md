# Archeology Doc para VS Code

*[English](https://github.com/jsoncr/archeology-doc/blob/main/extension/README.md) · Español*

**Entiende un código legado sin salir del editor.** Archeology Doc es la interfaz visual de
[`adoc`](https://github.com/jsoncr/archeology-doc#readme), un CLI que hace el inventario de un repositorio,
divide el trabajo en tareas pequeñas y hace que tu agente de IA (Claude Code, Codex, Gemini, Copilot,
Cursor, Devin…) lo documente: módulos, funcionalidades, cobertura de tests, hallazgos con evidencia
`ruta:línea` y un grafo del conocimiento. La extensión muestra todo eso en un dashboard y en la barra
lateral, y ejecuta cada paso por ti.

![Dashboard de Archeology Doc: progreso, hallazgos abiertos, preguntas pendientes y lo que necesita atención](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-summary-dark-es.png)

También funciona en editores basados en VS Code (Cursor, Windsurf, Devin Desktop).

## Instalación

- **Marketplace**: [Archeology Doc](https://marketplace.visualstudio.com/items?itemName=jcruz.archeology-doc)
  (`jcruz.archeology-doc`), o busca *Archeology Doc* en la vista de extensiones.
- **Línea de comandos**:

  ```bash
  code --install-extension jcruz.archeology-doc
  ```

La extensión se activa en carpetas que contienen `archeology-doc.toml`, y solo en
[workspaces de confianza](https://code.visualstudio.com/docs/editor/workspace-trust), porque ejecuta el
CLI `adoc` sobre el proyecto abierto.

## El CLI `adoc`

La extensión no tiene lógica de análisis propia: lee `adoc status --format json` y ejecuta comandos de
`adoc`. `adoc` es un único binario (macOS, Linux, Windows) que hace el trabajo determinista (inventario,
planificación de tareas, validación de cada nota y de cada `ruta:línea` citada, detección de cambios) y
delega la lectura del código al agente que ya usas.

![adoc status en una terminal: proyecto, estructura generada, progreso, tareas pendientes y siguiente paso](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/cli-status.png)

**Instalarlo** (binario precompilado, verificado con SHA-256):

```bash
curl -fsSL https://archeology-doc.com/download/install.sh | sh     # macOS / Linux
```

```powershell
irm https://archeology-doc.com/download/install.ps1 | iex          # Windows (PowerShell)
```

No hace falta hacerlo a mano: si falta `adoc` al iniciar, la extensión ofrece instalarlo en una terminal
integrada con el mismo instalador oficial. Después, **adoc: Instalar herramientas (adoc, VitePress)**
revisa todo y ofrece actualizar `adoc` (ejecuta `adoc upgrade --yes` y, si falla, el instalador). Desde
una terminal:

```bash
adoc upgrade --check     # solo informa: versión actual → última publicada
adoc upgrade             # descarga, verifica y sustituye el binario
```

**Comandos principales** (todos están también en la extensión):

| Comando | Qué hace |
| --- | --- |
| `adoc init` | Crea `archeology-doc.toml` y la estructura `knowledge/` + `docs/`, y registra el proyecto |
| `adoc scan` | Inventario determinista: lenguajes, módulos, puntos de entrada, tests, marcadores TODO/FIXME, historial de git |
| `adoc plan` | Divide el trabajo en tareas acotadas (overview, una por módulo, contradicciones, manual de usuario…) |
| `adoc inspect` | Ejecuta las tareas pendientes con el agente configurado y valida lo que escribe (`--resume` reintenta) |
| `adoc status` | Foto del proyecto y siguiente paso sugerido (`--format json` es lo que lee la extensión) |
| `adoc check` | Lista de lo que está hecho y lo que falta; código de salida para CI |
| `adoc sync` | Detecta cambios en el código, marca como desactualizadas las notas afectadas y reabre sus tareas |
| `adoc graph` | Grafo del conocimiento en JSON, texto o HTML interactivo (`--open`) |
| `adoc review` | Resumen de hallazgos, cobertura de tests y progreso |
| `adoc site dev` / `adoc preview` | Sitio de documentación VitePress; `preview` muestra el resultado en el navegador |

Documentación completa del CLI: [github.com/jsoncr/archeology-doc](https://github.com/jsoncr/archeology-doc#readme)
· Sitio web: [archeology-doc.com](https://archeology-doc.com)

## Inicio rápido

1. Instala la extensión y el CLI (ver arriba) y abre la carpeta del proyecto que quieres documentar.
2. Ejecuta **adoc: Inicializar proyecto** desde la paleta de comandos y elige el idioma de la
   documentación generada (inglés o español).
3. En la vista **Archeology** de la barra de actividad, la vista Tareas te guía: **1. Escanear** →
   **2. Planificar**.
4. Pulsa **▶ Ejecutar tareas** (o **adoc: Ejecutar tareas (inspect)**). Cada tarea se ejecuta en la terminal
   `adoc` con tu agente; las vistas y el dashboard se actualizan a medida que se escriben las notas.
5. Abre el dashboard (clic en `adoc` en la barra de estado) para revisar hallazgos, responder las preguntas
   del agente y explorar el grafo. **adoc: Ver resultado (preview)** abre el sitio de documentación.

Para cambiar de agente o de modelo, usa **adoc: Cambiar agente o modelo** (o el botón *Agente* del
dashboard).

## Funcionalidades

### Dashboard

Clic en la barra de estado, botón de dashboard en las vistas Tareas o Proyectos, o **adoc: Abrir
dashboard**. La cabecera muestra el agente, el perfil y el progreso, con botones para ejecutar tareas,
reintentar las fallidas, cambiar de agente, abrir el sitio y generar el reporte HTML.

| Pestaña | Contenido |
| --- | --- |
| Resumen | Progreso, tareas por hacer, hallazgos por severidad, preguntas pendientes, funcionalidades sin tests, tiempo de agente y estimación restante; *Necesita atención* (preguntas del agente, tareas fallidas, hallazgos altos, notas desactualizadas); extracto del overview |
| Proyecto | Inventario: archivos, tamaño, líneas, lenguajes, módulos, tests, marcadores, stack, manifiestos, puntos de entrada, CI y documentación existente |
| Tareas | Estado, tipo, módulos, duración, intentos y última ejecución de cada tarea; ejecutar, sesión interactiva, responder y log |
| Hallazgos | Filtros por severidad y estado; cada evidencia `ruta:línea` abre el código en esa línea |
| Documentación | Notas por tipo, notas desactualizadas y funcionalidades sin reporte de tests; ver sitio, reporte HTML, grafo |
| Preguntas | Preguntar al agente; responder en el panel las preguntas del agente; preguntas para el equipo técnico; FAQ |
| Grafo | El grafo del conocimiento (abajo) |

![Pestaña Tareas: estado, duración, intentos y acciones de cada tarea, incluida una que espera respuesta](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-tasks-dark.png)

![Pestaña Hallazgos: hallazgos por severidad con su tipo, estado y evidencia clicable](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-findings-dark.png)

### Cuando el agente pregunta

Los agentes se ejecutan sin interacción. Cuando uno no puede decidir algo, la tarea termina en estado
*pregunta* en lugar de adivinar. Respóndela en la pestaña Preguntas, en la vista Tareas (**Responder
pregunta**) o en vivo con **Ejecutar en modo interactivo**; la respuesta se guarda con la tarea y esta queda
lista para ejecutarse de nuevo. **Preguntar al agente** envía tu propia pregunta; la respuesta, con
evidencia, se guarda en `knowledge/faq/`.

![Pestaña Preguntas: preguntar al agente, responder una pregunta que bloquea una tarea, preguntas para el equipo y FAQ](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-questions-dark.png)

### Grafo del conocimiento

Pestaña del dashboard, botón en la vista Conocimiento o **adoc: Grafo del conocimiento**. Notas coloreadas
por tipo (hallazgos por severidad, notas desactualizadas con borde ámbar), aristas de wikilinks,
`related`, módulos y tests; los archivos de código citados como evidencia se pueden mostrar desde la
leyenda. Búsqueda (Enter centra la primera coincidencia), mostrar u ocultar tipos, zoom con la rueda o
`+`/`−`, arrastrar para mover; al pasar el cursor se resaltan los vecinos y un clic abre la nota, o el
código en la línea citada. Es el mismo renderizador de `adoc graph --open`, el reporte HTML y el sitio.

![Grafo del conocimiento con tema oscuro: overview, módulo, funcionalidades, hallazgos, tests y archivos citados](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/knowledge-graph-dark.png)

El dashboard sigue el tema de color de VS Code:

![Resumen del dashboard con tema claro](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-summary-light.png)

### Barra lateral Archeology

| Vista | Contenido | Acciones |
| --- | --- | --- |
| Proyectos | Proyectos del registro global `~/.archeology-doc/` (progreso, agente, hallazgos); el activo en verde | clic: usar ese proyecto; abrir en ventana nueva |
| Tareas | Agrupadas por estado: esperan tu respuesta, fallidas, en curso, pendientes, manuales, hechas | ▶ ejecutar, ejecutar en modo interactivo, responder, marcar como hecha, ver log, copiar comando |
| Hallazgos | Por severidad; cada evidencia `ruta:línea` abre el código en esa línea | clic: abre la nota |
| Preguntas | Del agente (clic: responder), para el equipo técnico y FAQ | preguntar al agente |
| Conocimiento | Notas por tipo; las desactualizadas se marcan | clic: abre la nota; grafo |
| Specs | Requisitos de OpenSpec con su estado de cumplimiento (solo si el proyecto tiene specs) | clic: abre la evidencia |

El proyecto activo sale del registro global, así que funciona si abres una subcarpeta del proyecto, su
carpeta padre o un monorepo con varios proyectos; la elección se recuerda por workspace.

### Barra de estado y editor

- **Barra de estado**: `adoc 50% · claude ? 1 ✗ 2`: progreso, agente activo (y modelo), preguntas y tareas
  fallidas. Clic: abre el dashboard; **adoc: Acciones…** lista todas las acciones.
- **Editor**: las líneas citadas como evidencia por un hallazgo abierto llevan una marca en el margen con el
  color de su severidad; al pasar el cursor se ve el hallazgo con un enlace a su nota
  (`archeologyDoc.decorations`).

Los comandos largos se ejecutan en la terminal integrada `adoc`, con el progreso visible; las vistas se
actualizan cuando cambian las tareas, el estado o las notas.

## Comandos

Todos los comandos están en la paleta bajo la categoría `adoc:`, salvo los marcados *(menú contextual)*,
que actúan sobre un elemento del árbol. Los títulos siguen el idioma de VS Code.

| Comando | ID | Ejecuta |
| --- | --- | --- |
| Abrir dashboard | `adoc.dashboard` | — |
| Actualizar | `adoc.refresh` | `adoc projects list`, `adoc status` |
| Acciones… | `adoc.menu` | selector con todas las acciones |
| Inicializar proyecto | `adoc.init` | `adoc init --lang en\|es` |
| Escanear (scan) | `adoc.scan` | `adoc scan` |
| Planificar tareas (plan) | `adoc.plan` | `adoc plan` |
| Rehacer el plan (plan --force) | `adoc.planForce` | `adoc plan --force` |
| Ejecutar tareas (inspect) | `adoc.inspect` | `adoc inspect` |
| Reintentar fallidas (inspect --resume) | `adoc.resume` | `adoc inspect --resume` |
| Resumen (review) | `adoc.review` | `adoc review` |
| Validar (check) | `adoc.check` | `adoc check` |
| Detectar cambios (sync) | `adoc.sync` | `adoc sync` |
| Verificar requisitos (doctor) | `adoc.doctor` | `adoc doctor` |
| Sitio: servidor de desarrollo | `adoc.siteDev` | `adoc site dev` |
| Sitio: build | `adoc.siteBuild` | `adoc site build` |
| Ver resultado (preview) | `adoc.preview` | `adoc preview` |
| Reporte HTML | `adoc.report` | `adoc report` |
| Preguntar al agente | `adoc.ask` | `adoc ask` |
| Grafo del conocimiento | `adoc.graph` | abre la pestaña Grafo |
| Cambiar agente o modelo | `adoc.selectAgent` | `adoc agent use` |
| Cambiar de proyecto | `adoc.selectProject` | — |
| Instalar herramientas (adoc, VitePress) | `adoc.installTools` | instalador / `adoc upgrade` / `adoc site setup` |
| Ejecutar tarea *(menú contextual)* | `adoc.task.run` | `adoc task run <id>` |
| Ejecutar en modo interactivo *(menú contextual)* | `adoc.task.runInteractive` | `adoc task run <id> --interactive` |
| Responder pregunta *(menú contextual)* | `adoc.task.answer` | `adoc task answer <id>` |
| Marcar como hecha (valida la salida) *(menú contextual)* | `adoc.task.done` | `adoc task done <id>` |
| Ver log *(menú contextual)* | `adoc.task.openLog` | — |
| Copiar comando *(menú contextual)* | `adoc.task.copyCommand` | — |
| Abrir carpeta en ventana nueva *(menú contextual)* | `adoc.openProjectFolder` | — |
| Responder pregunta del agente *(vista Preguntas)* | `adoc.answerQuestion` | `adoc task answer <id>` |

## Configuración

| Ajuste | Por defecto | Descripción |
| --- | --- | --- |
| `archeologyDoc.language` | `auto` | Idioma de la interfaz de la extensión y de los mensajes de `adoc` que ejecuta: `auto`, `en`, `es` (ver [Idioma](#idioma)) |
| `archeologyDoc.path` | `""` | Ruta al binario `adoc`. Vacío: la carpeta del instalador (`~/.local/bin`; en Windows `%LOCALAPPDATA%\Programs\archeology-doc\bin`), luego `~/.cargo/bin` y luego el `PATH`. Solo se lee de la configuración de usuario, nunca de la del workspace |
| `archeologyDoc.decorations` | `true` | Marcar en el editor las líneas citadas como evidencia de hallazgos |
| `archeologyDoc.checkToolsOnStartup` | `true` | Al iniciar, comprobar que `adoc`, git, Node y VitePress estén instalados y ofrecer instalar lo que falte |
| `archeologyDoc.downloadUrl` | `""` | URL https de la carpeta `/download` desde la que se instala `adoc`. Vacío: `https://archeology-doc.com/download`. Solo configuración de usuario |

## Idioma

La extensión está disponible en inglés y español. `archeologyDoc.language` (`auto` | `en` | `es`, por
defecto `auto`) elige el idioma; `auto` sigue el idioma de VS Code (español si empieza por `es`; si no,
inglés). Al cambiarlo se actualizan al momento las vistas, la barra de estado y el dashboard.

- **Sigue el ajuste**: avisos, selectores rápidos, vistas de la barra lateral, barra de estado, dashboard y
  grafo, y los mensajes del CLI: la extensión pasa `ADOC_LANG=<idioma>` a cada `adoc` que ejecuta (tras un
  cambio, el siguiente comando abre una terminal nueva).
- **Sigue el idioma de VS Code**: títulos de comandos, menús y vistas y descripciones de los ajustes (VS
  Code no permite a una extensión cambiarlos en tiempo de ejecución).
- **Idioma de la documentación**: se elige en **adoc: Inicializar proyecto** (`adoc init --lang en|es`),
  independiente del idioma de la interfaz.

## Requisitos

- VS Code 1.85 o posterior (o un editor compatible), en un workspace de confianza.
- El CLI `adoc` (la extensión ofrece instalarlo).
- Un agente de IA con modo headless para `adoc inspect` (Claude Code, Codex, Gemini CLI, Copilot CLI,
  Cursor, Devin, Aider, Amp, Goose, OpenCode, Qwen…), o el modo `manual`, que exporta los prompts.
- git. Para el sitio de documentación: Node 18+ y VitePress (la comprobación inicial ofrece
  `adoc site setup`).

## Solución de problemas

| Síntoma | Qué hacer |
| --- | --- |
| "No se encontró el CLI `adoc`" | Pulsa **Instalar**, o instálalo desde una terminal (ver [El CLI `adoc`](#el-cli-adoc)). Si VS Code se abrió desde el Dock y no ve tu `PATH`, define `archeologyDoc.path` en la configuración de **usuario** |
| Las vistas están vacías / "No hay un proyecto de archeology-doc para esta carpeta" | Ejecuta **adoc: Inicializar proyecto**, o **adoc: Cambiar de proyecto** para elegir uno de `~/.archeology-doc/` |
| La extensión no se activa | La carpeta debe contener `archeology-doc.toml` y ser de confianza (**Workspaces: Manage Workspace Trust**) |
| Una tarea termina como *fallida* | Abre su log (**Ver log**); `adoc check` lista las notas y evidencias inválidas. Reintenta con **Reintentar fallidas** |
| Una tarea termina como *pregunta* | El agente necesita una decisión: respóndela en la pestaña Preguntas o en la vista Tareas |
| No se encuentra el agente o no responde | **adoc: Verificar requisitos (doctor)** y luego **adoc: Cambiar agente o modelo** |
| Notas marcadas como *desactualizadas* | El código citado cambió (`adoc sync`): vuelve a ejecutar las tareas reabiertas |
| El sitio no arranca | **adoc: Instalar herramientas** revisa Node y VitePress y ofrece `adoc site setup` |
| Mensajes en otro idioma | Ajusta `archeologyDoc.language`; los títulos de los comandos siguen el idioma de VS Code |

## Enlaces

- CLI y documentación completa: [github.com/jsoncr/archeology-doc](https://github.com/jsoncr/archeology-doc#readme)
- Sitio web y descargas: [archeology-doc.com](https://archeology-doc.com)
- Marketplace: [jcruz.archeology-doc](https://marketplace.visualstudio.com/items?itemName=jcruz.archeology-doc)
- Incidencias: [github.com/jsoncr/archeology-doc/issues](https://github.com/jsoncr/archeology-doc/issues)
- [Changelog](https://github.com/jsoncr/archeology-doc/blob/main/extension/CHANGELOG.md)

## Desarrollo

```bash
cd extension
npm install
npm run compile      # o npm run watch
npm run package      # genera archeology-doc-<versión>.vsix
code --install-extension archeology-doc-<versión>.vsix
```

Para depurar: abre `extension/` en VS Code y pulsa F5. El renderizador del grafo vive en
`crates/archeology-core/assets/`; `npm run compile` lo copia a `media/`.

**Añadir un texto**: la misma clave en `en` y `es` en `src/strings.ts` (TypeScript exige ambas) y úsala con
`t('clave', { parámetro })` (en `media/*.js`, las claves con prefijo `web.`, `kind.` u `origin.` se envían al
webview). Títulos de `package.json`: `%clave%` en `package.nls.json` (inglés) y `package.nls.es.json`.
`npm run compile` ejecuta `scripts/check-i18n.js`, que comprueba que ambos idiomas tengan las mismas claves y
marcadores.

Las capturas de `screens/` se generan con el propio código del webview de la extensión (`media/`) y datos de
`adoc status --format json` y `adoc graph` sobre un proyecto de ejemplo; no se incluyen en el `.vsix`.
