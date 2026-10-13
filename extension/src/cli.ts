// Acceso al CLI `adoc`: la extensión no tiene lógica propia, solo lee el
// JSON de `adoc status` y ejecuta comandos.

import { execFile } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { language } from './i18n';
import { ProjectRow, Status } from './types';

const TERMINAL_NAME = 'adoc';

/** Límite para los comandos que se ejecutan en segundo plano. */
const EXEC_TIMEOUT_MS = 120_000;

/**
 * Ruta del binario: configuración de usuario, PATH o ~/.cargo/bin.
 *
 * Solo se acepta el valor global (usuario): un `.vscode/settings.json` del
 * repositorio abierto no puede decidir qué programa se ejecuta.
 */
export function adocPath(): string {
  // `arqueologyDoc.path`: clave con el nombre anterior del proyecto.
  const configured =
    vscode.workspace.getConfiguration('archeologyDoc').inspect<string>('path')?.globalValue ||
    vscode.workspace.getConfiguration('arqueologyDoc').inspect<string>('path')?.globalValue;
  if (configured) {
    return configured;
  }
  return installedAdoc() ?? 'adoc';
}

/**
 * `adoc` en las carpetas donde lo dejan los instaladores (o cargo). VS Code
 * abierto desde el Dock no siempre hereda el PATH del shell.
 */
export function installedAdoc(): string | undefined {
  const exe = process.platform === 'win32' ? 'adoc.exe' : 'adoc';
  const dirs =
    process.platform === 'win32'
      ? [path.join(process.env.LOCALAPPDATA ?? path.join(os.homedir(), 'AppData', 'Local'), 'Programs', 'archeology-doc', 'bin')]
      : [path.join(os.homedir(), '.local', 'bin')];
  dirs.push(path.join(os.homedir(), '.cargo', 'bin'));
  return dirs.map((d) => path.join(d, exe)).find((f) => fs.existsSync(f));
}

/**
 * Variables de entorno para cada invocación del CLI: `ADOC_LANG` lleva el
 * idioma resuelto de la extensión (los mensajes de `adoc` salen en ese idioma;
 * las claves y valores del JSON no cambian).
 */
export function cliEnv(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  return { ...process.env, ...extra, ADOC_LANG: language() };
}

/** Lo mismo para `createTerminal`: solo lo que se añade al entorno heredado. */
export function terminalEnv(): Record<string, string> {
  return { ADOC_LANG: language() };
}

/** Ejecuta `adoc <args>` y devuelve stdout; rechaza con stderr si falla. */
export function exec(root: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(
      adocPath(),
      args,
      {
        cwd: root,
        maxBuffer: 64 * 1024 * 1024,
        timeout: EXEC_TIMEOUT_MS,
        env: cliEnv({ ADOC_NO_PROGRESS: '1' }),
      },
      (err, stdout, stderr) => {
        if (err) {
          reject(new Error((stderr || err.message).trim()));
        } else {
          resolve(stdout);
        }
      },
    );
  });
}

/** Versión del CLI, o `undefined` si no está instalado. */
export async function version(root: string): Promise<string | undefined> {
  try {
    return (await exec(root, ['--version'])).trim();
  } catch {
    return undefined;
  }
}

/** Estado completo del proyecto (`adoc status --format json`). */
export async function status(root: string): Promise<Status> {
  return JSON.parse(await exec(root, ['status', '--format', 'json'])) as Status;
}

/** Directorio global de archeology-doc (registro, espacios de trabajo). */
export function configDir(): string {
  const env = process.env.ARCHEOLOGY_DOC_CONFIG_DIR;
  return env ? path.resolve(env) : path.join(os.homedir(), '.archeology-doc');
}

/** Proyectos del registro global (`~/.archeology-doc/registry.toml`). */
export async function projects(): Promise<ProjectRow[]> {
  return JSON.parse(await exec(os.homedir(), ['projects', 'list', '--format', 'json'])) as ProjectRow[];
}

/**
 * Argumento que ningún shell (bash, zsh, cmd, PowerShell) interpreta. Sin
 * `@` (splatting), `%` (variables de cmd) ni `,` (arrays de PowerShell).
 */
function isPlain(arg: string): boolean {
  return /^[\w+=:./-]+$/.test(arg);
}

