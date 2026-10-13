// Forma del JSON de `adoc status --format json`.

export interface Status {
  project: {
    name: string;
    id: string;
    root: string;
    workspace: string;
    knowledge: string;
    has_inventory: boolean;
  };
  agent: {
    active: string;
    model: string | null;
    mode: 'headless' | 'export' | string;
    available: string[];
  };
  profile: string;
  progress: number;
  tasks: TaskInfo[];
  findings: FindingInfo[];
  notes: NoteInfo[];
  stale_notes: string[];
  specs: SpecsSummary | null;
  severity_totals: Record<string, number>;
  features_without_tests: string[];
  missing_scenarios: number;
  questions: {
    agent: { task: string; question: string }[];
    team: string[];
    faq: { question: string; answer: string; origin: string; file: string }[];
  };
  timing: Timing;
  /** Datos del último `adoc scan`; `null` si todavía no se escaneó. */
  inventory: InventorySummary | null;
  /** Primeros párrafos del overview escritos por el agente. */
  overview_excerpt: string | null;
}

/** Tiempos de las ejecuciones con agente (milisegundos). */
export interface Timing {
  runs: number;
  total_agent_ms: number;
  avg_done_ms: number | null;
  estimated_remaining_ms: number | null;
  last_run_at: string | null;
}

/** Última ejecución con agente de una tarea. */
export interface TaskRunInfo {
  agent: string;
  status: string;
  duration_ms: number;
  attempts: number;
  exit_code: number | null;
  reason: string | null;
  /** Fin de la corrida (RFC 3339). */
  finished_at: string | null;
}

/** Resumen del inventario para el dashboard. */
export interface InventorySummary {
  generated_at: string;
  total_files: number;
  total_bytes: number;
  total_lines: number;
  code_files: number;
  languages: { name: string; files: number; bytes: number; lines: number }[];
  frameworks: string[];
  test_frameworks: string[];
  manifests: string[];
  entrypoints: string[];
  test_files: string[];
  disabled_tests: string[];
  ci_files: string[];
  doc_files: string[];
  markers_by_kind: Record<string, number>;
  markers: { kind: string; path: string; line: number; text: string }[];
  git: {
    commits_analyzed: number;
    authors: string[];
    last_commit_hash: string | null;
    last_commit_date: string | null;
    last_commit_subject: string | null;
    /** Archivos más modificados: [ruta, commits]. */
    hotspots: [string, number][];
  } | null;
  modules: {
    name: string;
    path: string;
    files: number;
    bytes: number;
    languages: string[];
    has_tests: boolean;
    recursive: boolean;
    code_files: number;
  }[];
}

export interface TaskInfo {
  id: string;
  kind: string;
  module: string | null;
  status: string;
  depends_on: string[];
  produce: string[];
  question: string | null;
  file: string;
  log: string | null;
  last_run: TaskRunInfo | null;
}

export interface FindingInfo {
  id: string;
  title: string;
  kind: string;
  severity: string;
  state: string;
  sources: string[];
  file: string;
}

export interface NoteInfo {
  id: string | null;
  kind: string;
  title: string;
  status: string | null;
  sources: string[];
  file: string;
}

export interface SpecsSummary {
  totals: Record<string, number>;
  total_requirements: number;
  requirements: Record<
    string,
    { id: string; title: string; status: string; evidence: string[]; finding: string | null }[]
  >;
  stale_changes: string[];
}

/** Fila de `adoc projects list --format json` (registro global). */
export interface ProjectRow {
  id: string;
  name: string;
  path: string;
  last_activity: string;
  last_command: string;
  available: boolean;
  progress: number | null;
  open_findings: number | null;
  compliance: string | null;
  agent: string | null;
}

/** Evidencia `ruta:línea` o `ruta:a-b` separada en partes. */
export interface Evidence {
  file: string;
  start?: number;
  end?: number;
}

export function parseEvidence(src: string): Evidence {
  const m = /^(.*):(\d+)(?:-(\d+))?$/.exec(src);
  if (!m) {
    return { file: src };
  }
  const start = Number(m[2]);
  return { file: m[1], start, end: m[3] ? Number(m[3]) : start };
}
