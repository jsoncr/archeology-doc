// Acceso al CLI `adoc`: la extensión no tiene lógica propia, solo lee el
// JSON de `adoc status` y ejecuta comandos.

import { execFile } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
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
        env: { ...process.env, ADOC_NO_PROGRESS: '1' },
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
 * Ejecuta en el terminal integrado: se ve el progreso (spinner) y funciona
 * el modo interactivo de los agentes.
 */
export function runInTerminal(root: string, args: string[]): void {
  const bin = adocPath();
  // Texto libre (preguntas, respuestas) o rutas con espacios: el comando se
  // lanza sin shell, así ni PowerShell ni cmd interpretan `"`, `;` o `$()`.
  if (![bin, ...args].every(isPlain)) {
    const direct = vscode.window.createTerminal({
      name: `${TERMINAL_NAME} · ${args[0] ?? ''}`,
      cwd: root,
      shellPath: bin,
      shellArgs: args,
    });
    direct.show(true);
    return;
  }
  // Un terminal por proyecto: al cambiar de proyecto no se ejecuta en otra carpeta.
  const name = `${TERMINAL_NAME} · ${path.basename(root)}`;
  let terminal = vscode.window.terminals.find((t) => t.name === name && t.exitStatus === undefined);
  if (!terminal) {
    terminal = vscode.window.createTerminal({ name, cwd: root });
  }
  terminal.show(true);
  terminal.sendText([bin, ...args].join(' '));
}