/** Comillas POSIX, solo para mostrar o copiar el comando. */
function quote(arg: string): string {
  if (isPlain(arg)) {
    return arg;
  }
  return `'${arg.replace(/'/g, `'\\''`)}'`;
}

/** Texto del comando, para mostrarlo o copiarlo. */
export function commandLine(args: string[]): string {
  return ['adoc', ...args].map(quote).join(' ');
}

/**
 * Ruta del binario tal como se escribe en el shell del terminal, o `undefined`
 * si no se puede escribir sin comillas (p. ej. tiene espacios).
 *
 * En Windows la ruta siempre lleva `\`: PowerShell y cmd la aceptan tal cual;
 * bash (Git Bash, MSYS) interpreta `\` como escape, así que ahí va con `/`.
 */
function shellBinary(bin: string): string | undefined {
  if (process.platform !== 'win32') {
    return isPlain(bin) ? bin : undefined;
  }
  if (!/^[\w+=:.\\/-]+$/.test(bin)) {
    return undefined;
  }
  const shell = path.basename(vscode.env.shell || '').toLowerCase();
  return /^(bash|sh|zsh|fish)(\.exe)?$/.test(shell) ? bin.replace(/\\/g, '/') : bin;
}

/**
 * Terminales con un proceso en marcha (`adoc inspect`, `task run -i`…): no se
 * les envía texto, se iría a la entrada del agente. Sin integración del shell
 * no se sabe cuándo termina un comando, así que el terminal queda ocupado.
 */
const busyTerminals = new Set<vscode.Terminal>();

/**
 * Sigue el inicio y fin de los comandos con la integración del shell
 * (VS Code 1.93+; en versiones anteriores esos eventos no existen).
 */
export function trackTerminals(): vscode.Disposable {
  const win = vscode.window as Partial<typeof vscode.window>;
  const subs: vscode.Disposable[] = [];
  if (win.onDidStartTerminalShellExecution && win.onDidEndTerminalShellExecution) {
    subs.push(
      win.onDidStartTerminalShellExecution((e) => busyTerminals.add(e.terminal)),
      win.onDidEndTerminalShellExecution((e) => busyTerminals.delete(e.terminal)),
    );
  }
  subs.push(vscode.window.onDidCloseTerminal((t) => busyTerminals.delete(t)));
  return vscode.Disposable.from(...subs);
}

/**
 * Ejecuta en el terminal integrado: se ve el progreso (spinner) y funciona
 * el modo interactivo de los agentes.
 */
export function runInTerminal(root: string, args: string[]): void {
  const bin = adocPath();
  const shellBin = shellBinary(bin);
  // Texto libre (preguntas, respuestas) o rutas con espacios: el comando se
  // lanza sin shell, así ni PowerShell ni cmd interpretan `"`, `;` o `$()`.
  if (shellBin === undefined || !args.every(isPlain)) {
    const direct = vscode.window.createTerminal({
      name: `${TERMINAL_NAME} · ${args[0] ?? ''}`,
      cwd: root,
      shellPath: bin,
      shellArgs: args,
      env: terminalEnv(),
    });
    direct.show(true);
    return;
  }
  // Un terminal por proyecto: al cambiar de proyecto no se ejecuta en otra carpeta.
  // Si está ocupado con otro comando, se abre uno nuevo con el mismo nombre.
  // Si se creó con otro idioma (`ADOC_LANG`), tampoco se reutiliza: su entorno
  // ya no se puede cambiar.
  const name = `${TERMINAL_NAME} · ${path.basename(root)}`;
  const env = terminalEnv();
  let terminal = vscode.window.terminals.find(
    (t) =>
      t.name === name &&
      t.exitStatus === undefined &&
      !busyTerminals.has(t) &&
      (t.creationOptions as vscode.TerminalOptions).env?.ADOC_LANG === env.ADOC_LANG,
  );
  if (!terminal) {
    terminal = vscode.window.createTerminal({ name, cwd: root, env });
  }
  terminal.show(true);
  // Ocupado desde ya: el evento de inicio llega después y puede no llegar nunca.
  busyTerminals.add(terminal);
  terminal.sendText([shellBin, ...args].join(' '));
}
