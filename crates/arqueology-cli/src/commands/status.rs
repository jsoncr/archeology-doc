//! `status` — estado completo del proyecto en una sola llamada.
//!
//! Pensado para integraciones (extensión de VS Code): tareas, hallazgos,
//! notas, cumplimiento de specs y agente activo, con rutas absolutas.

use anyhow::Result;
use arqueology_core::frontmatter::NoteFrontmatter;
use arqueology_core::planner;
use arqueology_core::Project;
use clap::Args;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct StatusArgs {
    /// Formato de salida.
    #[arg(long, default_value = "json", value_parser = ["json", "text"])]
    format: String,
}

#[derive(Serialize)]
pub struct Status {
    pub project: ProjectInfo,
    pub agent: AgentInfo,
    pub profile: String,
    /// Porcentaje de tareas `done`.
    pub progress: usize,
    pub tasks: Vec<TaskInfo>,
    pub findings: Vec<FindingInfo>,
    pub notes: Vec<NoteInfo>,
    pub stale_notes: Vec<String>,
    pub specs: Option<serde_json::Value>,
    /// Hallazgos abiertos por severidad.
    pub severity_totals: BTreeMap<String, usize>,
    /// Funcionalidades sin reporte de tests (ids) y escenarios faltantes.
    pub features_without_tests: Vec<String>,
    pub missing_scenarios: u64,
    pub questions: Questions,
    pub timing: Timing,
    /// Datos del proyecto según el último `adoc scan` (archivos, lenguajes, git…).
    pub inventory: Option<InventorySummary>,
    /// Primeros párrafos del overview escritos por el agente (fuera de la región del inventario).
    pub overview_excerpt: Option<String>,
}

/// Preguntas del proyecto: las que esperan respuesta y las ya respondidas.
#[derive(Serialize, Default)]
pub struct Questions {
    /// Preguntas de agentes que bloquean tareas (estado `question`).
    pub agent: Vec<AgentQuestion>,
    /// Dudas que los agentes dejaron para el equipo técnico
    /// (`knowledge/memory/open-questions.md`).
    pub team: Vec<String>,
    /// Preguntas respondidas: `adoc ask` (knowledge/faq/) y `adoc task answer`.
    pub faq: Vec<FaqEntry>,
}

#[derive(Serialize)]
pub struct AgentQuestion {
    pub task: String,
    pub question: String,
}

#[derive(Serialize)]
pub struct FaqEntry {
    pub question: String,
    pub answer: String,
    /// `agente` (adoc ask) o `equipo` (respuesta a una tarea).
    pub origin: String,
    pub file: PathBuf,
}

#[derive(Serialize)]
pub struct ProjectInfo {
    pub name: String,
    pub id: String,
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub knowledge: PathBuf,
    pub has_inventory: bool,
}

#[derive(Serialize)]
pub struct AgentInfo {
    pub active: String,
    pub model: Option<String>,
    pub mode: String,
    /// Agentes configurados y predefinidos, para elegir.
    pub available: Vec<String>,
}

#[derive(Serialize)]
pub struct TaskInfo {
    pub id: String,
    pub kind: String,
    pub module: Option<String>,
    pub status: String,
    pub depends_on: Vec<String>,
    pub produce: Vec<String>,
    pub question: Option<String>,
    pub file: PathBuf,
    /// Log más reciente de la tarea, si se ejecutó.
    pub log: Option<PathBuf>,
    /// Última ejecución con agente (duración, intentos, resultado).
    pub last_run: Option<TaskRunInfo>,
}

#[derive(Serialize, Clone)]
pub struct TaskRunInfo {
    pub agent: String,
    pub status: String,
    pub duration_ms: u128,
    pub attempts: u32,
    pub exit_code: Option<i32>,
    pub reason: Option<String>,
    /// Fin de la corrida en la que se ejecutó (RFC 3339).
    pub finished_at: Option<String>,
}

