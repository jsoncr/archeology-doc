# Archeology Doc for VS Code

*English · [Español](https://github.com/jsoncr/archeology-doc/blob/main/extension/README.es.md)*

**Understand a legacy codebase without leaving the editor.** Archeology Doc is the visual front end of
[`adoc`](https://github.com/jsoncr/archeology-doc#readme), a CLI that inventories a repository, splits the
work into small tasks and has your AI agent (Claude Code, Codex, Gemini, Copilot, Cursor, Devin…) document
it: modules, features, test coverage, findings with `path:line` evidence and a knowledge graph. The
extension shows all of that in a dashboard and in the side bar, and runs every step for you.

![Archeology Doc dashboard: progress, open findings, pending questions and what needs attention](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-summary-dark.png)

It also works in VS Code-based editors (Cursor, Windsurf, Devin Desktop).

## Install

- **Marketplace**: [Archeology Doc](https://marketplace.visualstudio.com/items?itemName=jcruz.archeology-doc)
  (`jcruz.archeology-doc`), or search for *Archeology Doc* in the Extensions view.
- **Command line**:

  ```bash
  code --install-extension jcruz.archeology-doc
  ```

The extension activates in folders that contain `archeology-doc.toml`, and only in
[trusted workspaces](https://code.visualstudio.com/docs/editor/workspace-trust), because it runs the
`adoc` CLI on the open project.

## The `adoc` CLI

The extension has no analysis logic of its own: it reads `adoc status --format json` and runs `adoc`
commands. `adoc` is a single binary (macOS, Linux, Windows) that does the deterministic work (inventory,
task planning, validation of every note and every cited `path:line`, change detection) and delegates the
reading of the code to the agent you already use.

![adoc status in a terminal: project, generated structure, progress, pending tasks and next step](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/cli-status.png)

**Install it** (precompiled binary, SHA-256 verified):

```bash
curl -fsSL https://archeology-doc.com/download/install.sh | sh     # macOS / Linux
```

```powershell
irm https://archeology-doc.com/download/install.ps1 | iex          # Windows (PowerShell)
```

You don't have to do it by hand: if `adoc` is missing when the extension starts, it offers to install it in
an integrated terminal with the same official installer. Later, **adoc: Install tools (adoc, VitePress)**
checks everything and offers to update `adoc` (it runs `adoc upgrade --yes` and falls back to the
installer). From a terminal:

```bash
adoc upgrade --check     # only reports: current version → latest published
adoc upgrade             # downloads, verifies and replaces the binary
```

**Key commands** (all of them are also in the extension):

| Command | What it does |
| --- | --- |
| `adoc init` | Creates `archeology-doc.toml` and the `knowledge/` + `docs/` structure, and registers the project |
| `adoc scan` | Deterministic inventory: languages, modules, entry points, tests, TODO/FIXME markers, git history |
| `adoc plan` | Splits the work into bounded tasks (overview, one per module, contradictions, user manual…) |
| `adoc inspect` | Runs the pending tasks with the configured agent and validates what it writes (`--resume` retries) |
| `adoc status` | Project snapshot and suggested next step (`--format json` is what the extension reads) |
| `adoc check` | Checklist of what is done and what is missing; exit code for CI |
| `adoc sync` | Detects code changes, marks the affected notes as outdated and reopens their tasks |
| `adoc graph` | Knowledge graph as JSON, text or interactive HTML (`--open`) |
| `adoc review` | Summary of findings, test coverage and progress |
| `adoc site dev` / `adoc preview` | VitePress documentation site; `preview` shows the result in the browser |

Full documentation of the CLI: [github.com/jsoncr/archeology-doc](https://github.com/jsoncr/archeology-doc#readme)
· Website: [archeology-doc.com](https://archeology-doc.com)

## Quick start

1. Install the extension and the CLI (see above), then open the folder of the project you want to
   document.
2. Run **adoc: Initialize project** from the Command Palette and choose the language of the generated
   documentation (English or Spanish).
3. In the **Archeology** view of the activity bar, the Tasks view guides you: **1. Scan** → **2. Plan**.
4. Click **▶ Run tasks** (or **adoc: Run tasks (inspect)**). Each task runs in the `adoc` terminal with your
   agent; the views and the dashboard refresh as notes are written.
5. Open the dashboard (click `adoc` in the status bar) to review findings, answer the agent's questions and
   explore the graph. **adoc: View result (preview)** opens the documentation site.

To change the agent or model, use **adoc: Change agent or model** (or the *Agent* button in the
dashboard).

## Features

### Dashboard

Click the status bar item, the dashboard button in the Tasks or Projects view, or run
**adoc: Open dashboard**. The header shows the agent, profile and progress, with buttons to run tasks, retry
failed ones, switch agent, open the site and build the HTML report.

| Tab | Content |
| --- | --- |
| Summary | Progress, tasks to do, findings by severity, pending questions, features without tests, agent time and estimated remaining time; *Needs attention* (agent questions, failed tasks, high findings, outdated notes); project overview excerpt |
| Project | Inventory: files, size, lines, languages, modules, tests, markers, stack, manifests, entry points, CI and existing docs |
| Tasks | Status, type, modules, duration, attempts and last run of each task; run, interactive session, answer and log |
| Findings | Filters by severity and state; each `path:line` evidence opens the code at that line |
| Documentation | Notes by type, outdated notes and features without a test report; view site, HTML report, graph |
| Questions | Ask the agent; answer the agent's questions inline; questions for the technical team; FAQ |
| Graph | The knowledge graph (below) |

![Tasks tab: status, duration, attempts and actions for each task, including one waiting for an answer](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-tasks-dark.png)

![Findings tab: findings by severity with their kind, state and clickable evidence](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-findings-dark.png)

### When the agent asks

Agents run non-interactively. When one cannot decide something, the task ends in the *question* state
instead of guessing. Answer it in the Questions tab, in the Tasks view (**Answer question**) or live with
**Run in interactive mode**; the answer is stored with the task and the task is ready to run again.
**Ask the agent** sends your own question; the answer, with evidence, is saved to `knowledge/faq/`.

![Questions tab: ask the agent, answer a blocking question, questions for the team and FAQ](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-questions-dark.png)

### Knowledge graph

Dashboard tab, button in the Knowledge view or **adoc: Knowledge graph**. Notes colored by type (findings
by severity, outdated notes with an amber ring), edges from wikilinks, `related`, modules and tests;
source files cited as evidence can be shown from the legend. Search (Enter centers the first match),
show or hide types, zoom with the wheel or `+`/`−`, drag to pan; hovering highlights the neighbors and a
click opens the note, or the code at the cited line. It is the same renderer used by `adoc graph --open`,
the HTML report and the site.

![Knowledge graph in a dark theme: overview, module, features, findings, tests and cited source files](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/knowledge-graph-dark.png)

The dashboard follows the VS Code color theme:

![Dashboard summary in a light theme](https://raw.githubusercontent.com/jsoncr/archeology-doc/main/extension/screens/dashboard-summary-light.png)

### Archeology side bar

| View | Content | Actions |
| --- | --- | --- |
| Projects | Projects in the global registry `~/.archeology-doc/` (progress, agent, findings); the active one in green | click: use that project; open in a new window |
| Tasks | Grouped by status: waiting for your answer, failed, running, pending, manual, done | ▶ run, run interactively, answer, mark done, view log, copy command |
| Findings | By severity; each `path:line` evidence opens the code at that line | click: opens the note |
| Questions | From the agent (click: answer), for the technical team, and FAQ | ask the agent |
| Knowledge | Notes by type; outdated ones are marked | click: opens the note; graph |
| Specs | OpenSpec requirements with their compliance status (only when the project has specs) | click: opens the evidence |

The active project comes from the global registry, so it works if you open a subfolder of the project,
its parent folder or a monorepo with several projects; the choice is remembered per workspace.

### Status bar and editor

- **Status bar**: `adoc 50% · claude ? 1 ✗ 2`: progress, active agent (and model), questions and failed
  tasks. Click: opens the dashboard; **adoc: Actions…** lists every action.
- **Editor**: lines cited as evidence by an open finding get a gutter mark colored by severity; hovering
  shows the finding with a link to its note (`archeologyDoc.decorations`).

Long commands run in the integrated terminal named `adoc`, so you see their progress; the views refresh
when tasks, state or notes change.

## Commands

All commands are in the Command Palette under the `adoc:` category, except the ones marked *(context
menu)*, which act on a tree item.

| Command | ID | Runs |
| --- | --- | --- |
| Open dashboard | `adoc.dashboard` | — |
| Refresh | `adoc.refresh` | `adoc projects list`, `adoc status` |
| Actions… | `adoc.menu` | quick pick with every action |
| Initialize project | `adoc.init` | `adoc init --lang en\|es` |
| Scan (scan) | `adoc.scan` | `adoc scan` |
| Plan tasks (plan) | `adoc.plan` | `adoc plan` |
| Redo the plan (plan --force) | `adoc.planForce` | `adoc plan --force` |
| Run tasks (inspect) | `adoc.inspect` | `adoc inspect` |
| Retry failed (inspect --resume) | `adoc.resume` | `adoc inspect --resume` |
| Summary (review) | `adoc.review` | `adoc review` |
| Validate (check) | `adoc.check` | `adoc check` |
| Detect changes (sync) | `adoc.sync` | `adoc sync` |
| Check requirements (doctor) | `adoc.doctor` | `adoc doctor` |
| Site: development server | `adoc.siteDev` | `adoc site dev` |
| Site: build | `adoc.siteBuild` | `adoc site build` |
| View result (preview) | `adoc.preview` | `adoc preview` |
| HTML report | `adoc.report` | `adoc report` |
| Ask the agent | `adoc.ask` | `adoc ask` |
| Knowledge graph | `adoc.graph` | opens the Graph tab |
| Change agent or model | `adoc.selectAgent` | `adoc agent use` |
| Switch project | `adoc.selectProject` | — |
| Install tools (adoc, VitePress) | `adoc.installTools` | installer / `adoc upgrade` / `adoc site setup` |
| Run task *(context menu)* | `adoc.task.run` | `adoc task run <id>` |
| Run in interactive mode *(context menu)* | `adoc.task.runInteractive` | `adoc task run <id> --interactive` |
| Answer question *(context menu)* | `adoc.task.answer` | `adoc task answer <id>` |
| Mark as done (validates the output) *(context menu)* | `adoc.task.done` | `adoc task done <id>` |
| View log *(context menu)* | `adoc.task.openLog` | — |
| Copy command *(context menu)* | `adoc.task.copyCommand` | — |
| Open folder in new window *(context menu)* | `adoc.openProjectFolder` | — |
| Answer the agent's question *(Questions view)* | `adoc.answerQuestion` | `adoc task answer <id>` |

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `archeologyDoc.language` | `auto` | Language of the extension UI and of the `adoc` messages it runs: `auto`, `en`, `es` (see [Language](#language)) |
| `archeologyDoc.path` | `""` | Path to the `adoc` binary. Empty: the installer folder (`~/.local/bin`; on Windows `%LOCALAPPDATA%\Programs\archeology-doc\bin`), then `~/.cargo/bin`, then `PATH`. Read only from user settings, never from workspace settings |
| `archeologyDoc.decorations` | `true` | Mark the lines cited as evidence of findings in the editor |
| `archeologyDoc.checkToolsOnStartup` | `true` | On startup, check that `adoc`, git, Node and VitePress are installed and offer to install what is missing |
| `archeologyDoc.downloadUrl` | `""` | https URL of the `/download` folder `adoc` is installed from. Empty: `https://archeology-doc.com/download`. User settings only |

## Language

The extension is available in English and Spanish. `archeologyDoc.language` (`auto` | `en` | `es`,
default `auto`) selects the language; `auto` follows the VS Code display language (Spanish if it starts
with `es`, English otherwise). Changing it updates the views, status bar and dashboard immediately.

- **Follows the setting**: notifications, quick picks, side bar views, status bar, dashboard and graph,
  and the CLI messages: the extension passes `ADOC_LANG=<language>` to every `adoc` it runs (after a change,
  the next command opens a new terminal).
- **Follows the VS Code display language**: command, menu and view titles and setting descriptions (VS
  Code does not let an extension change them at runtime).
- **Documentation language**: chosen in **adoc: Initialize project** (`adoc init --lang en|es`),
  independently of the UI language.

## Requirements

- VS Code 1.85 or later (or a compatible editor), in a trusted workspace.
- The `adoc` CLI (the extension offers to install it).
- An AI coding agent with a headless mode for `adoc inspect` (Claude Code, Codex, Gemini CLI, Copilot CLI,
  Cursor, Devin, Aider, Amp, Goose, OpenCode, Qwen…), or the `manual` mode, which exports the prompts.
- git. For the documentation site: Node 18+ and VitePress (the startup check offers `adoc site setup`).

## Troubleshooting

| Symptom | What to do |
| --- | --- |
| "The `adoc` CLI was not found" | Click **Install**, or install it from a terminal (see [The `adoc` CLI](#the-adoc-cli)). If VS Code was opened from the Dock and does not see your `PATH`, set `archeologyDoc.path` in **user** settings |
| The views are empty / "There is no archeology-doc project for this folder" | Run **adoc: Initialize project**, or **adoc: Switch project** to pick one from `~/.archeology-doc/` |
| The extension does not activate | The folder must contain `archeology-doc.toml` and be trusted (**Workspaces: Manage Workspace Trust**) |
| A task ends as *failed* | Open its log (**View log**); `adoc check` lists invalid notes and evidence. Retry with **Retry failed** |
| A task ends as *question* | The agent needs a decision: answer it in the Questions tab or the Tasks view |
| The agent is not found or does not respond | **adoc: Check requirements (doctor)**, then **adoc: Change agent or model** |
| Notes marked *outdated* | The cited code changed (`adoc sync`): run the reopened tasks again |
| The site does not start | **adoc: Install tools** checks Node and VitePress and offers `adoc site setup` |
| Messages in the wrong language | Set `archeologyDoc.language`; command titles follow the VS Code display language |

## Links

- CLI and full documentation: [github.com/jsoncr/archeology-doc](https://github.com/jsoncr/archeology-doc#readme)
- Website and downloads: [archeology-doc.com](https://archeology-doc.com)
- Marketplace: [jcruz.archeology-doc](https://marketplace.visualstudio.com/items?itemName=jcruz.archeology-doc)
- Issues: [github.com/jsoncr/archeology-doc/issues](https://github.com/jsoncr/archeology-doc/issues)
- [Changelog](https://github.com/jsoncr/archeology-doc/blob/main/extension/CHANGELOG.md)

## Development

```bash
cd extension
npm install
npm run compile      # or npm run watch
npm run package      # builds archeology-doc-<version>.vsix
code --install-extension archeology-doc-<version>.vsix
```

To debug: open `extension/` in VS Code and press F5. The graph renderer lives in
`crates/archeology-core/assets/`; `npm run compile` copies it to `media/`.

**Adding a string**: add the same key to `en` and `es` in `src/strings.ts` (TypeScript requires both) and
use it with `t('key', { param })` (in `media/*.js`, keys prefixed with `web.`, `kind.` or `origin.` are sent
to the webview). `package.json` titles: `%key%` in `package.nls.json` (English) and `package.nls.es.json`.
`npm run compile` runs `scripts/check-i18n.js`, which checks that both languages have the same keys and
placeholders.

The screenshots in `screens/` are rendered from the extension's own webview code (`media/`) with data from
`adoc status --format json` and `adoc graph` on a sample project; they are not packaged in the `.vsix`.
