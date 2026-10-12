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
