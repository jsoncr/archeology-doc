// Vistas del panel lateral: tareas, hallazgos, conocimiento y specs.

import * as path from 'path';
import * as vscode from 'vscode';
import { FindingInfo, NoteInfo, parseEvidence, ProjectRow, Status, TaskInfo } from './types';

type Node = vscode.TreeItem & { children?: Node[] };

/** Base común: los nodos se construyen al recibir un nuevo `Status`. */
abstract class StatusTree implements vscode.TreeDataProvider<Node> {
  private readonly emitter = new vscode.EventEmitter<Node | undefined>();
  readonly onDidChangeTreeData = this.emitter.event;
  protected roots: Node[] = [];

  update(status: Status | undefined): void {
    this.roots = status ? this.build(status) : [];
    this.emitter.fire(undefined);
  }

  protected abstract build(status: Status): Node[];

  getTreeItem(node: Node): vscode.TreeItem {
    return node;
  }

  getChildren(node?: Node): Node[] {
    return node ? node.children ?? [] : this.roots;
  }
}

function group(label: string, children: Node[], icon: string, expanded = true): Node {
  const node: Node = new vscode.TreeItem(
    `${label} (${children.length})`,
    expanded ? vscode.TreeItemCollapsibleState.Expanded : vscode.TreeItemCollapsibleState.Collapsed,
  );
  node.iconPath = new vscode.ThemeIcon(icon);
  node.children = children;
  return node;
}

function openCommand(file: string, line?: number): vscode.Command {
  const args: unknown[] = [vscode.Uri.file(file)];
  if (line !== undefined) {
    const pos = new vscode.Position(Math.max(0, line - 1), 0);
    args.push({ selection: new vscode.Range(pos, pos) });
  }
  return { command: 'vscode.open', title: 'Abrir', arguments: args };
}

// ── Tareas ────────────────────────────────────────────────────────────────

const TASK_STATES: { status: string; label: string; icon: string; color?: string }[] = [
  { status: 'question', label: 'Esperan tu respuesta', icon: 'question', color: 'charts.yellow' },
  { status: 'failed', label: 'Fallidas', icon: 'error', color: 'errorForeground' },
  { status: 'running', label: 'En curso / interrumpidas', icon: 'sync' },
  { status: 'pending', label: 'Pendientes', icon: 'circle-large-outline' },
  { status: 'manual', label: 'Manuales', icon: 'person' },
  { status: 'done', label: 'Hechas', icon: 'pass', color: 'testing.iconPassed' },
];

