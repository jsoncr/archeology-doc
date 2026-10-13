# archeology-doc

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
  <img src="assets/logo.png" alt="archeology-doc" width="360">
</picture>

CLI escrito en Rust que inspecciona proyectos de software — nuevos o legacy, pequeños o grandes — y genera:

- una **base de conocimiento en Markdown** navegable con Obsidian (`knowledge/`),
- **documentación con VitePress** (`docs/`, manual de usuario, guías, referencia),
- **contexto y memoria para agentes de IA** (`AGENTS.md`, `knowledge/memory/`),
- **integración con OpenSpec**: cruza specs ↔ código y detecta desfases,
- un **registro global** de los proyectos en los que trabajas (`~/.archeology-doc/`).

La investigación la realizan los agentes que ya usas (Claude Code, Codex, OpenCode, Devin…); archeology-doc orquesta, estructura y valida.

## Instalación

Binario precompilado (sin Rust), desde el sitio publicado:

```bash
curl -fsSL https://<sitio>/download/install.sh | sh          # macOS / Linux
irm https://<sitio>/download/install.ps1 | iex                # Windows (PowerShell)
```

Desde el código (instaladores y build de descargas en [WEBSITE/](WEBSITE/README.md)):

```bash
WEBSITE/installers/install.sh  # compila este clon con cargo
cargo install --path crates/archeology-cli
# o desde el repo:
cargo build --release
# binarios en target/release/: `adoc` (corto) y `archeology-doc` (equivalente)

adoc doctor                    # verifica git, Node, VitePress, OpenSpec y agentes
adoc site setup                # opcional: VitePress compartido para todos los proyectos
```

Desinstalar:

```bash
adoc uninstall                 # quita los binarios `adoc` y `archeology-doc`
adoc uninstall --purge         # además borra ~/.archeology-doc (registro, estado, aprobaciones, VitePress)
```

Los proyectos no se tocan: `archeology-doc.toml`, `knowledge/`, `docs/` y las regiones de `AGENTS.md`
quedan en cada repositorio. Si se instaló con `cargo install`, usa `cargo uninstall archeology-cli`.

## Uso rápido

```bash
cd mi-proyecto-legacy

adoc init            # crea archeology-doc.toml + estructura
adoc scan            # inventario determinista → inventory.json + knowledge/00-overview.md
adoc plan            # genera tareas acotadas en ~/.archeology-doc/projects/<proyecto>/tasks/
adoc inspect         # ejecuta con el agente configurado (o exporta en modo manual)
adoc review          # resumen: hallazgos, tests, progreso
adoc check           # validaciones para CI (frontmatter, evidencias, enlaces)
```

Modo manual (agentes sin headless, p. ej. Devin/Cursor):

```bash
adoc inspect --agent manual     # exporta prompts
# …ejecuta cada tarea (`adoc task show T-001`) con tu agente…
adoc task done T-001            # marca completada tras revisar la salida
```

Trabajar desde otro directorio:

```bash
adoc projects list
adoc review --project mi-proyecto
adoc resume                     # retoma la inspección pendiente
```

## Dónde queda cada cosa

```
mi-proyecto/                          # solo configuración y resultados
├── archeology-doc.toml               # config + ID del proyecto (versionable)
├── knowledge/  docs/  llms.txt       # vault Obsidian, sitio VitePress
└── AGENTS.md  CLAUDE.md  .agents/    # contexto para agentes (regiones gestionadas)

~/.archeology-doc/                    # estado de trabajo, fuera del repositorio
├── registry.toml                     # índice de proyectos
├── vitepress/                        # VitePress compartido (`adoc site setup`)
└── projects/<nombre>-<id8>/          # por proyecto
    ├── state.json  inventory.json
    ├── tasks/                        # T-*.md
    └── runs/<fecha>-<id>/            # run.log y un log por tarea
```

Los proyectos con el estado antiguo en `<proyecto>/.archeology-doc/` se migran solos al primer comando,
y el registro de `~/Library/Application Support/archeology-doc` (o `~/.config/…`) también.

## Agentes

