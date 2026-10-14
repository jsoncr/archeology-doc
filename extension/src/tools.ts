// Herramientas que necesita la extensión: el CLI `adoc` y, para el sitio,
// Node y VitePress. Al iniciar se comprueban y se ofrece instalar lo que falte;
// nada se instala sin que el usuario lo pida con un clic.

import { execFile } from 'child_process';
import * as os from 'os';
import * as vscode from 'vscode';
import * as cli from './cli';
import { t } from './i18n';

/** Carpeta /download del sitio oficial (instaladores y binarios). */
const DEFAULT_DOWNLOAD = 'https://archeology-doc.com/download';
/** Espera máxima a que el instalador deje el binario. */
const INSTALL_WAIT_MS = 15 * 60_000;

interface DoctorCheck {
  name: string;
  level: 'ok' | 'warn' | 'fail';
  detail: string;
  hint?: string;
}

/** URL de descarga: configuración de usuario o la oficial. Solo https. */
function downloadUrl(): string {
  const configured = vscode.workspace.getConfiguration('archeologyDoc').inspect<string>('downloadUrl')?.globalValue;
  const url = (configured || DEFAULT_DOWNLOAD).replace(/\/+$/, '');
  // Va dentro de una línea de shell: solo caracteres de URL sin comillas ni espacios.
  if (!/^https:\/\/[A-Za-z0-9._~\/:-]+$/.test(url)) {
    throw new Error(t('tools.invalidUrl', { url }));
  }
  return url;
}

/** `<bin> --version`, o `undefined` si no se puede ejecutar (archivo roto, a medio descargar…). */
function probeVersion(bin: string): Promise<string | undefined> {
  return new Promise((resolve) => {
    execFile(bin, ['--version'], { timeout: 10_000, env: cli.cliEnv() }, (err, stdout) => {
      resolve(err ? undefined : stdout.trim() || undefined);
    });
  });
}

/**
 * Abre un terminal con el instalador oficial. Se ve todo lo que hace y el
 * usuario puede cancelarlo. Cuando el binario responde a `--version` (con una
 * versión distinta de `previous`, si se está actualizando), se llama a `onInstalled`.
 */
export function installCli(onInstalled: () => void, previous?: string): void {
  let url: string;
  try {
    url = downloadUrl();
  } catch (err) {
    void vscode.window.showErrorMessage((err as Error).message);
    return;
  }
  openInstallTerminal(
    t('tools.installTerminal'),
    { sh: `curl -fsSL '${url}/install.sh' | sh`, ps: `irm '${url}/install.ps1' | iex` },
    {},
    () => cli.installedAdoc(),
    onInstalled,
    previous,
  );
}

/**
 * Actualiza el CLI con `adoc upgrade --yes` en un terminal (descarga,
 * verifica y sustituye los binarios donde estén). Si falla o el CLI es
 * anterior a `upgrade`, se ejecuta el instalador oficial.
 */
export function upgradeCli(onInstalled: () => void, previous?: string): void {
  let url: string;
  try {
    url = downloadUrl();
  } catch (err) {
    void vscode.window.showErrorMessage((err as Error).message);
    return;
  }
  // El binario y la URL van por variables de entorno: no se interpolan en la línea de shell.
  const env = { ADOC_BIN: cli.adocPath(), ARCHEOLOGY_DOC_DOWNLOAD: url };
  openInstallTerminal(
    t('tools.upgradeTerminal'),
    {
      sh: `"$ADOC_BIN" upgrade --yes || { echo; echo '${shellText(t('tools.upgradeFallback'))}'; curl -fsSL '${url}/install.sh' | sh; }`,
      ps:
        `& $env:ADOC_BIN upgrade --yes; if (-not $?) { Write-Host ''; ` +
        `Write-Host '${shellText(t('tools.upgradeFallback'))}'; irm '${url}/install.ps1' | iex }`,
    },
    env,
    // `adoc upgrade` sustituye el binario configurado; el instalador, el de su carpeta.
    () => cli.adocPath(),
    onInstalled,
    previous,
  );
}

