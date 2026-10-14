//! Salida de texto de `adoc status`: una foto del proyecto que cabe en una
//! pantalla (proyecto, estructura generada, progreso, tareas pendientes,
//! atención, agente y el siguiente paso).
//!
//! Solo usa datos ya calculados (inventario, estado, tareas, vault) más dos
//! o tres llamadas rápidas a `git`: nada costoso.

use super::status::{InventorySummary, Status, TaskInfo};
use crate::ui;
use archeology_core::i18n::{has_key, ui_lang};
use archeology_core::planner::NON_CODE_LANGUAGES;
use archeology_core::{t, Project};
use std::collections::BTreeMap;
use std::path::Path;

/// Ancho de la columna de etiquetas.
const LABEL_WIDTH: usize = 13;
/// Tareas pendientes que se listan.
const MAX_PENDING: usize = 8;
const MAX_LANGUAGES: usize = 4;
const MAX_MODULES: usize = 5;
/// Celdas de la barra de progreso.
const BAR_WIDTH: usize = 24;
/// Orden de las severidades de hallazgos.
const SEVERITIES: &[&str] = &["critical", "high", "medium", "low", "info"];

/// Texto completo de `adoc status` (termina en salto de línea).
pub fn render(project: &Project, st: &Status) -> String {
    let mut out = String::new();
    header(&mut out, project, st);
    project_section(&mut out, project, st);
    structure_section(&mut out, project, st);
    progress_section(&mut out, st);
    pending_section(&mut out, st);
    attention_section(&mut out, st);
    agent_section(&mut out, st);
    next_step_section(&mut out, project, st);
    out
}

/// Fila `  etiqueta   valor`.
fn row(out: &mut String, label: &str, value: &str) {
    out.push_str(&format!(
        "  {} {value}\n",
        ui::dim(&ui::pad(label, LABEL_WIDTH))
    ));
}

fn section(out: &mut String, title: &str) {
    out.push('\n');
    out.push_str(&ui::bold(title));
    out.push('\n');
}

fn sep() -> String {
    ui::dim(" · ")
}

fn header(out: &mut String, project: &Project, st: &Status) {
    let profile = match st.profile.as_str() {
        "small" | "medium" | "large" => t!(&format!("cli.plan.profile_{}", st.profile)),
        _ => t!("cli.status.profile_unknown"),
    };
    out.push_str(&format!(
        "{}{}{}{}{}\n",
        ui::bold(&st.project.name),
        sep(),
        t!("cli.status.doc_language", lang = project.doc_lang().code()),
        sep(),
        t!("cli.status.plan_profile", profile = profile),
    ));
}

// ---------------------------------------------------------------------------
// Proyecto
// ---------------------------------------------------------------------------

fn project_section(out: &mut String, project: &Project, st: &Status) {
    section(out, &t!("cli.status.section.project"));
    row(out, &t!("cli.status.label.root"), &ui::tilde(&project.root));
    let Some(inv) = &st.inventory else {
        row(
            out,
            &t!("cli.status.label.inventory"),
            &format!(
                "{} {}",
                ui::yellow(&t!("cli.status.not_scanned")),
                ui::cyan("→ adoc scan")
            ),
        );
        return;
    };
    row(out, &t!("cli.status.label.type"), &project_type(inv));
    let langs = languages(inv);
    if !langs.is_empty() {
        row(out, &t!("cli.status.label.languages"), &langs);
    }
    let loc: u64 = inv
        .languages
        .iter()
        .filter(|l| !NON_CODE_LANGUAGES.contains(&l.name.as_str()))
        .map(|l| l.lines)
        .sum();
    row(
        out,
        &t!("cli.status.label.size"),
        &[
            t!(
                "cli.status.files",
                count = ui::thousands(inv.total_files as u64)
            ),
            t!("cli.status.lines_of_code", count = ui::thousands(loc)),
            ui::bytes(inv.total_bytes),
        ]
        .join(&sep()),
    );
    if let Some(git) = git_line(&project.root, inv) {
        row(out, "Git", &git);
    }
    if !inv.modules.is_empty() {
        let mut mods: Vec<_> = inv.modules.iter().collect();
        mods.sort_by(|a, b| b.files.cmp(&a.files).then(a.name.cmp(&b.name)));
        let mut parts: Vec<String> = mods
            .iter()
            .take(MAX_MODULES)
            .map(|m| format!("{} {}", m.name, ui::dim(&format!("({})", m.files))))
            .collect();
        if mods.len() > MAX_MODULES {
            parts.push(ui::dim(&t!(
                "cli.status.more",
                count = mods.len() - MAX_MODULES
            )));
        }
        row(out, &t!("cli.status.label.modules"), &parts.join(&sep()));
    }
}

