// Elección del proyecto activo a partir del registro global (sin API de VS Code).

import * as path from 'path';
import { ProjectRow } from './types';

/**
 * 1. el elegido antes en este workspace, si sigue disponible;
 * 2. un proyecto cuya ruta es una carpeta abierta;
 * 3. el más cercano que contiene una carpeta abierta (estás en una subcarpeta);
 * 4. uno dentro de una carpeta abierta (carpeta padre o monorepo), el más reciente.
 */
export function matchProject(
  rows: ProjectRow[],
  folders: string[],
  saved?: string,
): ProjectRow | undefined {
  const available = rows.filter((p) => p.available);
  return (
    available.find((p) => p.id === saved) ??
    available.find((p) => folders.includes(p.path)) ??
    available
      .filter((p) => folders.some((f) => f.startsWith(p.path + path.sep)))
      .sort((a, b) => b.path.length - a.path.length)[0] ??
    available
      .filter((p) => folders.some((f) => p.path.startsWith(f + path.sep)))
      .sort((a, b) => b.last_activity.localeCompare(a.last_activity))[0]
  );
}