```bash
adoc agent list                       # disponibles; * = activo
adoc agent use claude --model opus    # cambia el agente y el modelo del proyecto
adoc agent use devin                  # Devin CLI
adoc agent use manual                 # sin agente: exporta tareas para hacerlas a mano
adoc agent use devin --reset          # vuelve a la definición predefinida del adaptador
adoc inspect --agent codex --model gpt-5   # solo para esta corrida
```

Agentes predefinidos (modo no interactivo con permiso de escritura):

| Agente | Comando |
|--------|---------|
| `claude` | `claude -p --permission-mode acceptEdits` (tarea por stdin) |
| `codex` | `codex exec -s workspace-write -` (stdin) |
| `devin` | `devin -p --permission-mode smart --respect-workspace-trust false --prompt-file <tarea>` |
| `gemini` | `gemini --approval-mode auto_edit` (stdin) |
| `qwen` | `qwen --approval-mode auto-edit` (stdin) |
| `opencode` | `opencode run <tarea>` |
| `cursor` | `agent -p --force --output-format text <tarea>` |
| `copilot` | `copilot -p <tarea> --allow-all-tools` |
| `amp` | `amp --dangerously-allow-all -x <tarea>` |
| `goose` | `goose run -i -` (stdin) |
| `aider` | `aider --message-file <tarea> --yes-always --no-auto-commits` |
| `manual` | exporta las tareas para hacerlas a mano |

Los flags de cada CLI cambian con el tiempo: `adoc agent test` y `adoc doctor` los comprueban, y cualquier
adaptador se puede redefinir en `[agent.adapters.<nombre>]`.

**Seguridad.** `archeology-doc.toml` viaja con el repositorio y puede venir de terceros, así que un
adaptador que no sea idéntico a uno predefinido no se ejecuta (ni siquiera `--version`) hasta que lo
revisas y lo apruebas:

```bash
adoc agent trust mi-agente     # muestra command, args y env, y pide confirmación
adoc agent untrust mi-agente   # retira la aprobación
```

La aprobación se guarda en `~/.archeology-doc/trusted-adapters.toml`, ligada al proyecto y a la
definición exacta: si el adaptador cambia en el repositorio, hay que volver a aprobarlo. Los
adaptadores `copilot`, `amp`, `aider` y `cursor` corren sin confirmaciones (pueden ejecutar comandos
de shell): `inspect` lo avisa. Úsalos solo con repositorios de confianza. `adoc preview` y
`adoc site` ejecutan la configuración de VitePress del proyecto (JavaScript).

`agent use` edita `archeology-doc.toml` conservando su formato. Con `manual`, `inspect` marca
las tareas como manuales; se ejecutan en esa herramienta y se cierran con `adoc task done <id>`.
Al cambiar a un agente con CLI, las tareas manuales sin completar vuelven a `pending` y basta `adoc inspect`;
las tareas `done` nunca se tocan.

## Comandos