/// Resumen del inventario para el dashboard.
#[derive(Serialize)]
pub struct InventorySummary {
    pub generated_at: String,
    pub total_files: usize,
    pub total_bytes: u64,
    pub total_lines: u64,
    pub code_files: usize,
    /// Por líneas, de mayor a menor.
    pub languages: Vec<LanguageRow>,
    pub frameworks: Vec<String>,
    pub test_frameworks: Vec<String>,
    pub manifests: Vec<String>,
    pub entrypoints: Vec<String>,
    pub test_files: Vec<String>,
    pub disabled_tests: Vec<String>,
    pub ci_files: Vec<String>,
    pub doc_files: Vec<String>,
    /// Marcadores por tipo (TODO, FIXME, HACK…).
    pub markers_by_kind: BTreeMap<String, usize>,
    /// Los primeros marcadores (no TODO primero: suelen ser más relevantes).
    pub markers: Vec<arqueology_core::model::Marker>,
    pub git: Option<GitSummary>,
    pub modules: Vec<arqueology_core::model::ModuleInfo>,
}

#[derive(Serialize)]
pub struct LanguageRow {
    pub name: String,
    pub files: usize,
    pub bytes: u64,
    pub lines: u64,
}

#[derive(Serialize)]
pub struct GitSummary {
    pub commits_analyzed: u32,
    pub authors: Vec<String>,
    pub last_commit_hash: Option<String>,
    pub last_commit_date: Option<String>,
    pub last_commit_subject: Option<String>,
    /// Archivos más modificados (ruta, commits), de mayor a menor.
    pub hotspots: Vec<(String, u32)>,
}

const MAX_MARKERS: usize = 40;
const MAX_HOTSPOTS: usize = 10;

fn inventory_summary(inv: &arqueology_core::model::Inventory) -> InventorySummary {
    let mut languages: Vec<LanguageRow> = inv
        .languages
        .iter()
        .map(|(name, s)| LanguageRow {
            name: name.clone(),
            files: s.files,
            bytes: s.bytes,
            lines: s.lines,
        })
        .collect();
    languages.sort_by(|a, b| b.lines.cmp(&a.lines).then(b.files.cmp(&a.files)));
    let mut markers_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for m in &inv.markers {
        *markers_by_kind.entry(m.kind.clone()).or_insert(0) += 1;
    }
    let mut markers = inv.markers.clone();
    markers.sort_by_key(|m| m.kind == "TODO");
    markers.truncate(MAX_MARKERS);
    let git = inv.git.as_ref().map(|g| {
        let mut parts = g.last_commit.as_deref().unwrap_or("").splitn(3, '|');
        let mut hotspots: Vec<(String, u32)> =
            g.hotspots.iter().map(|(f, n)| (f.clone(), *n)).collect();
        hotspots.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        hotspots.truncate(MAX_HOTSPOTS);
        GitSummary {
            commits_analyzed: g.commits_analyzed,
            authors: g.authors.clone(),
            last_commit_hash: parts
                .next()
                .filter(|s| !s.is_empty())
                .map(|h| h.chars().take(8).collect()),
            last_commit_date: parts.next().map(String::from),
            last_commit_subject: parts.next().map(String::from),
            hotspots,
        }
    });
    InventorySummary {
        generated_at: inv.generated_at.clone(),
        total_files: inv.total_files,
        total_bytes: inv.total_bytes,
        total_lines: inv.languages.values().map(|s| s.lines).sum(),
        code_files: planner::code_files(inv),
        languages,
        frameworks: inv.frameworks.clone(),
        test_frameworks: inv.test_frameworks.clone(),
        manifests: inv.manifests.clone(),
        entrypoints: inv.entrypoints.clone(),
        test_files: inv.test_files.clone(),
        disabled_tests: inv.disabled_tests.clone(),
        ci_files: inv.ci_files.clone(),
        doc_files: inv.doc_files.clone(),
        markers_by_kind,
        markers,
        git,
        modules: inv.modules.clone(),
    }
}