/// Tipo de proyecto a partir de manifiestos, frameworks y CI del inventario:
/// `Node.js service (TypeScript) · Express · Docker · GitHub Actions CI`.
pub fn project_type(inv: &InventorySummary) -> String {
    let tools: Vec<&str> = inv
        .manifests
        .iter()
        .filter_map(|m| {
            m.rsplit_once('(')
                .map(|(_, t)| t.trim_end_matches(')').trim())
        })
        .collect();
    let has = |tool: &str| tools.contains(&tool);
    let runtime = [
        ("npm", "Node.js"),
        ("cargo", "Rust"),
        ("python", "Python"),
        ("go", "Go"),
        ("maven", "JVM"),
        ("gradle", "JVM"),
        ("composer", "PHP"),
        ("bundler", "Ruby"),
        ("mix", "Elixir"),
        ("pub", "Dart"),
        ("spm", "Swift"),
        ("nuget", ".NET"),
    ]
    .iter()
    .find(|(tool, _)| has(tool))
    .map(|(_, r)| *r);
    // Lenguaje principal: el de más líneas entre los de código.
    let main_lang = inv
        .languages
        .iter()
        .find(|l| !NON_CODE_LANGUAGES.contains(&l.name.as_str()))
        .map(|l| l.name.as_str());
    let fw = |names: &[&str]| inv.frameworks.iter().any(|f| names.contains(&f.as_str()));
    let kind = if fw(&["Electron", "Tauri"]) {
        "desktop"
    } else if fw(&["React", "Vue", "Angular", "Svelte", "Next.js"]) {
        "web"
    } else if fw(&[
        "Express", "Fastify", "NestJS", "Axum", "Actix", "Django", "Flask", "FastAPI",
    ]) {
        "service"
    } else if fw(&["clap"]) {
        "cli"
    } else {
        "project"
    };
    let base = runtime.or(main_lang).unwrap_or("");
    let mut text = if base.is_empty() {
        t!("cli.status.kind.unknown")
    } else {
        t!(&format!("cli.status.kind.{kind}"), runtime = base)
    };
    if let (Some(_), Some(lang)) = (runtime, main_lang) {
        if !base.eq_ignore_ascii_case(lang) {
            text.push_str(&format!(" ({lang})"));
        }
    }
    let mut parts = vec![text];
    // Frameworks relevantes (sin los de test ni utilidades).
    const SKIP: &[&str] = &[
        "Jest",
        "Vitest",
        "Mocha",
        "pytest",
        "serde",
        "clap",
        "Vite",
        "VitePress",
        "NumPy",
    ];
    parts.extend(
        inv.frameworks
            .iter()
            .filter(|f| !SKIP.contains(&f.as_str()))
            .take(3)
            .cloned(),
    );
    if has("docker") {
        parts.push("Docker".into());
    }
    if let Some(ci) = ci_name(&inv.ci_files) {
        parts.push(ci);
    }
    parts.join(&sep())
}