| Comando | Descripción |
|---------|-------------|
| `init` | Inicializa config y estructura (`--yes`, `--agent`, `--lang`, `--name`, `--adapters claude,cursor,copilot`) |
| `scan` | Inventario determinista; actualiza `00-overview.md`, `AGENTS.md`, adaptadores, skills e índices (`--no-git`, `--json`) |
| `plan` | Genera/actualiza tareas priorizadas (`--module`, `--type`, `--path`, `--force`) |
| `inspect` | Ejecuta tareas con agente en orden de dependencias (`--agent`, `--model`, `--task`, `--module`, `--resume`, `--dry-run`) |
| `task list/show/export/done` | Gestión de tareas en modo manual (`done` valida la salida; `--force` la omite) |
| `task answer <id> "<texto>"` | Responde la pregunta de un agente y deja la tarea lista para reintentar |
| `task run <id>` | Ejecuta una sola tarea (`--interactive` abre la sesión del agente; `--agent`, `--model`) |
| `review` | Progreso, hallazgos, funcionalidades sin tests y cumplimiento de specs (`--severity`, `--format text\|json\|markdown`) |
| `check` | Validaciones para CI: frontmatter, evidencias (archivo y líneas), wikilinks, IDs, duplicados, notas stale (`--strict`, `--format`, `--site`) |
| `sync` | Marca `stale` las notas cuyas fuentes cambiaron y reabre sus tareas (`--since <ref>`, `--dry-run`) |
| `site setup` | Instala VitePress en el directorio de configuración, compartido por todos los proyectos (`--version`) |
| `site sync/dev/build` | Copia vault y specs a `docs/`, genera sidebar y `llms.txt`; `dev`/`build` delegan en VitePress |
| `status` | Estado completo del proyecto en JSON (usado por la extensión) |
| `preview` | Sitio con dashboard en el navegador; sin VitePress, el reporte HTML (`--port`, `--no-open`) |
| `report` | Reporte HTML único o Markdown (`--format`, `--out`, `--no-open`) |
| `graph` | Grafo del conocimiento en JSON: notas, wikilinks, relaciones y evidencias (vista de grafo de la extensión) |
| `ask "<pregunta>"` | Pregunta al agente; la respuesta queda en la FAQ (`--agent`, `--model`) |
| `doctor` | Verifica requisitos del CLI y del proyecto; código 1 si falta algo obligatorio (`--format`) |
| `agent list/test` | Lista y prueba adaptadores; `*` marca el activo |
| `agent use <agente>` | Cambia el agente activo del proyecto, p. ej. `claude` o `devin` (`--model`, `--no-model`, `--reset`) |
| `spec status` | Cumplimiento de requisitos de OpenSpec y cambios abandonados (`--capability`, `--format`) |
| `spec baseline` | Prepara un cambio `baseline-<fecha>` y la tarea para inferir specs base (`--capability`, `--dry-run`) |
| `spec validate <cambio>` | Valida un cambio con `openspec validate --strict` o validación estructural |
| `projects list/add/discover/prune` | Registro global con progreso, hallazgos y cumplimiento (`--format`) |
| `resume [proyecto]` | Retoma la última inspección pendiente |

Opción global: `--project <nombre|id>` en todos los comandos.

## Cómo funciona una tarea

1. `plan` escribe las tareas `T-*.md` en el espacio de trabajo con objetivo, archivos a leer, salidas (`# Produce`) y reglas.
2. `inspect` ejecuta cada tarea cuando sus dependencias están `done`. Una tarea solo queda `done` si el agente
   termina con código 0 **y** produjo lo declarado con frontmatter y evidencias válidas; si no, queda `failed`
   y sus dependientes, bloqueadas. Los reintentos se configuran en `agent.retries`.
3. Cada corrida deja `runs/<fecha>-<id>/run.log` y un log por tarea; en repos git avisa si el agente
   modificó archivos fuera de las rutas permitidas.
4. `sync` vuelve `stale` las notas cuyas fuentes cambiaron (salvo `locked: true`) y reabre sus tareas.

## Tamaño del proyecto

`plan` ajusta cuántas tareas genera según los archivos de **código** (no cuenta docs, datos, imágenes ni
videos; las carpetas ocultas de herramientas como `.devin` o `.vscode` no son módulos):

| Perfil | Archivos de código | Tareas por grupo de módulos | Ejemplo |
|--------|--------------------|-----------------------------|---------|
| `small` | ≤ 150 | 1 (módulos + funcionalidades + tests) | ~45 archivos → 6-7 tareas |
| `medium` | ≤ 1500 | 2 (módulos + funcionalidades, tests) | |
| `large` | > 1500 | 3 (módulos, funcionalidades, tests) | |

Los módulos vecinos se agrupan en una misma tarea hasta `max_files_per_task` / `max_bytes_per_task`.
El perfil es automático (`[plan] size = "auto"`) o fijo (`"small"`, `"medium"`, `"large"`). Si el plan
cambia, `adoc plan --force` lo reemplaza: borra las tareas que sobran y las que no cambian conservan su estado.

## Ver el resultado

```bash
adoc preview            # sitio VitePress con dashboard, abierto en el navegador
adoc report             # reporte HTML único (sin Node), se puede compartir
adoc report --out informe.html --no-open
adoc ask "¿Cómo se calcula el total de una reserva?"   # pregunta al agente → FAQ
```

- **`preview`** prepara el sitio (`site sync`) y lo abre con `vitepress dev --open`. La portada incluye un
  resumen (región gestionada `dashboard` en `docs/index.md`) y el sidebar añade **Estado**, **Hallazgos** y
  **Preguntas y FAQ**. Sin VitePress, muestra el reporte HTML.
