# Archeology Doc for VS Code

*English · [Español](README.es.md)*

A visual interface over the [`adoc`](../README.md) CLI: browse what was generated and run tasks without
remembering commands. It also works in VS Code-based editors (Cursor, Windsurf, Devin Desktop).

## Requirements

- The `adoc` CLI. If it is missing, the extension offers to install it on startup (official installer from
  `https://archeology-doc.com/download`, with SHA-256 verification; change the URL with
  `archeologyDoc.downloadUrl`). To install or update it later, run *adoc: Install tools*
  (`adoc.installTools`) or run the installer again. You can also set the path in `archeologyDoc.path`;
  from a clone of the repository, `cargo install --path crates/archeology-cli --force`.
- For the documentation site: git, Node 18+ and VitePress. The startup check offers `adoc site setup` or
  links to the download of whatever is missing (command *adoc: Install tools*; disable it with
  `archeologyDoc.checkToolsOnStartup`).
- A folder with `archeology-doc.toml` (or `adoc init` from the extension itself).

## Features

**Active project**: taken from the global registry `~/.archeology-doc/` (what `adoc projects list`
shows). It works even if you open a subfolder of the project, the parent folder or a monorepo with several
projects; the **Projects** view lists them all (progress, agent, findings) and a click switches the active
one. The choice is remembered per workspace.

**"Archeology" panel** (activity bar):

| View | Content | Actions |
|------|---------|---------|
| Projects | Registered in `~/.archeology-doc/`; the active one in green | click: use that project; open in a new window |
| Tasks | Grouped by status: question, failed, pending, manual, done | ▶ run, run interactively, answer question, mark done, view log, copy command |
| Questions | From the agent (click: answer), for the technical team, and FAQ | button: ask the agent |
| Findings | By severity; each `path:line` evidence opens the code at that line | click: opens the note |
| Knowledge | Notes by type; outdated ones are marked | click: opens the note |
| Specs | OpenSpec requirements with their compliance status | click: opens the evidence |

**Dashboard** (click the status bar, button in Tasks/Projects or `adoc: Open dashboard`), with tabs:

| Tab | Content |
|-----|---------|
| Summary | Progress, tasks to do, findings by severity, pending questions, features without tests, agent time and estimated remaining time; "Needs attention" (questions, failed tasks, high findings, outdated notes); agent and specs; overview excerpt |
| Tasks | Status, modules, duration, attempts and last run of each task; run, interactive session, answer and log |
| Findings | Filters by severity and state; evidence that opens the code at the line |
| Documentation | Notes by type, outdated ones and features without tests; view site and report |
| Questions | Ask the agent; answer the agent's questions in the panel; questions for the team; FAQ |
| Graph | The knowledge graph |

**Knowledge graph** (dashboard tab, button in the Knowledge view or `adoc: Knowledge graph`): like
Obsidian's graph. Notes colored by type (findings by severity, outdated ones with an amber border), edges
from wikilinks, `related`, modules and tests; source files cited as evidence can be shown with their
filter. Search (Enter centers the first match), show or hide types from the legend, zoom with the wheel or
`+`/`−`, drag to move; hovering highlights neighbors and a click opens the note (or the code at the cited
line). The renderer is shared with the CLI (`adoc graph --open`, the HTML report and the site): its single
copy lives in `crates/archeology-core/assets/` and `npm run compile` copies it to `media/`.

**Status bar**: `adoc 43% · devin ? 1 ✗ 2` — progress, active agent, questions and failed tasks.
Click: opens the dashboard (the action menu is in `adoc: Actions…`).

**Editor**: lines cited by an open finding get a mark in the gutter; hovering shows the finding with a
link to its note (`archeologyDoc.decorations` to disable it).

**Command palette** (`adoc: …`): view result (preview), HTML report, ask the agent, scan, plan, run,
retry, validate, sync, site, doctor and change agent or model.

Long commands run in the integrated terminal (`adoc`), with visible progress; the panel refreshes by
itself when tasks or the vault change.

## Language

The extension is available in English and Spanish. The `archeologyDoc.language` setting (`auto` | `en` |
`es`, default `auto`) selects the language; `auto` uses the VS Code display language (Spanish if it starts
with `es`, English otherwise). Changing it updates the views, status bar and dashboard immediately.

- **Follows the setting**: notifications, quick picks, panel views, status bar, dashboard and graph, and
  the CLI messages: the extension passes `ADOC_LANG=<language>` to every `adoc` it runs (in the background
  and in the terminal; after a language change the next command opens a new terminal).
- **Follows the VS Code display language**: command, menu and view titles, and setting descriptions
  (they come from `package.nls*.json`; VS Code does not let an extension change them at runtime).
- **Documentation language**: `adoc: Initialize project` asks which language the documentation is
  generated in (`adoc init --lang en|es`), independently of the UI language.

## Development

```bash
cd extension
npm install
npm run compile      # or npm run watch
npm run package      # builds archeology-doc-<version>.vsix
code --install-extension archeology-doc-0.1.0.vsix
```

To debug: open `extension/` in VS Code and press F5.

**Adding a string**: add the same key to `en` and `es` in `src/strings.ts` (TypeScript requires both) and
use it with `t('key', { param })` (in `media/*.js`, keys prefixed with `web.`, `kind.` or `origin.` are
sent to the webview). `package.json` titles: `%key%` in `package.nls.json` (English) and
`package.nls.es.json`. `npm run compile` runs `scripts/check-i18n.js`, which checks that both languages
have the same keys and placeholders.
