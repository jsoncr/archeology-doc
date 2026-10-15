// Idiomas del sitio (inglés y español). Se carga en <head>, antes del primer pintado:
// resuelve el idioma, ajusta <html lang> y, si no es el del HTML (inglés), oculta
// el cuerpo hasta traducirlo al cargar el DOM, para no mostrar el idioma equivocado.
//
// Marcado en index.html:
//   data-i18n="clave"                     → textContent
//   data-i18n-html="clave"                → innerHTML (solo cadenas fijas de este archivo)
//   data-i18n-attr="attr:clave;attr2:clave2" → atributos (title, aria-label, alt, content…)
//
// Orden para elegir el idioma: ?lang=en|es > localStorage (adoc-lang) > navigator.languages > en.
// WEBSITE/scripts/check-i18n.mjs comprueba que ambos diccionarios tengan las mismas claves.

(function () {
  'use strict';

  const SUPPORTED = ['en', 'es'];
  const DEFAULT_LANG = 'en';
  const STORAGE_KEY = 'adoc-lang';

  const MESSAGES = {
    en: {
      'meta.title': 'archeology-doc · document any project with your AI agents',
      'meta.description': 'Rust CLI that inspects new or legacy projects and generates a knowledge base, VitePress documentation and context for AI agents, backed by verifiable evidence.',
      'meta.ogLocale': 'en_US',
      'lang.group': 'Language',
      'theme.toggle': 'Toggle theme',

      'nav.how': 'How it works',
      'nav.install': 'Install',
      'nav.downloads': 'Downloads',
      'nav.commands': 'Commands',
      'nav.agents': 'Agents',

      'hero.eyebrow': 'Open-source CLI · Rust · local-first',
      'hero.title1': 'Dig into any project.',
      'hero.title2': 'Document it with evidence.',
      'hero.lead': '<strong>archeology-doc</strong> (<code>adoc</code>) inspects new or legacy projects and, with the AI agent you already use, generates a browsable knowledge base, documentation, findings and context for agents. Every claim cites its source as <code>path:line</code>.',
      'cta.install': 'Install',
      'cta.how': 'See how it works',

      'card.map.title': 'System map',
      'card.map.text': 'Modules, features, glossary and decisions in plain Markdown, browsable in Obsidian, VitePress or GitHub.',
      'card.risks.title': 'Contradictions and risks',
      'card.risks.text': 'Findings by severity: outdated documentation, duplicated rules, conflicting configuration, dead code.',
      'card.tests.title': 'Testing gaps',
      'card.tests.text': 'Features ↔ tests, covered and missing scenarios, disabled or empty tests.',
      'card.agents.title': 'Context for agents',
      'card.agents.text': '<code>AGENTS.md</code>, persistent memory and skills: every AI session starts knowing what exists and where.',
      'card.specs.title': 'Specs with OpenSpec',
      'card.specs.text': 'Requirement compliance checked against the code, with abandoned changes flagged in review, status and check.',
      'card.verify.title': 'Verifiable',
      'card.verify.text': '<code>adoc check</code> validates evidence, links and outdated notes; ideal for CI.',

      'how.title': 'How it works',
      'how.sub': 'Rust computes the deterministic parts; your agent does the interpretation; <code>adoc</code> orchestrates and validates.',
      'how.scan': 'AI-free inventory: languages, modules, tests, CI, git history and OpenSpec. Honors <code>.gitignore</code> and excludes secrets.',
      'how.plan': 'Splits the work into bounded tasks based on project size: a few tasks for a small one, more for a large one.',
      'how.inspect': 'Your agent runs each task. It is only marked done if it produced what was asked, with valid evidence.',
      'how.preview': 'A site with a dashboard: status, findings, questions for the team and FAQ. Or a single HTML report.',
      'how.sync': 'When the code changes, it marks the affected notes as outdated and reopens only those tasks.',

      'install.title': 'Install',
      'install.sub': 'One command for your system: it downloads the binary for your platform, verifies its SHA-256 and puts it on your PATH. No Rust needed. Node 18+ only for the VitePress site and the extension.',
      'install.osLabel': 'Operating system',
      'install.tabSource': 'From source',
      'install.orDownload': 'or download the binary',
      'install.macNote': 'When downloaded from the browser, macOS quarantines it: <code>xattr -d com.apple.quarantine adoc archeology-doc</code>, then move both to <code>~/.local/bin</code>. Not needed with the terminal command.',
      'install.linuxNote': 'Binaries for glibc 2.28 or later (Debian 10, Ubuntu 18.10, RHEL 8 and newer).',
      'install.windowsNote': 'The installer puts it in <code>%LOCALAPPDATA%\\Programs\\archeology-doc\\bin</code> and adds it to your PATH, without administrator rights. On Windows ARM64 it runs under x64 emulation.',
      'install.extras': 'Extras',
      'install.binariesNote': 'Two equivalent binaries are installed: <code>adoc</code> (short) and <code>archeology-doc</code>. To uninstall: <code>adoc uninstall</code> (with <code>--purge</code> it also deletes <code>~/.archeology-doc</code>; your project files are never touched).',

      'downloads.title': 'All downloads',
      'downloads.file': 'File',
      'downloads.platform': 'Platform',
      'downloads.size': 'Size',
      'downloads.placeholder': 'List available once a release is published.',
      'downloads.verifyNote': 'Verify a download with <code>shasum -a 256 &lt;file&gt;</code> (macOS/Linux) or <code>Get-FileHash &lt;file&gt;</code> (PowerShell) and compare it with <a href="download/SHA256SUMS">SHA256SUMS</a>.',

      'start.title': 'Getting started',
      'start.sub': 'In your project folder:',
      'daily.title': 'Day to day',
      'daily.review': 'Progress, findings, features without tests',
      'daily.resume': 'Retries failed or interrupted tasks',
      'daily.answer': 'Answers an agent that asked a question',
      'daily.run': 'Interactive session with the agent',
      'daily.askExample': 'adoc ask "…?"',
      'daily.ask': 'Ask about the project → FAQ',
      'daily.sync': 'Shows changed and uncovered files, marks outdated notes and reopens their tasks',
      'daily.check': 'Checklist of what is done and missing, with hints to fix it (exit code for CI)',
      'daily.report': 'Single HTML report to share',
      'daily.status': 'Summary: progress, pending tasks and the next step (<code>--format json</code> for integrations)',
      'daily.graph': 'Interactive knowledge graph in the browser; <code>--json</code> prints it as JSON',
      'setup.title': 'Projects, agents and settings',
      'setup.projectsList': 'All your projects and their progress',
      'setup.projectsDeleteExample': 'adoc projects delete <name>',
      'setup.projectsDelete': 'Removes a project from the registry; with <code>-d</code> it also deletes its generated files',
      'setup.agentUse': 'Project agent and model',
      'setup.language': 'Interface language: <code>en</code> or <code>es</code>',
      'maint.title': 'Maintenance',
      'maint.doctor': 'Checks git, Node, VitePress, OpenSpec and agents',
      'maint.upgradeCheck': 'Only checks whether a new version is available',
      'maint.upgrade': 'Downloads and installs the latest version',
      'maint.uninstall': 'Removes <code>adoc</code>; <code>--purge</code> also deletes <code>~/.archeology-doc</code>',

      'agents.title': 'Work with your agent',
      'agents.sub': 'Agent-agnostic: switch agent or model whenever you like. No CLI? Export the tasks and do them by hand.',

      'vscode.title': 'VS Code extension',
      'vscode.sub': 'Everything generated, in view while you code. Also works in Cursor, Windsurf and Devin Desktop.',
      'vscode.tasks': 'Tasks by status with a ▶ button to run them',
      'vscode.findings': 'Findings that jump to the evidence line',
      'vscode.graph': 'Knowledge graph, like in Obsidian',
      'vscode.questions': 'Questions from the agent and the team, plus FAQ',
      'vscode.marks': 'Editor marks on the cited code',
      'vscode.status': 'Progress and active agent in the status bar',
      'vscode.marketplace': 'Install from the Marketplace',
      'vscode.orTerminal': 'or from the terminal',
      'install.vscode': 'VS Code extension',
      'install.vscodeNote': "The <code>.vsix</code> is for offline installs and for editors that don't use the Microsoft Marketplace (Cursor, Windsurf): <code>code --install-extension archeology-doc.vsix</code>.",
      'mock.tasks': '▾ Tasks',
      'mock.question': '❓ T-004 · question',
      'mock.findings': '▾ Findings',
      'mock.finding': '⚠ Discount cap 10 % vs 20 %',

      'output.title': 'What stays in your project',
      'output.sub': 'Only results, in Markdown. Working state lives outside the repository.',
      'principles.title': 'Principles',
      'principles.evidence': '<b>Evidence first:</b> every claim cites <code>path:line</code>',
      'principles.safe': '<b>Non-destructive:</b> never modifies your code; respects your edits',
      'principles.local': '<b>Local-first:</b> nothing leaves your machine except what your agent sends',
      'principles.incremental': '<b>Incremental:</b> small, resumable tasks, module by module',
      'principles.standards': '<b>Standards:</b> Markdown, OpenSpec, AGENTS.md, Obsidian and VitePress',
      'footer.made': 'Made with Rust 🦀',

      // Repositorio, código abierto y actualización.
      "privacy.title": "Sensitive data stays out",
      "privacy.sub": "archeology-doc never reads secret files and tells your agent not to either. This is what is left out by default:",
      "privacy.secrets.title": "Secret files skipped",
      "privacy.secrets.text": "<code>.env*</code>, <code>*.pem</code>, <code>*.key</code>, <code>credentials*</code>, SSH keys (<code>id_rsa</code>, <code>id_ed25519</code>…), keystores (<code>*.p12</code>, <code>*.pfx</code>, <code>*.jks</code>), <code>.npmrc</code>, <code>.netrc</code>, <code>.git-credentials</code>, <code>*.tfvars</code>, <code>*.tfstate</code>, <code>secrets.*</code>. They never reach the inventory or the prompts.",
      "privacy.locked.title": "Can't be switched off",
      "privacy.locked.text": "A repository's configuration can add patterns (<code>[scan] secrets_patterns</code>) but never remove the built-in ones.",
      "privacy.agent.title": "Rules for the agent",
      "privacy.agent.text": "Every task tells the agent not to read or cite those files, and never to copy keys, tokens, passwords or connection strings into the notes: only that they exist and where.",
      "privacy.ignore.title": "Respects .gitignore",
      "privacy.ignore.text": "Plus <code>vendor/</code>, <code>node_modules/</code>, <code>target/</code>, <code>dist/</code> and minified files. Add your own with <code>[paths] exclude</code>. Symbolic links are not followed.",
      "privacy.outside.title": "Working state outside the repo",
      "privacy.outside.text": "Tasks, logs and approvals live in <code>~/.archeology-doc/</code>, not in your repository, and adoc never writes through a link that points outside the project.",
      "privacy.network.title": "Your code isn't uploaded",
      "privacy.network.text": "adoc only talks to the network to check for updates (<code>adoc upgrade</code>, <code>adoc doctor</code>; turn it off with <code>ADOC_NO_UPDATE_CHECK=1</code>). The agent you choose talks to its own provider. Third-party adapters need your approval (<code>adoc agent trust</code>).",
      "code.privacyConfig": "# archeology-doc.toml: added to the built-in rules",
      "privacy.note": "Detection works by file name: a secret hard-coded in regular source code can still reach the agent. The agent is told not to copy it, but rotate it anyway and move it to an excluded file.",
      'nav.openSource': 'Open source',
      'repo.github': 'GitHub repository',
      'install.upgradeNote': 'Stay up to date: <code>adoc upgrade</code> checks for a new version and updates itself automatically; <code>adoc upgrade --check</code> only checks, without changing anything.',
      'oss.title': 'Open source',
      'oss.sub': 'The whole project lives on GitHub under the <a href="https://github.com/jsoncr/archeology-doc/blob/main/LICENSE-MIT" rel="noopener">MIT license</a>: use it, modify it and share it freely.',
      'oss.cli.title': 'Rust CLI',
      'oss.cli.text': '<code>crates/</code>: the <code>adoc</code> CLI plus the core, scan, agents, OpenSpec and registry crates.',
      'oss.ext.title': 'VS Code extension',
      'oss.ext.text': '<code>extension/</code>: tasks, findings, knowledge graph and questions inside the editor.',
      'oss.installers.title': 'Installers',
      'oss.installers.text': '<code>install.sh</code> and <code>install.ps1</code>: one-command install for macOS, Linux and Windows, with SHA-256 verification.',
      'oss.templates.title': 'Templates',
      'oss.templates.text': '<code>templates/</code>: the Markdown notes and prompts the agents fill in, in English and Spanish.',
      'oss.source': 'Source code',
      'oss.releases': 'Releases',
      'oss.issues': 'Report a bug or request a feature',
      'oss.contribute.title': 'How to contribute',
      'oss.contribute.fork': '<b>Fork</b> the repository and create a branch for your change.',
      'oss.contribute.test': 'Run <code>cargo test --workspace</code> (and <code>npm run compile</code> in <code>extension/</code> if you touch it).',
      'oss.contribute.pr': 'Open a <b>pull request</b> describing what changes and why. For larger ideas, open an issue first.',
      'oss.build.title': 'Build from source',
      'footer.nav': 'Project',
      'footer.releases': 'Releases',
      'footer.issues': 'Issues',
      'footer.license': 'MIT License',
      'code.upgradeCheck': '# Only checks whether a new version is available',
      'code.upgrade': '# Downloads and installs the new version automatically',
      'code.ossCli': '# CLI (requires Rust)',
      'code.ossExt': '# VS Code extension (requires Node 18+) → .vsix',

      // Comentarios de los bloques de código (los comandos no se traducen).
      'code.sourceNeedsRust': '# Requires Rust (rustup.rs). Installs `adoc` into ~/.cargo/bin',
      'code.orCargo': '# or directly with cargo',
      'code.extrasWith': '# With shared VitePress and the VS Code extension',
      'code.doctor': '# Checks requirements: git, Node, VitePress, OpenSpec and agents',
      'code.projectName': 'my-project',
      'code.init': '# config + structure',
      'code.agentUse': '# or devin, codex, gemini…',
      'code.scan': '# deterministic inventory',
      'code.plan': '# tasks sized to the project',
      'code.inspect': '# the agent documents',
      'code.preview': '# see the result',
      'code.status': "# where you are and what's next",
      'code.agentList': '# available; * = active',
      'code.agentModel': '# project agent and model',
      'code.agentOnce': '# just for this run',
      'code.treeConfig': '# config + ID',
      'code.treeAgents': '# context for agents',
      'code.treeVault': '# Obsidian vault',
      'code.treeDocs': '# VitePress site',

      // Textos que genera main.js.
      'js.copy': 'Copy',
      'js.copied': 'Copied ✓',
      'js.copyFallback': 'Select and copy',
      'js.detected': 'Detected: {os}',
      'js.heroOs': 'Installing for {os} · other options under “Install”',
      'js.placeholderBase': 'https://your-site/download/',
      'js.unavailable': 'not available',
      'js.platformVsix': 'VS Code extension',
      'js.platformInstallSh': 'macOS / Linux installer',
      'js.platformInstallPs1': 'Windows installer',
    },

    es: {
      'meta.title': 'archeology-doc · documenta cualquier proyecto con tus agentes de IA',
      'meta.description': 'CLI en Rust que inspecciona proyectos nuevos o legacy y genera base de conocimiento, documentación VitePress y contexto para agentes de IA, con evidencias verificables.',
      'meta.ogLocale': 'es_ES',
      'lang.group': 'Idioma',
      'theme.toggle': 'Cambiar tema',

      'nav.how': 'Cómo funciona',
      'nav.install': 'Instalar',
      'nav.downloads': 'Descargas',
      'nav.commands': 'Comandos',
      'nav.agents': 'Agentes',

      'hero.eyebrow': 'CLI open source · Rust · local-first',
      'hero.title1': 'Excava cualquier proyecto.',
      'hero.title2': 'Documéntalo con evidencia.',
      'hero.lead': '<strong>archeology-doc</strong> (<code>adoc</code>) inspecciona proyectos nuevos o legacy y, con el agente de IA que ya usas, genera una base de conocimiento navegable, documentación, hallazgos y contexto para agentes. Cada afirmación cita su fuente <code>ruta:línea</code>.',
      'cta.install': 'Instalar',
      'cta.how': 'Ver cómo funciona',

      'card.map.title': 'Mapa del sistema',
      'card.map.text': 'Módulos, funcionalidades, glosario y decisiones en Markdown plano, navegable en Obsidian, VitePress o GitHub.',
      'card.risks.title': 'Contradicciones y riesgos',
      'card.risks.text': 'Hallazgos por severidad: documentación desactualizada, reglas duplicadas, configuración en conflicto, código muerto.',
      'card.tests.title': 'Huecos de testing',
      'card.tests.text': 'Funcionalidades ↔ tests, escenarios cubiertos y faltantes, tests desactivados o vacíos.',
      'card.agents.title': 'Contexto para agentes',
      'card.agents.text': '<code>AGENTS.md</code>, memoria persistente y skills: cada sesión de IA empieza sabiendo qué hay y dónde.',
      'card.specs.title': 'Specs con OpenSpec',
      'card.specs.text': 'Cumplimiento de requisitos contra el código y cambios abandonados señalados en review, status y check.',
      'card.verify.title': 'Verificable',
      'card.verify.text': '<code>adoc check</code> valida evidencias, enlaces y notas desactualizadas; ideal para CI.',

      'how.title': 'Cómo funciona',
      'how.sub': 'Lo determinista lo calcula Rust; la interpretación la hace tu agente; <code>adoc</code> orquesta y valida.',
      'how.scan': 'Inventario sin IA: lenguajes, módulos, tests, CI, historial git y OpenSpec. Respeta <code>.gitignore</code> y excluye secretos.',
      'how.plan': 'Divide el trabajo en tareas acotadas según el tamaño del proyecto: pocas tareas en uno pequeño, más en uno grande.',
      'how.inspect': 'Tu agente ejecuta cada tarea. Solo se marca hecha si produjo lo pedido con evidencias válidas.',
      'how.preview': 'Sitio con dashboard: estado, hallazgos, preguntas para el equipo y FAQ. O un reporte HTML único.',
      'how.sync': 'Cuando el código cambia, marca las notas afectadas como desactualizadas y reabre solo esas tareas.',

      'install.title': 'Instalar',
      'install.sub': 'Un comando según tu sistema: descarga el binario de tu plataforma, verifica su SHA-256 y lo deja en el PATH. No necesitas Rust. Node 18+ solo para el sitio VitePress y la extensión.',
      'install.osLabel': 'Sistema operativo',
      'install.tabSource': 'Desde el código',
      'install.orDownload': 'o descarga el binario',
      'install.macNote': 'Descargado desde el navegador, macOS lo marca en cuarentena: <code>xattr -d com.apple.quarantine adoc archeology-doc</code> y muévelos a <code>~/.local/bin</code>. Con el comando de terminal no hace falta.',
      'install.linuxNote': 'Binarios para glibc 2.28 o superior (Debian 10, Ubuntu 18.10, RHEL 8 y posteriores).',
      'install.windowsNote': 'El instalador lo deja en <code>%LOCALAPPDATA%\\Programs\\archeology-doc\\bin</code> y lo añade a tu PATH, sin permisos de administrador. En Windows ARM64 funciona con la emulación x64.',
      'install.extras': 'Extras',
      'install.binariesNote': 'Se instalan dos binarios equivalentes: <code>adoc</code> (corto) y <code>archeology-doc</code>. Para desinstalar: <code>adoc uninstall</code> (con <code>--purge</code> borra también <code>~/.archeology-doc</code>; los archivos de tus proyectos no se tocan).',

      'downloads.title': 'Todas las descargas',
      'downloads.file': 'Archivo',
      'downloads.platform': 'Plataforma',
      'downloads.size': 'Tamaño',
      'downloads.placeholder': 'Lista disponible cuando se publique una versión.',
      'downloads.verifyNote': 'Verifica una descarga con <code>shasum -a 256 &lt;archivo&gt;</code> (macOS/Linux) o <code>Get-FileHash &lt;archivo&gt;</code> (PowerShell) y compárala con <a href="download/SHA256SUMS">SHA256SUMS</a>.',

      'start.title': 'Primeros pasos',
      'start.sub': 'En la carpeta de tu proyecto:',
      'daily.title': 'Día a día',
      'daily.review': 'Progreso, hallazgos, funcionalidades sin tests',
      'daily.resume': 'Reintenta tareas fallidas o interrumpidas',
      'daily.answer': 'Responde a un agente que preguntó',
      'daily.run': 'Sesión interactiva con el agente',
      'daily.askExample': 'adoc ask "¿…?"',
      'daily.ask': 'Pregunta sobre el proyecto → FAQ',
      'daily.sync': 'Muestra archivos cambiados y sin cubrir, marca notas desactualizadas y reabre sus tareas',
      'daily.check': 'Lista de lo hecho y lo que falta, con pistas para arreglarlo (código de salida para CI)',
      'daily.report': 'Reporte HTML único para compartir',
      'daily.status': 'Resumen: progreso, tareas pendientes y el siguiente paso (<code>--format json</code> para integraciones)',
      'daily.graph': 'Grafo de conocimiento interactivo en el navegador; <code>--json</code> lo imprime como JSON',
      'setup.title': 'Proyectos, agentes y ajustes',
      'setup.projectsList': 'Todos tus proyectos y su progreso',
      'setup.projectsDeleteExample': 'adoc projects delete <nombre>',
      'setup.projectsDelete': 'Quita un proyecto del registro; con <code>-d</code> borra también sus archivos generados',
      'setup.agentUse': 'Agente y modelo del proyecto',
      'setup.language': 'Idioma de la interfaz: <code>en</code> o <code>es</code>',
      'maint.title': 'Mantenimiento',
      'maint.doctor': 'Verifica git, Node, VitePress, OpenSpec y agentes',
      'maint.upgradeCheck': 'Solo comprueba si hay una versión nueva',
      'maint.upgrade': 'Descarga e instala la última versión',
      'maint.uninstall': 'Quita <code>adoc</code>; <code>--purge</code> borra también <code>~/.archeology-doc</code>',

      'agents.title': 'Trabaja con tu agente',
      'agents.sub': 'Agnóstico: cambia de agente o de modelo cuando quieras. Sin CLI, exporta las tareas para hacerlas a mano.',

      'vscode.title': 'Extensión para VS Code',
      'vscode.sub': 'Todo lo generado, a la vista mientras programas. Funciona también en Cursor, Windsurf y Devin Desktop.',
      'vscode.tasks': 'Tareas por estado con botón ▶ para ejecutarlas',
      'vscode.findings': 'Hallazgos con salto a la línea de evidencia',
      'vscode.graph': 'Grafo del conocimiento, como en Obsidian',
      'vscode.questions': 'Preguntas del agente y del equipo, y FAQ',
      'vscode.marks': 'Marcas en el editor en el código citado',
      'vscode.status': 'Progreso y agente activo en la barra de estado',
      'vscode.marketplace': 'Instalar desde el Marketplace',
      'vscode.orTerminal': 'o desde la terminal',
      'install.vscode': 'Extensión de VS Code',
      'install.vscodeNote': 'El <code>.vsix</code> sirve para instalar sin conexión y en editores que no usan el Marketplace de Microsoft (Cursor, Windsurf): <code>code --install-extension archeology-doc.vsix</code>.',
      'mock.tasks': '▾ Tareas',
      'mock.question': '❓ T-004 · pregunta',
      'mock.findings': '▾ Hallazgos',
      'mock.finding': '⚠ Tope de descuento 10 % vs 20 %',

      'output.title': 'Qué queda en tu proyecto',
      'output.sub': 'Solo resultados, en Markdown. El estado de trabajo vive fuera del repositorio.',
      'principles.title': 'Principios',
      'principles.evidence': '<b>Evidencia ante todo:</b> cada afirmación cita <code>ruta:línea</code>',
      'principles.safe': '<b>No destructivo:</b> nunca modifica tu código; respeta tus ediciones',
      'principles.local': '<b>Local-first:</b> nada sale de tu máquina salvo lo que envíe tu agente',
      'principles.incremental': '<b>Incremental:</b> tareas pequeñas, reanudables, por módulos',
      'principles.standards': '<b>Estándares:</b> Markdown, OpenSpec, AGENTS.md, Obsidian y VitePress',
      'footer.made': 'Hecho en Rust 🦀',

      // Repositorio, código abierto y actualización.
      "privacy.title": "Los datos sensibles quedan fuera",
      "privacy.sub": "archeology-doc nunca lee archivos de secretos y le indica a tu agente que tampoco lo haga. Esto se omite por defecto:",
      "privacy.secrets.title": "Se omiten los archivos de secretos",
      "privacy.secrets.text": "<code>.env*</code>, <code>*.pem</code>, <code>*.key</code>, <code>credentials*</code>, llaves SSH (<code>id_rsa</code>, <code>id_ed25519</code>…), almacenes de claves (<code>*.p12</code>, <code>*.pfx</code>, <code>*.jks</code>), <code>.npmrc</code>, <code>.netrc</code>, <code>.git-credentials</code>, <code>*.tfvars</code>, <code>*.tfstate</code>, <code>secrets.*</code>. Nunca llegan al inventario ni a los prompts.",
      "privacy.locked.title": "No se puede desactivar",
      "privacy.locked.text": "La configuración de un repositorio puede añadir patrones (<code>[scan] secrets_patterns</code>), pero nunca quitar los predefinidos.",
      "privacy.agent.title": "Reglas para el agente",
      "privacy.agent.text": "Cada tarea le indica al agente que no lea ni cite esos archivos, y que nunca copie claves, tokens, contraseñas ni cadenas de conexión en las notas: solo que existen y dónde.",
      "privacy.ignore.title": "Respeta .gitignore",
      "privacy.ignore.text": "Además de <code>vendor/</code>, <code>node_modules/</code>, <code>target/</code>, <code>dist/</code> y los archivos minificados. Añade los tuyos con <code>[paths] exclude</code>. No se siguen los enlaces simbólicos.",
      "privacy.outside.title": "El estado de trabajo, fuera del repo",
      "privacy.outside.text": "Tareas, logs y aprobaciones viven en <code>~/.archeology-doc/</code>, no en tu repositorio, y adoc nunca escribe a través de un enlace que apunte fuera del proyecto.",
      "privacy.network.title": "Tu código no se sube",
      "privacy.network.text": "adoc solo usa la red para buscar actualizaciones (<code>adoc upgrade</code>, <code>adoc doctor</code>; se desactiva con <code>ADOC_NO_UPDATE_CHECK=1</code>). El agente que elijas habla con su propio proveedor. Los adaptadores de terceros necesitan tu aprobación (<code>adoc agent trust</code>).",
      "code.privacyConfig": "# archeology-doc.toml: se suma a las reglas predefinidas",
      "privacy.note": "La detección es por nombre de archivo: un secreto escrito directamente en el código fuente sí puede llegar al agente. Se le indica que no lo copie, pero conviene rotarlo y moverlo a un archivo excluido.",
      'nav.openSource': 'Código abierto',
      'repo.github': 'Repositorio en GitHub',
      'install.upgradeNote': 'Mantente al día: <code>adoc upgrade</code> busca una versión nueva y se actualiza automáticamente; <code>adoc upgrade --check</code> solo comprueba, sin cambiar nada.',
      'oss.title': 'Código abierto',
      'oss.sub': 'Todo el proyecto está en GitHub bajo <a href="https://github.com/jsoncr/archeology-doc/blob/main/LICENSE-MIT" rel="noopener">licencia MIT</a>: úsalo, modifícalo y compártelo libremente.',
      'oss.cli.title': 'CLI en Rust',
      'oss.cli.text': '<code>crates/</code>: la CLI <code>adoc</code> y los crates core, scan, agents, OpenSpec y registry.',
      'oss.ext.title': 'Extensión de VS Code',
      'oss.ext.text': '<code>extension/</code>: tareas, hallazgos, grafo de conocimiento y preguntas dentro del editor.',
      'oss.installers.title': 'Instaladores',
      'oss.installers.text': '<code>install.sh</code> e <code>install.ps1</code>: instalación con un comando en macOS, Linux y Windows, con verificación SHA-256.',
      'oss.templates.title': 'Plantillas',
      'oss.templates.text': '<code>templates/</code>: las notas Markdown y los prompts que completan los agentes, en inglés y español.',
      'oss.source': 'Código fuente',
      'oss.releases': 'Versiones',
      'oss.issues': 'Reportar un error o pedir una mejora',
      'oss.contribute.title': 'Cómo contribuir',
      'oss.contribute.fork': 'Haz un <b>fork</b> del repositorio y crea una rama para tu cambio.',
      'oss.contribute.test': 'Ejecuta <code>cargo test --workspace</code> (y <code>npm run compile</code> en <code>extension/</code> si la tocas).',
      'oss.contribute.pr': 'Abre un <b>pull request</b> que explique qué cambia y por qué. Para ideas grandes, abre antes un issue.',
      'oss.build.title': 'Compilar desde el código',
      'footer.nav': 'Proyecto',
      'footer.releases': 'Versiones',
      'footer.issues': 'Issues',
      'footer.license': 'Licencia MIT',
      'code.upgradeCheck': '# Solo comprueba si hay una versión nueva',
      'code.upgrade': '# Descarga e instala la versión nueva automáticamente',
      'code.ossCli': '# CLI (necesita Rust)',
      'code.ossExt': '# Extensión de VS Code (necesita Node 18+) → .vsix',

      'code.sourceNeedsRust': '# Necesita Rust (rustup.rs). Instala `adoc` en ~/.cargo/bin',
      'code.orCargo': '# o directamente con cargo',
      'code.extrasWith': '# Con VitePress compartido y la extensión de VS Code',
      'code.doctor': '# Verifica requisitos: git, Node, VitePress, OpenSpec y agentes',
      'code.projectName': 'mi-proyecto',
      'code.init': '# config + estructura',
      'code.agentUse': '# o devin, codex, gemini…',
      'code.scan': '# inventario determinista',
      'code.plan': '# tareas según el tamaño',
      'code.inspect': '# el agente documenta',
      'code.preview': '# ver el resultado',
      'code.status': '# dónde estás y qué sigue',
      'code.agentList': '# disponibles; * = activo',
      'code.agentModel': '# agente y modelo del proyecto',
      'code.agentOnce': '# solo para esta corrida',
      'code.treeConfig': '# config + ID',
      'code.treeAgents': '# contexto para agentes',
      'code.treeVault': '# vault Obsidian',
      'code.treeDocs': '# sitio VitePress',

      'js.copy': 'Copiar',
      'js.copied': 'Copiado ✓',
      'js.copyFallback': 'Selecciona y copia',
      'js.detected': 'Detectado: {os}',
      'js.heroOs': 'Instalación para {os} · otras opciones en «Instalar»',
      'js.placeholderBase': 'https://tu-sitio/download/',
      'js.unavailable': 'no disponible',
      'js.platformVsix': 'Extensión de VS Code',
      'js.platformInstallSh': 'Instalador macOS / Linux',
      'js.platformInstallPs1': 'Instalador Windows',
    },
  };

  // Normaliza una etiqueta de idioma ("es-CR", "EN_us") a un idioma soportado o null.
  const normalize = (tag) => {
    const base = String(tag || '').toLowerCase().split(/[-_]/)[0];
    return SUPPORTED.includes(base) ? base : null;
  };

  const readStored = () => {
    try {
      return normalize(localStorage.getItem(STORAGE_KEY));
    } catch {
      return null; // almacenamiento no disponible
    }
  };

  const readQuery = () => {
    try {
      return normalize(new URLSearchParams(location.search).get('lang'));
    } catch {
      return null;
    }
  };

  // Primer idioma del navegador que sea en* o es*.
  const readNavigator = () => {
    const list = (navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language]) || [];
    for (const tag of list) {
      const lang = normalize(tag);
      if (lang) return lang;
    }
    return null;
  };

  const resolveInitial = () => readQuery() || readStored() || readNavigator() || DEFAULT_LANG;

  let current = DEFAULT_LANG;

  // Traduce una clave; {nombre} se reemplaza con vars. Si falta, cae al inglés y luego a la clave.
  const t = (key, vars) => {
    const dict = MESSAGES[current] || MESSAGES[DEFAULT_LANG];
    let text = key in dict ? dict[key] : key in MESSAGES[DEFAULT_LANG] ? MESSAGES[DEFAULT_LANG][key] : key;
    if (vars) text = text.replace(/\{(\w+)\}/g, (m, name) => (name in vars ? String(vars[name]) : m));
    return text;
  };

  // Aplica el idioma a todo el documento y avisa a main.js (evento adoc:langchange).
  const apply = (lang) => {
    current = normalize(lang) || DEFAULT_LANG;
    const root = document.documentElement;
    root.lang = current;
    document.querySelectorAll('[data-i18n]').forEach((el) => {
      el.textContent = t(el.dataset.i18n);
    });
    document.querySelectorAll('[data-i18n-html]').forEach((el) => {
      // Solo cadenas fijas de MESSAGES: nunca datos del usuario ni de la URL.
      el.innerHTML = t(el.dataset.i18nHtml);
    });
    document.querySelectorAll('[data-i18n-attr]').forEach((el) => {
      el.dataset.i18nAttr.split(';').forEach((pair) => {
        const idx = pair.indexOf(':');
        if (idx < 1) return;
        el.setAttribute(pair.slice(0, idx).trim(), t(pair.slice(idx + 1).trim()));
      });
    });
    document.querySelectorAll('[data-lang]').forEach((btn) => {
      btn.setAttribute('aria-pressed', String(btn.dataset.lang === current));
    });
    document.dispatchEvent(new CustomEvent('adoc:langchange', { detail: { lang: current } }));
  };

  // Cambio por el usuario: se recuerda y, si la URL trae ?lang, se actualiza para que no lo pise al recargar.
  const setLang = (lang) => {
    const next = normalize(lang);
    if (!next) return;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* sin persistencia */
    }
    try {
      const url = new URL(location.href);
      if (url.searchParams.has('lang')) {
        url.searchParams.set('lang', next);
        history.replaceState(null, '', url);
      }
    } catch {
      /* URL no editable (p. ej. file://): se ignora */
    }
    apply(next);
  };

  // API mínima para main.js y para el script de comprobación.
  const api = {
    SUPPORTED,
    DEFAULT_LANG,
    STORAGE_KEY,
    MESSAGES,
    t,
    setLang,
    get lang() {
      return current;
    },
  };
  globalThis.AdocI18n = api;

  // Fuera del navegador (comprobación con node) no hay DOM: solo se exponen los diccionarios.
  if (typeof document === 'undefined') return;

  const initial = resolveInitial();
  current = initial;
  const root = document.documentElement;
  root.lang = initial;
  // El HTML ya está en inglés; en otro idioma se oculta hasta traducir (ver styles.css).
  if (initial !== DEFAULT_LANG) root.setAttribute('data-i18n-pending', '');

  const init = () => {
    try {
      apply(initial);
    } finally {
      root.removeAttribute('data-i18n-pending');
    }
    document.querySelectorAll('[data-lang]').forEach((btn) => {
      btn.addEventListener('click', () => setLang(btn.dataset.lang));
    });
  };
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
})();