export class TaskNode extends vscode.TreeItem {
  constructor(public readonly task: TaskInfo, icon: string, color?: string) {
    super(task.id, vscode.TreeItemCollapsibleState.None);
    const modules = task.module ? task.module.split(',') : [];
    const scope =
      modules.length === 0 ? '' : modules.length <= 2 ? modules.join(', ') : `${modules.length} módulos`;
    this.description = scope ? `${task.kind} · ${scope}` : task.kind;
    this.iconPath = new vscode.ThemeIcon(icon, color ? new vscode.ThemeColor(color) : undefined);
    this.contextValue = `task.${task.status}`;
    this.command = openCommand(task.file);
    const md = new vscode.MarkdownString();
    md.appendMarkdown(`**${task.id}** · ${task.kind} · \`${task.status}\`\n\n`);
    if (task.question) {
      md.appendMarkdown(`❓ **Pregunta:** ${task.question}\n\n`);
    }
    if (modules.length) {
      md.appendMarkdown(`Módulos: ${modules.join(', ')}\n\n`);
    }
    if (task.produce.length) {
      md.appendMarkdown(`Produce:\n${task.produce.map((p) => `- \`${p}\``).join('\n')}\n\n`);
    }
    if (task.depends_on.length) {
      md.appendMarkdown(`Depende de: ${task.depends_on.join(', ')}`);
    }
    this.tooltip = md;
  }
}

export class TasksTree extends StatusTree {
  protected build(status: Status): Node[] {
    return TASK_STATES.map((s) => {
      const tasks = status.tasks.filter((t) => t.status === s.status);
      const children = tasks.map((t) => new TaskNode(t, s.icon, s.color) as Node);
      // Las hechas, plegadas: lo importante es lo que falta.
      return children.length ? group(s.label, children, s.icon, s.status !== 'done') : undefined;
    }).filter((n): n is Node => n !== undefined);
  }
}

// ── Hallazgos ─────────────────────────────────────────────────────────────

const SEVERITIES: { severity: string; label: string; color: string }[] = [
  { severity: 'critical', label: 'Críticos', color: 'errorForeground' },
  { severity: 'high', label: 'Altos', color: 'errorForeground' },
  { severity: 'medium', label: 'Medios', color: 'charts.yellow' },
  { severity: 'low', label: 'Bajos', color: 'charts.blue' },
  { severity: 'unknown', label: 'Sin severidad', color: 'descriptionForeground' },
];

function findingNode(f: FindingInfo, root: string, color: string): Node {
  const node: Node = new vscode.TreeItem(
    f.title,
    f.sources.length ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None,
  );
  node.description = `${f.kind} · ${f.state}`;
  const closed = f.state === 'dismissed' || f.state === 'resolved';
  node.iconPath = new vscode.ThemeIcon(closed ? 'pass' : 'warning', new vscode.ThemeColor(color));
  node.command = openCommand(f.file);
  node.tooltip = `${f.id}\n${f.severity} · ${f.kind} · ${f.state}`;
  node.children = f.sources.map((src) => {
    const ev = parseEvidence(src);
    const item: Node = new vscode.TreeItem(src, vscode.TreeItemCollapsibleState.None);
    item.iconPath = new vscode.ThemeIcon('go-to-file');
    item.command = openCommand(path.join(root, ev.file), ev.start);
    return item;
  });
  return node;
}

export class FindingsTree extends StatusTree {
  protected build(status: Status): Node[] {
    return SEVERITIES.map((s) => {
      const items = status.findings
        .filter((f) => (SEVERITIES.some((x) => x.severity === f.severity) ? f.severity : 'unknown') === s.severity)
        .sort((a, b) => Number(a.state !== 'open') - Number(b.state !== 'open'));
      const children = items.map((f) => findingNode(f, status.project.root, s.color));
      return children.length ? group(s.label, children, 'warning') : undefined;
    }).filter((n): n is Node => n !== undefined);
  }
}

// ── Conocimiento ──────────────────────────────────────────────────────────

const NOTE_TYPES: { kind: string; label: string; icon: string }[] = [
  { kind: 'overview', label: 'Overview', icon: 'book' },
  { kind: 'module', label: 'Módulos', icon: 'symbol-module' },
  { kind: 'feature', label: 'Funcionalidades', icon: 'symbol-event' },
  { kind: 'test-report', label: 'Reportes de tests', icon: 'beaker' },
  { kind: 'spec-report', label: 'Cumplimiento de specs', icon: 'checklist' },
  { kind: 'glossary', label: 'Glosario', icon: 'symbol-key' },
  { kind: 'decision', label: 'Decisiones', icon: 'law' },
  { kind: 'memory', label: 'Memoria', icon: 'database' },
  { kind: 'moc', label: 'Índices', icon: 'list-tree' },
];

export class KnowledgeTree extends StatusTree {
  protected build(status: Status): Node[] {
    const stale = new Set(status.stale_notes.map((n) => path.join(status.project.root, n)));
    const known = new Set(NOTE_TYPES.map((t) => t.kind).concat('finding'));
    const types = [...NOTE_TYPES, { kind: 'other', label: 'Otras', icon: 'file' }];
    return types
      .map((t) => {
        const notes = status.notes.filter((n) =>
          t.kind === 'other' ? !known.has(n.kind) : n.kind === t.kind,
        );
        const children = notes
          .sort((a, b) => a.title.localeCompare(b.title))
          .map((n) => noteNode(n, t.icon, stale.has(n.file)));
        return children.length ? group(t.label, children, t.icon, t.kind === 'overview') : undefined;
      })
      .filter((n): n is Node => n !== undefined);
  }
}

function noteNode(n: NoteInfo, icon: string, stale: boolean): Node {
  const node: Node = new vscode.TreeItem(n.title, vscode.TreeItemCollapsibleState.None);
  node.iconPath = new vscode.ThemeIcon(
    stale ? 'history' : icon,
    stale ? new vscode.ThemeColor('charts.yellow') : undefined,
  );
  node.description = stale ? 'desactualizada' : n.status ?? undefined;
  node.command = openCommand(n.file);
  node.tooltip = `${n.file}${n.sources.length ? `\nEvidencias: ${n.sources.length}` : ''}`;
  return node;
}

// ── Specs ─────────────────────────────────────────────────────────────────

const REQ_ICONS: Record<string, [string, string?]> = {
  implemented: ['pass', 'testing.iconPassed'],
  partial: ['circle-large-outline', 'charts.yellow'],
  'not-found': ['circle-slash', 'errorForeground'],
  contradicted: ['error', 'errorForeground'],
  unevaluated: ['question', 'descriptionForeground'],
};

export class SpecsTree extends StatusTree {
  protected build(status: Status): Node[] {
    const specs = status.specs;
    if (!specs) {
      return [];
    }
    return Object.entries(specs.requirements).map(([cap, reqs]) => {
      const children = reqs.map((r) => {
        const [icon, color] = REQ_ICONS[r.status] ?? ['circle-outline'];
        const item: Node = new vscode.TreeItem(r.title, vscode.TreeItemCollapsibleState.None);
        item.description = r.status;
        item.iconPath = new vscode.ThemeIcon(icon, color ? new vscode.ThemeColor(color) : undefined);
        item.tooltip = `${r.id}${r.evidence.length ? `\n${r.evidence.join('\n')}` : ''}`;
        const first = r.evidence[0];
        if (first) {
          const ev = parseEvidence(first);
          item.command = openCommand(path.join(status.project.root, ev.file), ev.start);
        }
        return item;
      });
      return group(cap, children, 'symbol-namespace');
    });
  }
}

// ── Proyectos (registro global) ───────────────────────────────────────────

export class ProjectNode extends vscode.TreeItem {
  constructor(public readonly project: ProjectRow, active: boolean, inWorkspace: boolean) {
    super(project.name, vscode.TreeItemCollapsibleState.None);
    const parts: string[] = [];
    if (project.progress !== null) {
      parts.push(`${project.progress}%`);
    }
    if (project.agent) {
      parts.push(project.agent);
    }
    if (project.open_findings) {
      parts.push(`${project.open_findings} hallazgos`);
    }
    if (!project.available) {
      parts.push('ruta no encontrada');
    }
    this.description = parts.join(' · ');
    this.iconPath = new vscode.ThemeIcon(
      active ? 'circle-filled' : project.available ? 'circle-outline' : 'warning',
      active ? new vscode.ThemeColor('charts.green') : undefined,
    );
    this.tooltip = `${project.path}\nÚltima actividad: ${project.last_activity.replace('T', ' ').slice(0, 16)} (${project.last_command})`;
    this.contextValue = inWorkspace ? 'project.inWorkspace' : 'project.external';
    if (project.available) {
      this.command = { command: 'adoc.selectProject', title: 'Usar este proyecto', arguments: [this] };
    }
  }
}

export class ProjectsTree implements vscode.TreeDataProvider<ProjectNode> {
  private readonly emitter = new vscode.EventEmitter<ProjectNode | undefined>();
  readonly onDidChangeTreeData = this.emitter.event;
  private nodes: ProjectNode[] = [];

  update(rows: ProjectRow[], activeId: string | undefined, workspaceFolders: string[]): void {
    const inside = (p: string) =>
      workspaceFolders.some((f) => p === f || p.startsWith(f + path.sep) || f.startsWith(p + path.sep));
    this.nodes = [...rows]
      // Primero los del workspace abierto, luego por actividad reciente.
      .sort(
        (a, b) =>
          Number(inside(b.path)) - Number(inside(a.path)) ||
          b.last_activity.localeCompare(a.last_activity),
      )
      .map((r) => new ProjectNode(r, r.id === activeId, inside(r.path)));
    this.emitter.fire(undefined);
  }

  getTreeItem(node: ProjectNode): vscode.TreeItem {
    return node;
  }

  getChildren(node?: ProjectNode): ProjectNode[] {
    return node ? [] : this.nodes;
  }
}

// ── Preguntas ─────────────────────────────────────────────────────────────

export class QuestionsTree extends StatusTree {
  protected build(status: Status): Node[] {
    const q = status.questions;
    const agent = q.agent.map((a) => {
      const item: Node = new vscode.TreeItem(a.question, vscode.TreeItemCollapsibleState.None);
      item.description = a.task;
      item.iconPath = new vscode.ThemeIcon('question', new vscode.ThemeColor('charts.yellow'));
      item.tooltip = `${a.task} espera tu respuesta. Clic para responder.`;
      item.command = { command: 'adoc.answerQuestion', title: 'Responder', arguments: [a.task, a.question] };
      return item;
    });
    const openQuestions = path.join(status.project.knowledge, 'memory', 'open-questions.md');
    const team = q.team.map((t) => {
      const item: Node = new vscode.TreeItem(t.replace(/^\d{4}-\d{2}-\d{2} · /, ''), vscode.TreeItemCollapsibleState.None);
      item.iconPath = new vscode.ThemeIcon('organization');
      item.tooltip = t;
      item.command = openCommand(openQuestions);
      return item;
    });
    const faq = q.faq.map((f) => {
      const item: Node = new vscode.TreeItem(f.question, vscode.TreeItemCollapsibleState.None);
      item.description = f.origin === 'agente' ? 'agente' : 'equipo';
      item.iconPath = new vscode.ThemeIcon('comment-discussion');
      item.tooltip = new vscode.MarkdownString(`**${f.question}**\n\n${f.answer}`);
      item.command = openCommand(f.file);
      return item;
    });
    const groups: Node[] = [];
    if (agent.length) {
      groups.push(group('Del agente (bloquean tareas)', agent, 'question'));
    }
    if (team.length) {
      groups.push(group('Para el equipo técnico', team, 'organization'));
    }
    groups.push(group('Preguntas frecuentes', faq, 'comment-discussion', faq.length > 0));
    return groups;
  }
}