/// Tiempos de las ejecuciones con agente.
#[derive(Serialize, Default)]
pub struct Timing {
    /// Corridas de `inspect` registradas.
    pub runs: usize,
    /// Tiempo total de agente (todas las ejecuciones de tareas).
    pub total_agent_ms: u128,
    /// Promedio de las tareas completadas.
    pub avg_done_ms: Option<u128>,
    /// Estimación para las tareas que faltan (promedio × pendientes).
    pub estimated_remaining_ms: Option<u128>,
    pub last_run_at: Option<String>,
}

#[derive(Serialize)]
pub struct FindingInfo {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub severity: String,
    pub state: String,
    /// Evidencias `ruta:línea` tal como están en la nota.
    pub sources: Vec<String>,
    pub file: PathBuf,
}

#[derive(Serialize)]
pub struct NoteInfo {
    pub id: Option<String>,
    pub kind: String,
    pub title: String,
    pub status: Option<String>,
    pub sources: Vec<String>,
    pub file: PathBuf,
}

pub fn run(project: &Project, args: StatusArgs) -> Result<()> {
    let status = build(project)?;
    if args.format == "json" {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        println!(
            "{} · agente {} · {}% · {} tareas · {} hallazgos · {} notas",
            status.project.name,
            status.agent.active,
            status.progress,
            status.tasks.len(),
            status.findings.len(),
            status.notes.len()
        );
    }
    Ok(())
}

