# Changelog

All notable changes to the Archeology Doc extension (`jcruz.archeology-doc`).

## [Unreleased]

### Changed

- **Update adoc** (from **adoc: Install tools**, when everything is installed) now runs `adoc upgrade --yes`
  in a terminal, which downloads, verifies and replaces the binary where it is installed; if it fails or
  the installed CLI predates `upgrade`, it falls back to the official installer.
- The knowledge graph is requested with `adoc graph --json`, retrying with `adoc graph --format json` for
  CLI versions that do not know `--json`.

### Documentation

- README (English and Spanish) rewritten: install, the `adoc` CLI (install, upgrade, key commands), quick
  start, features with screenshots, full commands and settings tables, troubleshooting and links. The
  broken relative link to the repository README now points to GitHub.
- Marketplace metadata: homepage, issue tracker, keywords, categories and gallery banner.
- Screenshots in `screens/` (excluded from the package).

## [0.1.1]

### Added

- English and Spanish UI. The `archeologyDoc.language` setting (`auto` | `en` | `es`) controls
  notifications, quick picks, views, status bar, dashboard and graph; `auto` follows the VS Code display
  language. Command, view and setting titles are localized through `package.nls.json` /
  `package.nls.es.json`.
- The extension passes `ADOC_LANG` to every `adoc` it runs, so CLI messages use the same language.
- **adoc: Initialize project** asks for the language of the generated documentation
  (`adoc init --lang en|es`).
- `npm run compile` checks that both languages have the same keys and placeholders
  (`scripts/check-i18n.js`).

### Changed

- The knowledge graph uses the renderer shared with the CLI (`adoc graph --open`, the HTML report and the
  site), copied from `crates/archeology-core/assets/` at build time.
- Published as `jcruz.archeology-doc` under the MIT license.

## [0.1.0]

- First version: Archeology side bar (Projects, Tasks, Findings, Questions, Knowledge, Specs), dashboard
  with Summary, Project, Tasks, Findings, Documentation, Questions and Graph tabs, status bar item,
  evidence marks in the editor, and commands for the whole `adoc` workflow.