- **`report`** genera un HTML autocontenido (sin CDN ni JS) en el espacio de trabajo, con progreso, hallazgos
  por severidad y evidencias, funcionalidades sin tests, specs, tareas y preguntas.
- **Preguntas**: las del agente que bloquean tareas (`task answer`), las dudas para el equipo técnico que los
  agentes registran en `knowledge/memory/open-questions.md`, y la **FAQ**: respuestas de `adoc ask`
  (notas `type: faq` en `knowledge/faq/`) y las dadas con `task answer`.

## Cuando el agente pregunta

Los agentes corren sin interfaz, así que nadie puede contestarles en el momento:

1. **Cada tarea les indica no preguntar**: deciden lo más razonable, lo marcan `confidence: low` y anotan la
   duda en `knowledge/memory/open-questions.md`.
2. **Si aun así terminan con una pregunta**, la tarea queda en estado `question` (no se reintenta en vano):
   ```
   ? T-014 pregunta: ¿El descuento máximo correcto es 10 % o 20 %?
     responde con: adoc task answer T-014 "<respuesta>"
   ```
   `task answer` guarda la respuesta en la sección `# Respuestas` de la tarea y la vuelve a `pending`; el
   siguiente `adoc inspect` la ejecuta con esa información. Las tareas que no dependen de ella siguen.
3. **Para tareas complejas**, `adoc task run T-014 --interactive` abre la sesión normal del agente con la
   tarea como primer mensaje; al salir, `adoc` valida la salida y la marca `done`. Disponible para claude,
   codex, devin, gemini, qwen y cursor (`interactive_args` en la config para otros).

## Sitio

VitePress (Node 18+) se busca en este orden:

1. **Proyecto**: `node_modules/vitepress` (`npm i -D vitepress`).
2. **Compartido**: `adoc site setup` lo instala una vez en el directorio de configuración.
3. **Global**: `npm i -g vitepress`.

Con 2 y 3, `site dev|build` enlaza `docs/node_modules` a esa instalación (ignorado en `docs/.gitignore`),
sin tocar el `package.json` del proyecto. En Windows el enlace requiere el modo desarrollador; si no,
usa la opción 1. `adoc doctor` muestra cuál se usará. Para verificar en CI: `adoc check --site`.

## Variables de entorno

| Variable | Uso |
|----------|-----|
| `ARCHEOLOGY_DOC_CONFIG_DIR` | Directorio de configuración (por defecto `~/.archeology-doc`). Útil en tests y CI. |
| `RUST_LOG` | Filtro de logs (`-v`/`-vv` equivalen a `info`/`debug`). |

## Extensión de VS Code

`extension/` contiene una interfaz visual sobre el CLI: panel con tareas (ejecutar, interactivo, responder
preguntas), hallazgos con salto a la evidencia, conocimiento por tipo y specs; barra de estado con progreso
y agente; marcas en el editor en las líneas citadas por hallazgos. Ver [extension/README.md](extension/README.md).

Al iniciar comprueba las herramientas: si falta `adoc` ofrece instalarlo (ejecuta el instalador oficial en
un terminal visible y verifica el SHA-256), y si faltan git, Node o VitePress ofrece instalarlos o enlaza su
descarga. Nada se instala sin un clic. Comando: *adoc: Instalar herramientas*; se desactiva con
`archeologyDoc.checkToolsOnStartup`.

```bash
cd extension && npm install && npm run package
code --install-extension archeology-doc-0.1.0.vsix
```

## Estructura del repo

```
crates/
  archeology-core/      # modelos, estado, plantillas, validación
  archeology-scan/      # scanner e inventario
  archeology-agents/    # orquestador y adaptadores
  archeology-openspec/  # spec bridge
  archeology-registry/  # registro global
  archeology-cli/       # binario
templates/              # plantillas de notas, tareas y sitio
fixtures/               # repos de prueba: small-node, medium-ts, legacy-py, with-openspec
```

## Desarrollo

```bash
cargo build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
# build real del sitio (instala VitePress en un directorio temporal; requiere Node y red):
cargo test -- --ignored
```

## Licencia

MIT OR Apache-2.0