/// Proveedor de CI según los archivos detectados.
fn ci_name(files: &[String]) -> Option<String> {
    let first = files.first()?;
    let name = if files.iter().any(|f| f.contains(".github/workflows")) {
        "GitHub Actions CI"
    } else if files.iter().any(|f| f.contains(".gitlab-ci")) {
        "GitLab CI"
    } else if files.iter().any(|f| f.contains(".circleci")) {
        "CircleCI"
    } else if files.iter().any(|f| f.contains("azure-pipelines")) {
        "Azure Pipelines"
    } else if files.iter().any(|f| f.contains("Jenkinsfile")) {
        "Jenkins"
    } else if files.iter().any(|f| f.contains("bitbucket-pipelines")) {
        "Bitbucket Pipelines"
    } else {
        return Some(format!("CI ({first})"));
    };
    Some(name.to_string())
}

/// `TypeScript 97% · SQL 3%` (por líneas, solo lenguajes de código si hay).
fn languages(inv: &InventorySummary) -> String {
    let code: Vec<_> = inv
        .languages
        .iter()
        .filter(|l| !NON_CODE_LANGUAGES.contains(&l.name.as_str()) && l.lines > 0)
        .collect();
    let rows = if code.is_empty() {
        inv.languages.iter().filter(|l| l.lines > 0).collect()
    } else {
        code
    };
    let total: u64 = rows.iter().map(|l| l.lines).sum();
    if total == 0 {
        return String::new();
    }
    let mut parts: Vec<String> = rows
        .iter()
        .take(MAX_LANGUAGES)
        .map(|l| {
            let pct = (l.lines * 100 + total / 2) / total;
            format!("{} {}", l.name, ui::dim(&format!("{}%", pct.max(1))))
        })
        .collect();
    if rows.len() > MAX_LANGUAGES {
        parts.push(ui::dim(&t!(
            "cli.status.more",
            count = rows.len() - MAX_LANGUAGES
        )));
    }
    parts.join(&sep())
}

/// Estado de git en vivo (rama, commits, último commit, cambios sin
/// confirmar); si `git` no responde, lo que guardó el último scan.
fn git_line(root: &Path, inv: &InventorySummary) -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = archeology_core::gitcmd::git_command(root)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).to_string())
    };
    let mut parts = Vec::new();
    if let Some(status) = git(&["status", "--porcelain=v1", "--branch"]) {
        let mut lines = status.lines();
        let branch = lines.next().and_then(|l| l.strip_prefix("## ")).map(|l| {
            let l = l.strip_prefix("No commits yet on ").unwrap_or(l);
            l.split("...").next().unwrap_or(l).trim().to_string()
        });
        if let Some(b) = branch {
            parts.push(b);
        }
        if let Some(n) = git(&["rev-list", "--count", "HEAD"]) {
            let n = n.trim();
            parts.push(if n == "1" {
                t!("cli.status.commit_one")
            } else {
                t!("cli.status.commits", count = n)
            });
        }
        if let Some(ts) =
            git(&["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse::<i64>().ok())
        {
            let now = chrono::Utc::now().timestamp();
            parts.push(t!("cli.status.last_commit", age = ui::age(now - ts)));
        }
        let changes = lines.filter(|l| !l.trim().is_empty()).count();
        parts.push(if changes == 0 {
            ui::green(&t!("cli.status.git_clean"))
        } else {
            ui::yellow(&t!("cli.status.git_changes", count = changes))
        });
        return Some(parts.join(&sep()));
    }
    // Sin git disponible: datos del inventario.
    let g = inv.git.as_ref()?;
    parts.push(t!("cli.status.commits", count = g.commits_analyzed));
    if let Some(secs) = g
        .last_commit_date
        .as_deref()
        .and_then(ui::seconds_since_rfc3339)
    {
        parts.push(t!("cli.status.last_commit", age = ui::age(secs)));
    }
    Some(parts.join(&sep()))
}

// ---------------------------------------------------------------------------
// Estructura generada
// ---------------------------------------------------------------------------

/// Marca de presencia de un archivo de contexto: ✓ con región gestionada,
/// `!` sin ella, ✗ si no existe.
fn file_mark(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) if !archeology_core::managed::region_names(&text).is_empty() => ui::green("✓"),
        Ok(_) => ui::yellow("!"),
        Err(_) => ui::red("✗"),
    }
}