pub fn build(project: &Project) -> Result<Status> {
    let state = arqueology_core::state::State::load(&project.state_file())?;
    let tasks = tasks(project, &state)?;
    let done = tasks.iter().filter(|t| t.status == "done").count();

    let mut findings = Vec::new();
    let mut notes = Vec::new();
    for path in arqueology_core::index::vault_notes(project) {
        let Some(fm) = fs::read_to_string(&path)
            .ok()
            .and_then(|t| NoteFrontmatter::parse(&t))
        else {
            continue;
        };
        let kind = fm.note_type.clone().unwrap_or_else(|| "unknown".into());
        let title = fm.title.clone().unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        if kind == "finding" {
            let get =
                |k: &str, d: &str| fm.get(k).and_then(|v| v.as_str()).unwrap_or(d).to_string();
            findings.push(FindingInfo {
                id: fm.id.clone().unwrap_or_default(),
                title: title.clone(),
                kind: get("kind", ""),
                severity: get("severity", "unknown"),
                state: get("state", "open"),
                sources: fm.sources.clone(),
                file: path.clone(),
            });
        }
        notes.push(NoteInfo {
            id: fm.id.clone(),
            kind,
            title,
            status: fm.status.clone(),
            sources: fm.sources.clone(),
            file: path,
        });
    }

    let inv: Option<arqueology_core::model::Inventory> =
        fs::read_to_string(project.inventory_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
    let profile = inv
        .as_ref()
        .map(|i| {
            planner::SizeProfile::resolve(i, &project.config.plan)
                .as_str()
                .to_string()
        })
        .unwrap_or_else(|| "unknown".into());

    let specs = if project.config.openspec_enabled(&project.root) {
        super::spec::load_spec(project)
            .ok()
            .flatten()
            .map(|spec| super::spec::compliance(project, &spec))
            .and_then(|c| serde_json::to_value(c).ok())
    } else {
        None
    };

    let mut severity_totals: BTreeMap<String, usize> = BTreeMap::new();
    for f in findings
        .iter()
        .filter(|f| f.state == "open" || f.state == "confirmed")
    {
        *severity_totals.entry(f.severity.clone()).or_insert(0) += 1;
    }
    let (_, features_without_tests, missing_scenarios) = super::review::test_coverage(project);
    let mut faq = faq_notes(&notes);
    faq.extend(answered_in_tasks(&tasks));
    let questions = Questions {
        agent: tasks
            .iter()
            .filter_map(|t| {
                t.question.clone().map(|q| AgentQuestion {
                    task: t.id.clone(),
                    question: q,
                })
            })
            .collect(),
        team: team_questions(project),
        faq,
    };

    let active = project.config.agent.default.clone();
    let adapter = project.config.agent.adapters.get(&active);
    let mut available: Vec<String> = project.config.agent.adapters.keys().cloned().collect();
    for k in arqueology_core::config::builtin_adapters().keys() {
        if !available.contains(k) {
            available.push(k.clone());
        }
    }

    let timing_info = timing(&state, &tasks);
    Ok(Status {
        project: ProjectInfo {
            name: project.config.project.name.clone(),
            id: project.config.project.id.clone(),
            root: project.root.clone(),
            workspace: project.state_dir(),
            knowledge: project.knowledge_dir(),
            has_inventory: inv.is_some(),
        },
        agent: AgentInfo {
            active,
            model: adapter.and_then(|a| a.model.clone()),
            mode: adapter
                .map(|a| match a.mode {
                    arqueology_core::config::AdapterMode::Headless => "headless",
                    arqueology_core::config::AdapterMode::Export => "export",
                })
                .unwrap_or("unknown")
                .to_string(),
            available,
        },
        profile,
        progress: (done * 100).checked_div(tasks.len()).unwrap_or(0),
        tasks,
        findings,
        notes,
        stale_notes: arqueology_core::index::stale_notes(project),
        specs,
        severity_totals,
        features_without_tests,
        missing_scenarios,
        questions,
        timing: timing_info,
        inventory: inv.as_ref().map(inventory_summary),
        overview_excerpt: overview_excerpt(project),
    })
}

/// Entradas `- …` de las dudas abiertas para el equipo (sin el encabezado).
fn team_questions(project: &Project) -> Vec<String> {
    let path = project.memory_dir().join("open-questions.md");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let body = arqueology_core::frontmatter::extract_frontmatter(&text)
        .map(|(_, b)| b.to_string())
        .unwrap_or(text);
    body.lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Preguntas respondidas en la sección `# Respuestas` de las tareas.
fn answered_in_tasks(tasks: &[TaskInfo]) -> Vec<FaqEntry> {
    let mut out = Vec::new();
    for t in tasks {
        let Ok(text) = fs::read_to_string(&t.file) else {
            continue;
        };
        let mut lines = text
            .lines()
            .skip_while(|l| l.trim() != "# Respuestas")
            .skip(1);
        while let Some(line) = lines.next() {
            let Some((_, q)) = line.split_once("Pregunta: ") else {
                continue;
            };
            if let Some(a) = lines
                .next()
                .and_then(|l| l.trim().strip_prefix("Respuesta: "))
            {
                out.push(FaqEntry {
                    question: q.trim().to_string(),
                    answer: a.trim().to_string(),
                    origin: "equipo".into(),
                    file: t.file.clone(),
                });
            }
        }
    }
    out
}

/// Notas `type: faq` (de `adoc ask`): el título es la pregunta, el cuerpo la respuesta.
fn faq_notes(notes: &[NoteInfo]) -> Vec<FaqEntry> {
    notes
        .iter()
        .filter(|n| n.kind == "faq")
        .filter_map(|n| {
            let text = fs::read_to_string(&n.file).ok()?;
            let body = arqueology_core::frontmatter::extract_frontmatter(&text)
                .map(|(_, b)| b.to_string())
                .unwrap_or(text);
            // Sin el primer encabezado (repite la pregunta).
            let answer = body
                .lines()
                .skip_while(|l| l.trim().is_empty() || l.starts_with("# "))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            Some(FaqEntry {
                question: n.title.clone(),
                answer,
                origin: "agente".into(),
                file: n.file.clone(),
            })
        })
        .collect()
}

fn tasks(project: &Project, state: &arqueology_core::state::State) -> Result<Vec<TaskInfo>> {
    let mut out = Vec::new();
    let runs = last_runs(state);
    let dir = project.tasks_dir();
    if !dir.is_dir() {
        return Ok(out);
    }
    for e in fs::read_dir(&dir)?.flatten() {
        let file = e.path();
        let Some(meta) = fs::read_to_string(&file)
            .ok()
            .and_then(|t| planner::parse_task(&t))
        else {
            continue;
        };
        out.push(TaskInfo {
            last_run: runs.get(&meta.task_id).cloned(),
            question: state.questions.get(&meta.task_id).cloned(),
            log: last_log(&project.runs_dir(), &meta.task_id),
            id: meta.task_id,
            kind: meta.kind,
            module: meta.module,
            status: meta.status,
            depends_on: meta.depends_on,
            produce: meta.produce,
            file,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Última ejecución de cada tarea según el historial de corridas.
fn last_runs(state: &arqueology_core::state::State) -> BTreeMap<String, TaskRunInfo> {
    let mut out = BTreeMap::new();
    for run in &state.runs {
        for t in &run.tasks {
            out.insert(
                t.task_id.clone(),
                TaskRunInfo {
                    agent: run.agent.clone(),
                    status: t.status.clone(),
                    duration_ms: t.duration_ms,
                    attempts: t.attempts,
                    exit_code: t.exit_code,
                    reason: t.reason.clone(),
                    finished_at: run.finished_at.clone(),
                },
            );
        }
    }
    out
}

fn timing(state: &arqueology_core::state::State, tasks: &[TaskInfo]) -> Timing {
    let total_agent_ms = state
        .runs
        .iter()
        .flat_map(|r| r.tasks.iter())
        .map(|t| t.duration_ms)
        .sum();
    let done: Vec<u128> = tasks
        .iter()
        .filter(|t| t.status == "done")
        .filter_map(|t| t.last_run.as_ref().map(|r| r.duration_ms))
        .filter(|ms| *ms > 0)
        .collect();
    let avg = (!done.is_empty()).then(|| done.iter().sum::<u128>() / done.len() as u128);
    let remaining = tasks.iter().filter(|t| t.status != "done").count() as u128;
    Timing {
        runs: state.runs.iter().filter(|r| !r.tasks.is_empty()).count(),
        total_agent_ms,
        avg_done_ms: avg,
        estimated_remaining_ms: avg.map(|a| a * remaining),
        last_run_at: state
            .runs
            .iter()
            .rev()
            .find(|r| !r.tasks.is_empty())
            .and_then(|r| r.finished_at.clone()),
    }
}

/// Texto del overview escrito por el agente: sin frontmatter, sin regiones
/// gestionadas y sin el título; los primeros párrafos.
fn overview_excerpt(project: &Project) -> Option<String> {
    let text = fs::read_to_string(project.knowledge_dir().join("00-overview.md")).ok()?;
    let body = arqueology_core::frontmatter::extract_frontmatter(&text)
        .map(|(_, b)| b.to_string())
        .unwrap_or(text);
    let mut out = Vec::new();
    let mut in_region = false;
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("<!-- arqueology-doc:begin") {
            in_region = true;
            continue;
        }
        if t.starts_with("<!-- arqueology-doc:end") {
            in_region = false;
            continue;
        }
        if in_region || t.starts_with("# ") || t.starts_with('>') {
            continue;
        }
        out.push(line);
    }
    let full = out.join("\n");
    let full = full.trim();
    // Corta en el último párrafo completo antes del límite, nunca a mitad de línea.
    let limit = full
        .char_indices()
        .nth(2400)
        .map(|(i, _)| i)
        .unwrap_or(full.len());
    let excerpt = if limit < full.len() {
        let head = &full[..limit];
        head.rfind("\n\n")
            .or_else(|| head.rfind('\n'))
            .map(|i| &head[..i])
            .unwrap_or(head)
    } else {
        full
    };
    (!excerpt.trim().is_empty()).then(|| excerpt.trim().to_string())
}

/// Log más reciente de una tarea.
pub fn last_log(runs: &Path, id: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(runs)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(format!("{id}.log")).is_file())
        .collect();
    dirs.sort();
    dirs.pop().map(|d| d.join(format!("{id}.log")))
}
