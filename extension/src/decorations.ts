// Marcas en el editor: las líneas citadas como evidencia de un hallazgo
// muestran un icono en el margen y el hallazgo al pasar el cursor (RF-142).

import * as path from 'path';
import * as vscode from 'vscode';
import { t } from './i18n';
import { FindingInfo, parseEvidence, Status } from './types';

const SEVERITY_COLORS: Record<string, string> = {
  critical: 'editorError.foreground',
  high: 'editorError.foreground',
  medium: 'editorWarning.foreground',
  low: 'editorInfo.foreground',
};

export class FindingDecorations implements vscode.Disposable {
  private readonly types = new Map<string, vscode.TextEditorDecorationType>();
  private status: Status | undefined;
  private readonly subscription: vscode.Disposable;

  constructor() {
    for (const [severity, color] of Object.entries(SEVERITY_COLORS)) {
      this.types.set(
        severity,
        vscode.window.createTextEditorDecorationType({
          isWholeLine: true,
          overviewRulerColor: new vscode.ThemeColor(color),
          overviewRulerLane: vscode.OverviewRulerLane.Right,
          borderWidth: '0 0 0 2px',
          borderStyle: 'solid',
          borderColor: new vscode.ThemeColor(color),
        }),
      );
    }
    this.subscription = vscode.window.onDidChangeVisibleTextEditors(() => this.render());
  }

  update(status: Status | undefined): void {
    this.status = status;
    this.render();
  }

  private render(): void {
    const enabled = vscode.workspace.getConfiguration('archeologyDoc').get<boolean>('decorations', true);
    for (const editor of vscode.window.visibleTextEditors) {
      const bySeverity = new Map<string, vscode.DecorationOptions[]>();
      if (enabled && this.status) {
        for (const f of this.status.findings) {
          if (f.state === 'dismissed' || f.state === 'resolved') {
            continue;
          }
          for (const src of f.sources) {
            const ev = parseEvidence(src);
            if (ev.start === undefined) {
              continue;
            }
            const file = path.join(this.status.project.root, ev.file);
            if (path.normalize(editor.document.uri.fsPath) !== path.normalize(file)) {
              continue;
            }
            const end = Math.min(ev.end ?? ev.start, editor.document.lineCount);
            const range = new vscode.Range(ev.start - 1, 0, Math.max(ev.start, end) - 1, 0);
            const severity = SEVERITY_COLORS[f.severity] ? f.severity : 'low';
            const list = bySeverity.get(severity) ?? [];
            list.push({ range, hoverMessage: hover(f) });
            bySeverity.set(severity, list);
          }
        }
      }
      for (const [severity, type] of this.types) {
        editor.setDecorations(type, bySeverity.get(severity) ?? []);
      }
    }
  }

  dispose(): void {
    this.subscription.dispose();
    for (const t of this.types.values()) {
      t.dispose();
    }
  }
}

function hover(f: FindingInfo): vscode.MarkdownString {
  const md = new vscode.MarkdownString(undefined, true);
  md.isTrusted = { enabledCommands: ['vscode.open'] };
  const open = `command:vscode.open?${encodeURIComponent(JSON.stringify([vscode.Uri.file(f.file)]))}`;
  // Título, tipo y estado los escribe un agente: se escapan para que no puedan
  // inyectar enlaces `command:` en un markdown que permite comandos.
  md.appendMarkdown(`$(warning) **${escapeMd(f.severity)}** · ${escapeMd(f.kind)} · ${escapeMd(f.state)}\n\n`);
  md.appendMarkdown(`**${escapeMd(f.title)}**\n\n[${escapeMd(t('decorations.openFinding'))}](${open})`);
  return md;
}

/** Escapa la sintaxis de Markdown (y los `$(icono)`) de un texto. */
function escapeMd(text: string): string {
  return String(text ?? '').replace(/[\\`*_{}[\]()#+\-.!|<>$~]/g, '\\$&');
}