/** Texto traducido dentro de comillas simples de sh/PowerShell: sin comillas. */
function shellText(text: string): string {
  return text.replace(/'/g, '');
}

/**
 * Terminal con `script` (sh en macOS/Linux, PowerShell en Windows). El
 * comando no avisa al terminar: se espera a que `probe()` dé un binario que
 * responda a `--version` con una versión distinta de `previous`.
 */
function openInstallTerminal(
  name: string,
  script: { sh: string; ps: string },
  extraEnv: Record<string, string>,
  probe: () => string | undefined,
  onInstalled: () => void,
  previous?: string,
): void {
  const closeHint = shellText(t('tools.closeTerminal'));
  const env = { ...cli.terminalEnv(), ...extraEnv };
  const terminal =
    process.platform === 'win32'
      ? vscode.window.createTerminal({
          name,
          env,
          shellPath: 'powershell.exe',
          shellArgs: ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', script.ps],
        })
      : vscode.window.createTerminal({
          name,
          env,
          shellPath: '/bin/sh',
          shellArgs: ['-c', `${script.sh}; echo; echo '${closeHint}'; read _`],
        });
  terminal.show();

  // Existir no basta: el binario puede estar a medio escribir.
  const started = Date.now();
  let probing = false;
  const timer = setInterval(async () => {
    if (probing) {
      return;
    }
    probing = true;
    try {
      const found = probe();
      const current = found ? await probeVersion(found) : undefined;
      const ready = !!current && current !== previous;
      if (ready || Date.now() - started > INSTALL_WAIT_MS) {
        clearInterval(timer);
        if (ready) {
          void vscode.window.showInformationMessage(t('tools.installed', { version: current ?? '', path: found ?? '' }));
          onInstalled();
        }
      }
    } finally {
      probing = false;
    }
  }, 2000);
}

/** `adoc doctor --format json`; su código de salida es 1 si algo falla, pero el JSON sirve igual. */
function doctor(root: string | undefined): Promise<DoctorCheck[]> {
  return new Promise((resolve) => {
    execFile(
      cli.adocPath(),
      ['doctor', '--format', 'json'],
      // En el proyecto activo, doctor también revisa su configuración y su agente.
      { cwd: root ?? os.homedir(), timeout: 60_000, env: cli.cliEnv({ ADOC_NO_PROGRESS: '1' }) },
      (_err, stdout) => {
        try {
          resolve((JSON.parse(stdout) as { checks: DoctorCheck[] }).checks ?? []);
        } catch {
          resolve([]);
        }
      },
    );
  });
}

let checkedThisSession = false;

/**
 * Comprueba las herramientas y ofrece instalar las que faltan.
 * `force`: desde el comando de la paleta (informa también si todo está bien).
 */
export async function checkTools(
  host: { root: string | undefined; run(args: string[]): void; refresh(): Promise<void> },
  force = false,
): Promise<void> {
  const config = vscode.workspace.getConfiguration('archeologyDoc');
  if (!force && (checkedThisSession || !config.get<boolean>('checkToolsOnStartup', true))) {
    return;
  }
  checkedThisSession = true;

  const current = await cli.version(host.root ?? os.homedir());
  if (!current) {
    const install = t('tools.install');
    const configure = t('tools.configurePath');
    const never = t('tools.dontAskAgain');
    const choice = await vscode.window.showWarningMessage(t('tools.cliMissing'), install, configure, never);
    if (choice === install) {
      installCli(() => void host.refresh().then(() => checkTools(host, true)));
    } else if (choice === configure) {
      await vscode.commands.executeCommand('workbench.action.openSettings', 'archeologyDoc.path');
    } else if (choice === never) {
      await config.update('checkToolsOnStartup', false, vscode.ConfigurationTarget.Global);
    }
    return;
  }

  const checks = await doctor(host.root);
  const missing = (name: string) => checks.some((c) => c.name === name && c.level !== 'ok');
  const actions: { label: string; run: () => void }[] = [];
  if (missing('git')) {
    actions.push({ label: t('tools.downloadGit'), run: () => void vscode.env.openExternal(vscode.Uri.parse('https://git-scm.com/downloads')) });
  }
  if (missing('node')) {
    actions.push({ label: t('tools.downloadNode'), run: () => void vscode.env.openExternal(vscode.Uri.parse('https://nodejs.org/')) });
  } else if (missing('vitepress')) {
    actions.push({ label: t('tools.installVitepress'), run: () => host.run(['site', 'setup']) });
  }

  if (actions.length === 0) {
    if (force) {
      // Desde la paleta: todo está, pero se ofrece actualizar el CLI (p. ej. tras actualizar la extensión).
      const update = t('tools.update');
      const choice = await vscode.window.showInformationMessage(t('tools.allInstalled', { version: current }), update);
      if (choice === update) {
        upgradeCli(() => void host.refresh(), current);
      }
    }
    return;
  }
  const names = [missing('git') && 'git', missing('node') && 'Node', !missing('node') && missing('vitepress') && 'VitePress']
    .filter(Boolean)
    .join(', ');
  const diagnose = t('tools.showDiagnostics');
  const choice = await vscode.window.showWarningMessage(
    t('tools.missing', { names }),
    ...actions.map((a) => a.label),
    diagnose,
  );
  if (choice === diagnose) {
    host.run(['doctor']);
  } else {
    actions.find((a) => a.label === choice)?.run();
  }
}
