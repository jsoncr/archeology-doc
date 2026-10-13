// Dashboard del proyecto: resumen, tareas (con tiempos), hallazgos,
// documentación, preguntas y, en otra pestaña, el grafo del conocimiento.
//
// El webview solo dibuja (media/dashboard.js); los datos salen de
// `adoc status` y `adoc graph`, y las acciones vuelven aquí como mensajes.

import * as crypto from 'crypto';
import * as path from 'path';
import * as vscode from 'vscode';
import * as cli from './cli';
import { Key, language, t, webviewStrings } from './i18n';
import { Status } from './types';

export type DashboardTab = 'summary' | 'project' | 'tasks' | 'findings' | 'docs' | 'questions' | 'graph';

/** Lo que el dashboard necesita del controlador. */
export interface DashboardHost {
  root: string | undefined;
  status: Status | undefined;
  run(args: string[]): void;
  scheduleRefresh(delay?: number): void;
}

type Message =
  | { type: 'ready' }
  | { type: 'refresh' }
  | { type: 'dashboard-ready' }
  | { type: 'open'; file: string; line?: number }
  | { type: 'run'; args: string[] }
  | { type: 'command'; id: string }
  | { type: 'answer'; task: string; text: string }
  | { type: 'ask'; question: string };

/** Comandos de VS Code que el webview puede pedir. */
const ALLOWED_COMMANDS = new Set(['adoc.selectAgent']);
/** Subcomandos de `adoc` que el webview puede lanzar. */
const ALLOWED_RUN = new Set(['scan', 'plan', 'inspect', 'check', 'preview', 'report']);
const TASK_ID = /^[A-Za-z0-9_][A-Za-z0-9_-]*$/;

/**
 * El webview no se trata como confiable: muestra texto escrito por agentes.
 * Si alguna vez se colara HTML, solo podría pedir las acciones de los botones.
 */
function allowedRun(args: unknown): args is string[] {
  if (!Array.isArray(args) || !args.every((a) => typeof a === 'string')) {
    return false;
  }
  const [cmd, ...rest] = args as string[];
  if (cmd === 'task') {
    // task run <id> [--interactive]
    return (
      rest[0] === 'run' &&
      TASK_ID.test(rest[1] ?? '') &&
      (rest.length === 2 || (rest.length === 3 && rest[2] === '--interactive'))
    );
  }
  if (cmd === 'plan') {
    return rest.length === 0 || (rest.length === 1 && rest[0] === '--force');
  }
  if (cmd === 'inspect') {
    // inspect [--resume] (botón "Reintentar")
    return rest.length === 0 || (rest.length === 1 && rest[0] === '--resume');
  }
  return ALLOWED_RUN.has(cmd) && rest.length === 0;
}

/** `true` si `file` está dentro de alguna de las carpetas `roots`. */
function isInside(file: string, roots: (string | undefined)[]): boolean {
  const target = path.resolve(file);
  return roots.some((r) => {
    if (!r) {
      return false;
    }
    const rel = path.relative(path.resolve(r), target);
    return rel !== '' && !rel.startsWith('..') && !path.isAbsolute(rel);
  });
}

export class DashboardPanel {
  private static current: DashboardPanel | undefined;
  private readonly panel: vscode.WebviewPanel;
  private disposed = false;

  static show(extensionUri: vscode.Uri, host: DashboardHost, tab: DashboardTab = 'summary'): void {
    if (!DashboardPanel.current) {
      DashboardPanel.current = new DashboardPanel(extensionUri, host);
    }
    const d = DashboardPanel.current;
    d.panel.reveal(vscode.ViewColumn.Active);
    void d.panel.webview.postMessage({ type: 'tab', tab });
    d.pushStatus();
  }

  /** El estado cambió: actualizar el panel si está abierto. */
  static update(): void {
    const d = DashboardPanel.current;
    if (d) {
      d.pushStatus();
      void d.pushGraph();
    }
  }

  /** Cambió el idioma: se vuelve a generar el HTML (el webview pide el estado al cargar). */
  static relocalize(): void {
    const d = DashboardPanel.current;
    if (d && !d.disposed) {
      d.panel.webview.html = d.html();
    }
  }

