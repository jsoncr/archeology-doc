// Internacionalización de la extensión (inglés y español).
//
// No se usa solo `vscode.l10n`: ese idioma lo fija VS Code y no se puede
// cambiar con un ajuste. Aquí el idioma sale de `archeologyDoc.language`
// (`auto` = idioma de VS Code) y se aplica a todo lo que se genera en tiempo
// de ejecución: avisos, quick picks, árboles, barra de estado, webviews y el
// CLI (`ADOC_LANG`). Los títulos de comandos y vistas de package.json usan
// package.nls*.json y siguen el idioma de VS Code (limitación de VS Code).
//
// Los textos están en strings.ts (cómo añadir uno: ver su cabecera);
// `npm run compile` además compara las claves de en/es y de package.nls*.json.

import * as vscode from 'vscode';
import { dictionaries, Key, Lang } from './strings';

export type { Key, Lang };

const en = dictionaries.en;

/** Prefijos de las claves que se envían al webview. */
const WEBVIEW_PREFIXES = ['web.', 'kind.', 'origin.'];

let cached: Lang | undefined;

/**
 * Idioma resuelto: `archeologyDoc.language` si es `en`/`es`; con `auto`,
 * el idioma de VS Code (`es*` → es, si no en).
 */
export function language(): Lang {
  if (cached) {
    return cached;
  }
  const setting = vscode.workspace.getConfiguration('archeologyDoc').get<string>('language', 'auto');
  if (setting === 'en' || setting === 'es') {
    cached = setting;
  } else {
    cached = (vscode.env.language || 'en').toLowerCase().startsWith('es') ? 'es' : 'en';
  }
  return cached;
}

/** Texto de `key` en el idioma resuelto, con los `{nombre}` sustituidos por `params`. */
export function t(key: Key, params?: Record<string, string | number>): string {
  const text = dictionaries[language()][key] ?? en[key] ?? key;
  if (!params) {
    return text;
  }
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    params[name] !== undefined ? String(params[name]) : match,
  );
}

/** Subconjunto del diccionario que necesita el webview (con respaldo en inglés). */
export function webviewStrings(): Record<string, string> {
  const dict = dictionaries[language()];
  const out: Record<string, string> = {};
  for (const key of Object.keys(en) as Key[]) {
    if (WEBVIEW_PREFIXES.some((p) => key.startsWith(p))) {
      out[key] = dict[key] ?? en[key];
    }
  }
  return out;
}

/** Avisa cuando cambia `archeologyDoc.language` (y olvida el idioma en caché). */
export function watchLanguage(onChange: () => void): vscode.Disposable {
  return vscode.workspace.onDidChangeConfiguration((e) => {
    if (e.affectsConfiguration('archeologyDoc.language')) {
      const before = cached;
      cached = undefined;
      if (language() !== before) {
        onChange();
      }
    }
  });
}