fn structure_section(out: &mut String, project: &Project, st: &Status) {
    section(out, &t!("cli.status.section.structure"));
    let know = format!("{}/", project.config.paths.knowledge.trim_end_matches('/'));
    if project.knowledge_dir().is_dir() {
        let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
        for n in st.notes.iter().filter(|n| n.kind != "moc") {
            *by_type.entry(n.kind.as_str()).or_insert(0) += 1;
        }
        let total: usize = by_type.values().sum();
        let mut types: Vec<(&str, usize)> = by_type.into_iter().collect();
        types.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut parts = vec![t!("cli.status.notes", count = total)];
        parts.extend(types.iter().map(|(kind, n)| {
            let key = format!("cli.status.note_type.{kind}");
            let label = if has_key(ui_lang(), &key) {
                t!(&key)
            } else {
                kind.to_string()
            };
            format!("{label} {n}")
        }));
        if !st.stale_notes.is_empty() {
            parts.push(ui::yellow(&t!(
                "cli.status.stale",
                count = st.stale_notes.len()
            )));
        }
        row(out, &know, &parts.join(&sep()));
    } else {
        row(
            out,
            &know,
            &format!(
                "{} {}",
                ui::red(&t!("cli.status.missing")),
                ui::cyan("→ adoc init")
            ),
        );
    }

    let docs = project.docs_dir();
    let docs_label = format!("{}/", project.config.paths.docs.trim_end_matches('/'));
    if docs.join(".vitepress").is_dir() {
        let sidebar = docs
            .join(".vitepress")
            .join(archeology_core::site::SIDEBAR_FILE);
        let value = match ui::seconds_since_modified(&sidebar) {
            Some(secs) if sidebar.is_file() => {
                t!("cli.status.site_synced", age = ui::age(secs))
            }
            _ => format!(
                "{} {}",
                ui::yellow(&t!("cli.status.site_not_synced")),
                ui::cyan("→ adoc site sync")
            ),
        };
        row(out, &docs_label, &value);
    } else if docs.is_dir() {
        row(out, &docs_label, &ui::dim(&t!("cli.status.docs_no_site")));
    }

    let mut ctx = vec![format!(
        "AGENTS.md {}",
        file_mark(&project.root.join("AGENTS.md"))
    )];
    let claude = project.root.join("CLAUDE.md");
    if claude.is_file()
        || project
            .config
            .context
            .adapters
            .iter()
            .any(|a| a == "claude")
    {
        ctx.push(format!("CLAUDE.md {}", file_mark(&claude)));
    }
    let llms = project.root.join("llms.txt");
    ctx.push(format!(
        "llms.txt {}",
        if llms.is_file() {
            ui::green("✓")
        } else {
            ui::dim("–")
        }
    ));
    row(out, &t!("cli.status.label.context"), &ctx.join(&sep()));

    if project.config.openspec_enabled(&project.root) {
        if let Ok(Some(spec)) = super::specs::load_spec(project) {
            let reqs: usize = spec.capabilities.iter().map(|c| c.requirements.len()).sum();
            row(
                out,
                "OpenSpec",
                &[
                    t!("cli.status.specs", count = spec.capabilities.len()),
                    t!("cli.status.requirements", count = reqs),
                    t!("cli.status.changes", count = spec.active_changes.len()),
                ]
                .join(&sep()),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Progreso y tareas
// ---------------------------------------------------------------------------

fn count_status(st: &Status, status: &str) -> usize {
    st.tasks.iter().filter(|t| t.status == status).count()
}

fn progress_section(out: &mut String, st: &Status) {
    if st.tasks.is_empty() {
        return;
    }
    let done = count_status(st, "done");
    let mut text = format!(
        "{}  {}",
        ui::bar(done, st.tasks.len(), BAR_WIDTH),
        t!(
            "cli.status.progress",
            done = done,
            total = st.tasks.len(),
            pct = st.progress
        )
    );
    let failed = count_status(st, "failed");
    if failed > 0 {
        text.push_str(&sep());
        text.push_str(&ui::red(&t!("cli.status.failed", count = failed)));
    }
    out.push('\n');
    out.push_str(&format!(
        "{} {text}\n",
        ui::bold(&ui::pad(
            &t!("cli.status.section.progress"),
            LABEL_WIDTH + 2
        ))
    ));
}

/// Estado de una tarea con color.
fn status_label(status: &str, width: usize) -> String {
    let padded = ui::pad(status, width);
    match status {
        "failed" => ui::red(&padded),
        "question" => ui::yellow(&padded),
        "running" => ui::cyan(&padded),
        "pending" => padded,
        _ => ui::dim(&padded),
    }
}

fn pending_section(out: &mut String, st: &Status) {
    let pending: Vec<&TaskInfo> = st.tasks.iter().filter(|t| t.status != "done").collect();
    if pending.is_empty() {
        return;
    }
    section(
        out,
        &t!("cli.status.section.pending", count = pending.len()),
    );
    let status_of: BTreeMap<&str, &str> = st
        .tasks
        .iter()
        .map(|t| (t.id.as_str(), t.status.as_str()))
        .collect();
    let shown = &pending[..pending.len().min(MAX_PENDING)];
    let what = |t: &TaskInfo| match &t.module {
        Some(m) => format!("{} · {m}", t.kind),
        None => t.kind.clone(),
    };
    let id_w = shown
        .iter()
        .map(|t| t.id.chars().count())
        .max()
        .unwrap_or(0);
    let what_w = shown
        .iter()
        .map(|t| what(t).chars().count().min(30))
        .max()
        .unwrap_or(0);
    let status_w = shown
        .iter()
        .map(|t| t.status.chars().count())
        .max()
        .unwrap_or(0);
    for task in shown {
        let blocked: Vec<&str> = task
            .depends_on
            .iter()
            .filter(|d| status_of.get(d.as_str()).is_some_and(|s| *s != "done"))
            .map(String::as_str)
            .collect();
        let mut line = format!(
            "  {}  {}  {}",
            ui::bold(&ui::pad(&task.id, id_w)),
            ui::pad(&ui::truncate(&what(task), 30), what_w),
            status_label(&task.status, status_w)
        );
        if !blocked.is_empty() {
            line.push_str(&format!(
                "  {}",
                ui::dim(&t!("cli.status.blocked_by", ids = blocked.join(", ")))
            ));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    if pending.len() > MAX_PENDING {
        out.push_str(&format!(
            "  {} {}\n",
            ui::dim(&t!("cli.status.more", count = pending.len() - MAX_PENDING)),
            ui::cyan("→ adoc task list")
        ));
    }
}

// ---------------------------------------------------------------------------
// Atención: preguntas y hallazgos
// ---------------------------------------------------------------------------

fn attention_section(out: &mut String, st: &Status) {
    let agent_q = &st.questions.agent;
    let team_q = &st.questions.team;
    let open_findings: usize = st.severity_totals.values().sum();
    if agent_q.is_empty() && team_q.is_empty() && open_findings == 0 {
        return;
    }
    section(out, &t!("cli.status.section.attention"));
    let total_q = agent_q.len() + team_q.len();
    if total_q > 0 {
        let first = match agent_q.first() {
            Some(q) => format!("{}: “{}”", q.task, ui::truncate(q.question.trim(), 60)),
            None => format!("“{}”", ui::truncate(team_q[0].trim(), 64)),
        };
        row(
            out,
            &t!("cli.status.label.questions"),
            &format!(
                "{}{}{}",
                ui::yellow(&t!("cli.status.questions_open", count = total_q)),
                sep(),
                first
            ),
        );
    }
    if open_findings > 0 {
        let mut sevs: Vec<(&String, &usize)> = st.severity_totals.iter().collect();
        let rank = |s: &str| SEVERITIES.iter().position(|x| *x == s).unwrap_or(99);
        sevs.sort_by_key(|(s, _)| rank(s));
        let parts: Vec<String> = sevs
            .iter()
            .map(|(s, n)| {
                let text = format!("{s} {n}");
                match s.as_str() {
                    "critical" | "high" => ui::red(&text),
                    "medium" => ui::yellow(&text),
                    _ => text,
                }
            })
            .collect();
        row(
            out,
            &t!("cli.status.label.findings"),
            &format!(
                "{}: {}",
                t!("cli.status.findings_open", count = open_findings),
                parts.join(&sep())
            ),
        );
    }
}

fn agent_section(out: &mut String, st: &Status) {
    let model = st
        .agent
        .model
        .as_deref()
        .map(|m| t!("cli.status.model", model = m))
        .unwrap_or_else(|| t!("cli.status.default_model"));
    out.push('\n');
    out.push_str(&format!(
        "{} {}{}{}{}{}\n",
        ui::bold(&ui::pad(&t!("cli.status.section.agent"), LABEL_WIDTH + 2)),
        ui::bold(&st.agent.active),
        sep(),
        st.agent.mode,
        sep(),
        model
    ));
}

// ---------------------------------------------------------------------------
// Siguiente paso
// ---------------------------------------------------------------------------

/// Comando sugerido y motivo según el estado del proyecto.
pub fn next_step(project: &Project, st: &Status) -> (String, String) {
    if st.inventory.is_none() {
        return ("adoc scan".into(), t!("cli.status.next.scan"));
    }
    if st.tasks.is_empty() {
        return ("adoc plan".into(), t!("cli.status.next.plan"));
    }
    if let Some(q) = st.questions.agent.first() {
        return (
            format!("adoc task answer {}", q.task),
            t!("cli.status.next.answer"),
        );
    }
    if count_status(st, "failed") > 0 {
        return (
            "adoc inspect --resume".into(),
            t!("cli.status.next.resume", count = count_status(st, "failed")),
        );
    }
    let manual = count_status(st, "manual");
    let open = st
        .tasks
        .iter()
        .filter(|t| t.status != "done" && t.status != "manual")
        .count();
    if open > 0 {
        return (
            "adoc inspect".into(),
            t!("cli.status.next.inspect", count = open),
        );
    }
    if manual > 0 {
        return (
            "adoc task list".into(),
            t!("cli.status.next.manual", count = manual),
        );
    }
    if !st.stale_notes.is_empty() {
        return (
            "adoc sync".into(),
            t!("cli.status.next.sync", count = st.stale_notes.len()),
        );
    }
    // Todo hecho: sitio desactualizado respecto de la última corrida → sync.
    let sidebar = project
        .docs_dir()
        .join(".vitepress")
        .join(archeology_core::site::SIDEBAR_FILE);
    if project.docs_dir().join(".vitepress").is_dir() {
        let synced = ui::seconds_since_modified(&sidebar);
        let last_run = st
            .timing
            .last_run_at
            .as_deref()
            .and_then(ui::seconds_since_rfc3339);
        let outdated = match (synced, last_run) {
            (None, _) => true,
            (Some(s), Some(r)) => s > r,
            _ => false,
        };
        if outdated {
            return ("adoc site sync".into(), t!("cli.status.next.site_sync"));
        }
    }
    ("adoc preview".into(), t!("cli.status.next.preview"))
}

fn next_step_section(out: &mut String, project: &Project, st: &Status) {
    let (cmd, why) = next_step(project, st);
    out.push('\n');
    out.push_str(&format!(
        "{} → {}  {}\n",
        ui::bold(&ui::pad(&t!("cli.status.section.next"), LABEL_WIDTH + 2)),
        ui::cyan(&ui::bold(&cmd)),
        ui::dim(&why)
    ));
}
