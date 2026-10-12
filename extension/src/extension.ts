// Extensión de arqueology-doc: interfaz sobre el CLI `adoc` (RF-140 a RF-144).
//
// No duplica lógica: lee `adoc status --format json` y ejecuta comandos en el
// terminal integrado. El CLI sigue siendo la única fuente de verdad.

import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import * as cli from './cli';
import { FindingDecorations } from './decorations';
import { DashboardPanel } from './dashboard';
import { FindingsTree, KnowledgeTree, ProjectNode, ProjectsTree, QuestionsTree, SpecsTree, TaskNode, TasksTree } from './trees';
import { matchProject } from './match';
import { ProjectRow, Status } from './types';

const CONFIG_FILE = 'arqueology-doc.toml';
/** Clave en workspaceState del proyecto elegido para este workspace. */
const ACTIVE_KEY = 'adoc.activeProject';

/** Carpetas del workspace con la ruta real (el registro guarda rutas canónicas). */
function workspaceFolders(): string[] {
  return (vscode.workspace.workspaceFolders ?? []).map((f) => {
    try {
      return fs.realpathSync(f.uri.fsPath);
    } catch {
      return f.uri.fsPath;
    }
  });
}

class Controller implements vscode.Disposable {
  root: string | undefined;
  status: Status | undefined;
  private readonly tasks = new TasksTree();
  private readonly findings = new FindingsTree();
  private readonly knowledge = new KnowledgeTree();
  private readonly specs = new SpecsTree();
  private readonly questions = new QuestionsTree();
  private readonly projectsTree = new ProjectsTree();
  projects: ProjectRow[] = [];
  private rootWatcher: vscode.FileSystemWatcher | undefined;
  private readonly decorations = new FindingDecorations();
  private readonly statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50);
  private readonly disposables: vscode.Disposable[] = [];
  private watchedWorkspace: string | undefined;
  private workspaceWatcher: vscode.FileSystemWatcher | undefined;
  private timer: NodeJS.Timeout | undefined;
  private warnedMissingCli = false;

  constructor(private readonly context: vscode.ExtensionContext) {
    // El registro global cambia con `init`, `projects add/prune` o cualquier
    // comando de un proyecto (última actividad).
    const registry = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(vscode.Uri.file(cli.configDir()), 'registry.toml'),
    );
    registry.onDidChange(() => this.scheduleRefresh());
    registry.onDidCreate(() => this.scheduleRefresh());
    this.disposables.push(
      registry,
      vscode.window.registerTreeDataProvider('adoc.projects', this.projectsTree),
      vscode.window.registerTreeDataProvider('adoc.tasks', this.tasks),
      vscode.window.registerTreeDataProvider('adoc.findings', this.findings),
      vscode.window.registerTreeDataProvider('adoc.knowledge', this.knowledge),
      vscode.window.registerTreeDataProvider('adoc.specs', this.specs),
      vscode.window.registerTreeDataProvider('adoc.questions', this.questions),
      this.decorations,
      this.statusBar,
      vscode.window.onDidChangeWindowState((s) => s.focused && this.scheduleRefresh()),
    );
    this.statusBar.command = 'adoc.dashboard';
  }

  /**
   * Determina el proyecto activo a partir del registro global
   * (`~/.arqueology-doc/registry.toml`):
   * 1. el elegido antes en este workspace, si sigue registrado;
   * 2. un proyecto cuya ruta es una carpeta abierta;
   * 3. uno que contiene la carpeta abierta (estás en una subcarpeta);
   * 4. uno dentro de la carpeta abierta (carpeta padre o monorepo), el más reciente;
   * 5. sin registro: la carpeta con arqueology-doc.toml más cercana.
   */
  async locate(): Promise<void> {
    const folders = workspaceFolders();
    try {
      this.projects = await cli.projects();
    } catch {
      this.projects = [];
    }
    const saved = this.context.workspaceState.get<string>(ACTIVE_KEY);
    const pick = matchProject(this.projects, folders, saved);

    let root: string | undefined = pick?.path;
    if (!root) {
      for (const f of folders) {
        if (fs.existsSync(path.join(f, CONFIG_FILE))) {
          root = f;
          break;
        }
      }
    }
    if (!root) {
      const found = await vscode.workspace.findFiles(`**/${CONFIG_FILE}`, '**/node_modules/**', 1);
      root = found[0] ? path.dirname(found[0].fsPath) : undefined;
    }
    this.setRoot(root);
  }

  /** Cambia el proyecto activo y vigila sus archivos. */
  setRoot(root: string | undefined): void {
    if (root === this.root && this.rootWatcher) {
      return;
    }
    this.root = root;
    this.rootWatcher?.dispose();
    this.rootWatcher = undefined;
    void vscode.commands.executeCommand(
      'setContext',
      'adoc.hasProject',
      !!root && fs.existsSync(path.join(root, CONFIG_FILE)),
    );
    if (root) {
      this.rootWatcher = vscode.workspace.createFileSystemWatcher(
        new vscode.RelativePattern(vscode.Uri.file(root), `{${CONFIG_FILE},knowledge/**,AGENTS.md}`),
      );
      this.rootWatcher.onDidChange(() => this.scheduleRefresh());
      this.rootWatcher.onDidCreate(() => this.scheduleRefresh());
      this.rootWatcher.onDidDelete(() => this.scheduleRefresh());
    }
  }

  /** Usa un proyecto del registro y lo recuerda para este workspace. */
  async select(project: ProjectRow): Promise<void> {
    await this.context.workspaceState.update(ACTIVE_KEY, project.id);
    this.setRoot(project.path);
    await this.refresh();
  }

  /** Agrupa ráfagas de cambios (un agente escribe muchas notas seguidas). */
  scheduleRefresh(delay = 600): void {
    if (this.timer) {
      clearTimeout(this.timer);
    }
    this.timer = setTimeout(() => {
      this.refresh().catch((err) => console.error('arqueology-doc: refresh', err));
    }, delay);
  }

  async refresh(): Promise<void> {
    try {
      this.projects = await cli.projects();
    } catch {
      // Sin CLI: se informa abajo al pedir el estado.
    }
    // Sin proyecto todavía (p. ej. recién inicializado): reintentar la detección.
    if (!this.root || !fs.existsSync(path.join(this.root, CONFIG_FILE))) {
      await this.locate();
    }
    const root = this.root;
    if (!root || !fs.existsSync(path.join(root, CONFIG_FILE))) {
      await vscode.commands.executeCommand('setContext', 'adoc.hasProject', false);
      this.apply(undefined);
      return;
    }
    await vscode.commands.executeCommand('setContext', 'adoc.hasProject', true);
    try {
      this.apply(await cli.status(root));
    } catch (err) {
      const message = (err as Error).message;
      if (/ENOENT|not found|no such file/i.test(message) && !this.warnedMissingCli) {
        this.warnedMissingCli = true;
        // Sin `await`: la activación no debe esperar a que se cierre el aviso.
        void vscode.window
          .showWarningMessage(
            'No se encontró el CLI `adoc`. Instálalo con `cargo install --path crates/arqueology-cli` o configura arqueologyDoc.path.',
            'Configurar ruta',
          )
          .then((choice) => {
            if (choice) {
              void vscode.commands.executeCommand('workbench.action.openSettings', 'arqueologyDoc.path');
            }
          });
      }
      this.statusBar.text = '$(warning) adoc';
      this.statusBar.tooltip = message;
      this.statusBar.show();
    }
  }

  private apply(status: Status | undefined): void {
    this.status = status;
    DashboardPanel.update();
    this.projectsTree.update(this.projects, status?.project.id, workspaceFolders());
    this.tasks.update(status);
    this.findings.update(status);
    this.knowledge.update(status);
    this.specs.update(status);
    this.questions.update(status);
    this.decorations.update(status);
    void vscode.commands.executeCommand('setContext', 'adoc.hasTasks', !!status?.tasks.length);
    void vscode.commands.executeCommand('setContext', 'adoc.hasSpecs', !!status?.specs);
    if (!status) {
      this.statusBar.hide();
      return;
    }
    // Las tareas viven fuera del repo (~/.arqueology-doc/projects/…): se vigilan aparte.
    if (status.project.workspace !== this.watchedWorkspace) {
      this.workspaceWatcher?.dispose();
      this.watchedWorkspace = status.project.workspace;
      this.workspaceWatcher = vscode.workspace.createFileSystemWatcher(
        new vscode.RelativePattern(vscode.Uri.file(status.project.workspace), '{tasks/**,state.json}'),
      );
      this.workspaceWatcher.onDidChange(() => this.scheduleRefresh());
      this.workspaceWatcher.onDidCreate(() => this.scheduleRefresh());
      this.workspaceWatcher.onDidDelete(() => this.scheduleRefresh());
    }
    const questions = status.tasks.filter((t) => t.status === 'question').length;
    const failed = status.tasks.filter((t) => t.status === 'failed').length;
    const model = status.agent.model ? ` (${status.agent.model})` : '';
    this.statusBar.text =
      `$(book) adoc ${status.progress}% · ${status.agent.active}${model}` +
      (questions ? ` $(question) ${questions}` : '') +
      (failed ? ` $(error) ${failed}` : '');
    // Con varios proyectos posibles, el nombre del activo evita confusiones.
    if (this.projects.length > 1) {
      this.statusBar.text = this.statusBar.text.replace('adoc', `adoc ${status.project.name}`);
    }
    this.statusBar.tooltip = `${status.project.name} · perfil ${status.profile} · ${status.tasks.length} tareas, ${status.findings.length} hallazgos\nClic: abrir el dashboard`;
    this.statusBar.show();
  }

  run(args: string[]): void {
    if (!this.root) {
      void vscode.window.showWarningMessage('Abre una carpeta con un proyecto de arqueology-doc.');
      return;
    }
    cli.runInTerminal(this.root, args);
  }

  dispose(): void {
    if (this.timer) {
      clearTimeout(this.timer);
    }
    this.workspaceWatcher?.dispose();
    this.rootWatcher?.dispose();
    for (const d of this.disposables) {
      d.dispose();
    }
  }
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const ctl = new Controller(context);
  context.subscriptions.push(ctl);
  await ctl.locate();

  const terminalCommands: Record<string, string[]> = {
    'adoc.init': ['init'],
    'adoc.scan': ['scan'],
    'adoc.plan': ['plan'],
    'adoc.planForce': ['plan', '--force'],
    'adoc.inspect': ['inspect'],
    'adoc.resume': ['inspect', '--resume'],
    'adoc.review': ['review'],
    'adoc.check': ['check'],
    'adoc.sync': ['sync'],
    'adoc.doctor': ['doctor'],
    'adoc.siteDev': ['site', 'dev'],
    'adoc.siteBuild': ['site', 'build'],
    'adoc.preview': ['preview'],
    'adoc.report': ['report'],
  };
  for (const [id, args] of Object.entries(terminalCommands)) {
    context.subscriptions.push(vscode.commands.registerCommand(id, () => ctl.run(args)));
  }

  const register = (id: string, fn: (...a: any[]) => unknown) =>
    context.subscriptions.push(vscode.commands.registerCommand(id, fn));

  register('adoc.refresh', () => ctl.refresh());
  register('adoc.dashboard', () => DashboardPanel.show(context.extensionUri, ctl, 'summary'));
  register('adoc.graph', () => DashboardPanel.show(context.extensionUri, ctl, 'graph'));

  register('adoc.selectProject', async (node?: ProjectNode) => {
    let project = node?.project;
    if (!project) {
      const rows = ctl.projects.filter((p) => p.available);
      if (!rows.length) {
        void vscode.window.showInformationMessage('No hay proyectos registrados en ~/.arqueology-doc.');
        return;
      }
      const pick = await vscode.window.showQuickPick(
        rows.map((p) => ({
          label: p.name,
          description: p.progress !== null ? `${p.progress}% · ${p.agent ?? ''}` : undefined,
          detail: p.path,
          project: p,
        })),
        { title: 'Proyecto de arqueology-doc' },
      );
      project = pick?.project;
    }
    if (project) {
      await ctl.select(project);
    }
  });
  register('adoc.openProjectFolder', async (node: ProjectNode) => {
    await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(node.project.path), {
      forceNewWindow: true,
    });
  });

  register('adoc.task.run', (node: TaskNode) => ctl.run(['task', 'run', node.task.id]));
  register('adoc.task.runInteractive', (node: TaskNode) =>
    ctl.run(['task', 'run', node.task.id, '--interactive']),
  );
  register('adoc.task.done', (node: TaskNode) => ctl.run(['task', 'done', node.task.id]));
  register('adoc.task.copyCommand', async (node: TaskNode) => {
    const line = cli.commandLine(['task', 'run', node.task.id]);
    await vscode.env.clipboard.writeText(line);
    void vscode.window.showInformationMessage(`Copiado: ${line}`);
  });
  register('adoc.task.openLog', async (node: TaskNode) => {
    if (!node.task.log) {
      void vscode.window.showInformationMessage(`${node.task.id} todavía no tiene log.`);
      return;
    }
    await vscode.window.showTextDocument(vscode.Uri.file(node.task.log));
  });
  register('adoc.ask', async () => {
    const question = await vscode.window.showInputBox({
      title: 'Preguntar al agente sobre el proyecto',
      prompt: 'La respuesta, con evidencias, queda en knowledge/faq/ y en la FAQ del preview',
      placeHolder: '¿Cómo se calcula el total de una reserva?',
      ignoreFocusOut: true,
    });
    if (question) {
      ctl.run(['ask', question]);
    }
  });
  register('adoc.answerQuestion', (task: string, question: string) =>
    vscode.commands.executeCommand('adoc.task.answer', {
      task: { id: task, question },
    }),
  );

  register('adoc.task.answer', async (node: { task: { id: string; question: string | null } }) => {
    const answer = await vscode.window.showInputBox({
      title: `Responder a ${node.task.id}`,
      prompt: node.task.question ?? 'Instrucción adicional para el agente',
      placeHolder: 'Tu respuesta',
      ignoreFocusOut: true,
    });
    if (!answer || !ctl.root) {
      return;
    }
    try {
      await cli.exec(ctl.root, ['task', 'answer', node.task.id, answer]);
      const next = await vscode.window.showInformationMessage(
        `Respuesta guardada en ${node.task.id}.`,
        'Ejecutar ahora',
      );
      if (next) {
        ctl.run(['task', 'run', node.task.id]);
      }
    } catch (err) {
      void vscode.window.showErrorMessage((err as Error).message);
    }
    ctl.scheduleRefresh(0);
  });

  register('adoc.selectAgent', async () => {
    const status = ctl.status;
    if (!status || !ctl.root) {
      return;
    }
    const pick = await vscode.window.showQuickPick(
      status.agent.available.map((name) => ({
        label: name,
        description: name === status.agent.active ? 'activo' : undefined,
      })),
      { title: 'Agente para las tareas', placeHolder: `Activo: ${status.agent.active}` },
    );
    if (!pick) {
      return;
    }
    const model = await vscode.window.showInputBox({
      title: `Modelo para ${pick.label}`,
      prompt: 'Vacío = modelo por defecto del agente',
      value: pick.label === status.agent.active ? status.agent.model ?? '' : '',
      ignoreFocusOut: true,
    });
    if (model === undefined) {
      return;
    }
    const args = ['agent', 'use', pick.label, ...(model ? ['--model', model] : ['--no-model'])];
    try {
      const out = await cli.exec(ctl.root, args);
      void vscode.window.showInformationMessage(out.trim().split('\n').slice(0, 2).join(' · '));
    } catch (err) {
      void vscode.window.showErrorMessage((err as Error).message);
    }
    ctl.scheduleRefresh(0);
  });

  register('adoc.menu', async () => {
    const items: (vscode.QuickPickItem & { command: string })[] = [
      { label: '$(dashboard) Abrir dashboard', description: 'resumen, tareas, hallazgos, preguntas', command: 'adoc.dashboard' },
      { label: '$(preview) Ver resultado', description: 'adoc preview', command: 'adoc.preview' },
      { label: '$(type-hierarchy) Grafo del conocimiento', description: 'adoc graph', command: 'adoc.graph' },
      { label: '$(comment-discussion) Preguntar al agente', description: 'adoc ask', command: 'adoc.ask' },
      { label: '$(play) Ejecutar tareas', description: 'adoc inspect', command: 'adoc.inspect' },
      { label: '$(debug-restart) Reintentar fallidas', description: 'adoc inspect --resume', command: 'adoc.resume' },
      { label: '$(hubot) Cambiar agente o modelo', description: 'adoc agent use', command: 'adoc.selectAgent' },
      { label: '$(folder-library) Cambiar de proyecto', description: '~/.arqueology-doc', command: 'adoc.selectProject' },
      { label: '$(search) Escanear', description: 'adoc scan', command: 'adoc.scan' },
      { label: '$(list-ordered) Planificar', description: 'adoc plan', command: 'adoc.plan' },
      { label: '$(list-ordered) Rehacer el plan', description: 'adoc plan --force', command: 'adoc.planForce' },
      { label: '$(sync) Detectar cambios', description: 'adoc sync', command: 'adoc.sync' },
      { label: '$(checklist) Validar', description: 'adoc check', command: 'adoc.check' },
      { label: '$(graph) Resumen', description: 'adoc review', command: 'adoc.review' },
      { label: '$(file-code) Reporte HTML', description: 'adoc report', command: 'adoc.report' },
      { label: '$(globe) Sitio (dev)', description: 'adoc site dev', command: 'adoc.siteDev' },
      { label: '$(pulse) Verificar requisitos', description: 'adoc doctor', command: 'adoc.doctor' },
    ];
    const pick = await vscode.window.showQuickPick(items, { title: 'arqueology-doc' });
    if (pick) {
      await vscode.commands.executeCommand(pick.command);
    }
  });

  // Sin `await`: el primer estado se carga en segundo plano.
  ctl.refresh().catch((err) => console.error('arqueology-doc: refresh', err));
}

export function deactivate(): void {}