  private constructor(private readonly extensionUri: vscode.Uri, private readonly host: DashboardHost) {
    this.panel = vscode.window.createWebviewPanel(
      'adoc.dashboard',
      'archeology-doc',
      vscode.ViewColumn.Active,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
        localResourceRoots: [vscode.Uri.joinPath(extensionUri, 'media')],
      },
    );
    this.panel.iconPath = vscode.Uri.joinPath(extensionUri, 'media', 'icon.png');
    this.panel.webview.html = this.html();
    this.panel.onDidDispose(() => {
      this.disposed = true;
      DashboardPanel.current = undefined;
    });
    this.panel.webview.onDidReceiveMessage((msg: Message) => void this.handle(msg));
  }

  private async handle(msg: Message): Promise<void> {
    switch (msg.type) {
      case 'dashboard-ready':
        this.pushStatus();
        break;
      case 'ready':
      case 'refresh':
        // `ready`/`refresh` los envía el grafo.
        await this.pushGraph();
        if (msg.type === 'refresh') {
          this.host.scheduleRefresh(0);
        }
        break;
      case 'open': {
        const project = this.host.status?.project;
        if (typeof msg.file !== 'string' || !isInside(msg.file, [project?.root, project?.workspace])) {
          break;
        }
        const options: vscode.TextDocumentShowOptions = { viewColumn: vscode.ViewColumn.Beside, preview: true };
        if (msg.line) {
          const pos = new vscode.Position(Math.max(0, msg.line - 1), 0);
          options.selection = new vscode.Range(pos, pos);
        }
        try {
          await vscode.window.showTextDocument(vscode.Uri.file(msg.file), options);
        } catch {
          void vscode.window.showWarningMessage(t('dash.openFailed', { file: msg.file }));
        }
        break;
      }
      case 'run':
        if (allowedRun(msg.args)) {
          this.host.run(msg.args);
        }
        break;
      case 'command':
        if (ALLOWED_COMMANDS.has(msg.id)) {
          await vscode.commands.executeCommand(msg.id);
        }
        break;
      case 'answer':
        if (this.host.root && TASK_ID.test(String(msg.task)) && typeof msg.text === 'string' && msg.text.trim()) {
          try {
            // `--`: un texto que empieza con `-` no se toma como opción.
            await cli.exec(this.host.root, ['task', 'answer', msg.task, '--', msg.text.trim()]);
            void vscode.window.showInformationMessage(t('dash.answerSaved', { task: msg.task }));
          } catch (err) {
            void vscode.window.showErrorMessage((err as Error).message);
          }
          this.host.scheduleRefresh(0);
        }
        break;
      case 'ask':
        if (typeof msg.question === 'string' && msg.question.trim()) {
          this.host.run(['ask', '--', msg.question.trim()]);
        }
        break;
    }
  }

  private pushStatus(): void {
    if (!this.disposed) {
      void this.panel.webview.postMessage({ type: 'status', status: this.host.status ?? null });
    }
  }

  private async pushGraph(): Promise<void> {
    if (this.disposed || !this.host.root) {
      return;
    }
    try {
      const graph = JSON.parse(await cli.exec(this.host.root, ['graph', '--format', 'json']));
      await this.panel.webview.postMessage({ type: 'graph', graph });
    } catch {
      // Sin vault todavía: la pestaña del grafo queda vacía.
    }
  }

  private html(): string {
    const webview = this.panel.webview;
    const media = (f: string) => webview.asWebviewUri(vscode.Uri.joinPath(this.extensionUri, 'media', f));
    const nonce = crypto.randomBytes(16).toString('base64');
    // Textos del webview: van en un atributo `data-` (no en un script inline),
    // así la CSP sigue exigiendo el nonce para cualquier script.
    const strings = escapeHtml(JSON.stringify({ lang: language(), strings: webviewStrings() }));
    const tx = (key: Key) => escapeHtml(t(key));
    return `<!doctype html>
<html lang="${language()}">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${webview.cspSource}; style-src ${webview.cspSource} 'unsafe-inline'; script-src 'nonce-${nonce}';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="stylesheet" href="${media('graph.css')}">
<link rel="stylesheet" href="${media('dashboard.css')}">
</head>
<body data-i18n="${strings}">
  <header class="top">
    <div class="title">
      <h1 id="name">archeology-doc</h1>
      <div class="meta" id="meta"></div>
    </div>
    <div class="actions">
      <button class="primary" data-run="inspect" title="adoc inspect">▶ ${tx('dash.runTasks')}</button>
      <button data-run="inspect --resume" title="adoc inspect --resume">↻ ${tx('dash.retry')}</button>
      <button data-command="adoc.selectAgent" title="adoc agent use">${tx('dash.agent')}</button>
      <button data-run="preview" title="adoc preview">${tx('dash.viewSite')}</button>
      <button data-run="report" title="adoc report">${tx('dash.report')}</button>
      <button id="reload" title="${tx('dash.refresh')}">⟳</button>
    </div>
  </header>
  <div id="outdated" class="outdated" hidden></div>
  <nav class="tabs" id="tabs">
    <button data-tab="summary" class="active">${tx('dash.tab.summary')}</button>
    <button data-tab="project">${tx('dash.tab.project')}</button>
    <button data-tab="tasks">${tx('dash.tab.tasks')}</button>
    <button data-tab="findings">${tx('dash.tab.findings')}</button>
    <button data-tab="docs">${tx('dash.tab.docs')}</button>
    <button data-tab="questions">${tx('dash.tab.questions')}</button>
    <button data-tab="graph">${tx('dash.tab.graph')}</button>
  </nav>
  <main>
    <section data-panel="summary" class="panel active" id="summary"></section>
    <section data-panel="project" class="panel" id="project"></section>
    <section data-panel="tasks" class="panel" id="tasks"></section>
    <section data-panel="findings" class="panel" id="findings"></section>
    <section data-panel="docs" class="panel" id="docs"></section>
    <section data-panel="questions" class="panel" id="questions"></section>
    <section data-panel="graph" class="panel graph-panel">
      <!-- Lo rellena el renderizador compartido (media/graph.js, copiado de crates/archeology-core/assets/). -->
      <div id="graph"></div>
    </section>
  </main>
  <script nonce="${nonce}" src="${media('i18n.js')}"></script>
  <script nonce="${nonce}" src="${media('graph.js')}"></script>
  <script nonce="${nonce}" src="${media('dashboard.js')}"></script>
</body>
</html>`;
  }
}

/** Escapa texto para HTML (contenido y atributos). */
function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string);
}
