//! `check` — validaciones para CI (RF-130, RF-131) presentadas como una
//! lista de comprobación: qué se revisa, qué está bien, qué falta y cómo
//! arreglarlo.
//!
//! El código de salida no cambia respecto de la versión anterior (CI
//! depende de él): falla con errores de validación de las notas, con
//! advertencias de validación si se usa `--strict`, o si `--site` no compila.
//! Las comprobaciones de avance (inventario, tareas, overview, AGENTS.md,
//! sitio, OpenSpec) son informativas: el estado del proyecto vive fuera del
//! repositorio y en CI no existe.

use anyhow::Result;
use archeology_core::context::AgentsMdState;
use archeology_core::frontmatter::{IssueKind, IssueSeverity, NoteIssue};
use archeology_core::validate::{self, CheckReport, EvidenceRepair};
use archeology_core::{t, Project};
use clap::Args;
use serde::Serialize;
use std::fs;

use crate::ui;

#[derive(Args)]
pub struct CheckArgs {
    /// Las advertencias también fallan.
    #[arg(long)]
    strict: bool,
    /// Verifica además que el sitio VitePress compila (RF-134).
    #[arg(long)]
    site: bool,
    /// Con --site, aprueba ejecutar la configuración de VitePress del proyecto
    /// (JavaScript del repositorio) sin preguntar.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Ajusta las evidencias cuyo rango termina después del final del archivo.
    #[arg(long)]
    fix: bool,
    /// Formato de salida.
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    format: String,
}

/// Resultado de una comprobación.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warn,
    Error,
    /// Opcional o no aplica.
    Skip,
}

/// Una línea de la lista de comprobación (contrato JSON estable).
#[derive(Serialize)]
pub struct CheckItem {
    /// Identificador estable: `config`, `inventory`, `tasks`, `overview`,
    /// `frontmatter`, `evidence`, `wikilinks`, `duplicates`, `stale`,
    /// `agents_md`, `site`, `openspec`.
    pub id: &'static str,
    /// Grupo: `setup`, `knowledge` u `outputs`.
    pub group: &'static str,
    pub status: CheckStatus,
    /// Elementos revisados (notas, evidencias, enlaces…), si aplica.
    pub count: Option<usize>,
    pub message: String,
    /// Cómo arreglarlo (a menudo un comando `adoc …`).
    pub hint: Option<String>,
    pub details: Vec<Detail>,
    /// Lo que `--fix` corrigió (evidencias ajustadas al final del archivo).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub adjusted: Vec<Detail>,
}

#[derive(Serialize, Clone)]
pub struct Detail {
    /// Ruta relativa a la raíz del proyecto.
    pub path: String,
    pub line: Option<usize>,
    /// `error`, `warning` o, en `adjusted`, `fixed`.
    pub severity: &'static str,
    pub message: String,
}

/// Formato anterior de `--format json` (se conserva: solo se añaden claves).
#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    errors: usize,
    warnings: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<Issue<'a>>,
    /// Lista de comprobación.
    checks: &'a [CheckItem],
    summary: Summary,
    exit_code: i32,
}

#[derive(Serialize)]
struct Issue<'a> {
    path: &'a str,
    severity: &'a str,
    message: &'a str,
    line: Option<usize>,
    /// Comprobación a la que pertenece.
    check: &'static str,
}

#[derive(Serialize, Default)]
struct Summary {
    ok: usize,
    warnings: usize,
    errors: usize,
    skipped: usize,
}

/// Detalles por comprobación que se muestran en texto.
const MAX_DETAILS: usize = 5;
/// Ancho de la columna de nombres.
const NAME_WIDTH: usize = 18;

pub fn run(project: &Project, args: CheckArgs) -> Result<i32> {
    // `--fix` primero: el informe (y el código de salida) refleja solo lo
    // que sigue mal después de ajustar.
    let report = ui::with_spinner(t!("cli.check.running"), || {
        let adjusted = if args.fix {
            validate::repair_vault(project)
        } else {
            Vec::new()
        };
        validate::run_checks(project, args.strict).map(|r| (r, adjusted))
    });
    let (report, adjusted) = report?;

    let site_result = if args.site {
        Some(
            super::site::sync(project)
                .and_then(|_| super::toolchain::run_vitepress(project, "build", args.yes)),
        )
    } else {
        None
    };

    let checks = build_checks(project, &report, &adjusted, site_result.as_ref());

    let mut fail = !report.errors.is_empty() || (args.strict && !report.warnings.is_empty());
    if matches!(site_result, Some(Err(_))) {
        fail = true;
    }
    let code = if fail { 1 } else { 0 };

    let mut summary = Summary::default();
    for c in &checks {
        match c.status {
            CheckStatus::Ok => summary.ok += 1,
            CheckStatus::Warn => summary.warnings += 1,
            CheckStatus::Error => summary.errors += 1,
            CheckStatus::Skip => summary.skipped += 1,
        }
    }

    if args.format == "json" {
        let mut issues = Vec::new();
        for (list, severity) in [(&report.errors, "error"), (&report.warnings, "warning")] {
            for i in list {
                issues.push(Issue {
                    path: &i.path,
                    severity,
                    message: &i.message,
                    line: i.line,
                    check: check_id(i.kind),
                });
            }
        }
        let out = Report {
            ok: report.is_ok() && (!args.strict || report.warnings.is_empty()),
            errors: report.errors.len(),
            warnings: report.warnings.len(),
            issues,
            checks: &checks,
            summary,
            exit_code: code,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        print!(
            "{}",
            render(project, &checks, &summary, args.strict, &report)
        );
    }
    Ok(code)
}

/// Comprobación a la que pertenece cada tipo de problema.
fn check_id(kind: IssueKind) -> &'static str {
    match kind {
        IssueKind::Frontmatter | IssueKind::Unreadable | IssueKind::Output => "frontmatter",
        IssueKind::Evidence => "evidence",
        IssueKind::Wikilink => "wikilinks",
        IssueKind::Duplicate => "duplicates",
        IssueKind::Stale => "stale",
    }
}

/// Problemas de `report` de los tipos dados, como detalles con ruta relativa.
fn details(project: &Project, report: &CheckReport, kinds: &[IssueKind]) -> Vec<Detail> {
    let to_detail = |i: &NoteIssue| Detail {
        path: project.rel(std::path::Path::new(&i.path)),
        line: i.line,
        severity: match i.severity {
            IssueSeverity::Error => "error",
            IssueSeverity::Warning => "warning",
        },
        message: i.message.clone(),
    };
    // Errores primero.
    report
        .errors
        .iter()
        .chain(report.warnings.iter())
        .filter(|i| kinds.contains(&i.kind))
        .map(to_detail)
        .collect()
}

/// Estado según los detalles: error si hay alguno de error, aviso si hay avisos.
fn status_of(details: &[Detail]) -> CheckStatus {
    if details.iter().any(|d| d.severity == "error") {
        CheckStatus::Error
    } else if details.is_empty() {
        CheckStatus::Ok
    } else {
        CheckStatus::Warn
    }
}

fn item(
    id: &'static str,
    group: &'static str,
    status: CheckStatus,
    count: Option<usize>,
    message: String,
    hint: Option<String>,
) -> CheckItem {
    CheckItem {
        id,
        group,
        status,
        count,
        message,
        hint,
        details: Vec::new(),
        adjusted: Vec::new(),
    }
}

/// Texto con la cantidad: clave `<key>_one` si es 1, `<key>` si no.
fn counted(key: &str, count: usize) -> String {
    if count == 1 {
        t!(&format!("{key}_one"))
    } else {
        t!(key, count = count)
    }
}

fn build_checks(
    project: &Project,
    report: &CheckReport,
    adjusted: &[EvidenceRepair],
    site: Option<&Result<()>>,
) -> Vec<CheckItem> {
    let mut out = Vec::new();

    // --- Configuración y estado -------------------------------------------
    // Si el proyecto se abrió, la configuración se leyó y validó.
    out.push(item(
        "config",
        "setup",
        CheckStatus::Ok,
        None,
        t!(
            "cli.check.config_ok",
            file = archeology_core::config::CONFIG_FILE
        ),
        None,
    ));

    let inv: Option<archeology_core::model::Inventory> =
        fs::read_to_string(project.inventory_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
    out.push(match &inv {
        Some(i) => {
            let age = ui::seconds_since_rfc3339(&i.generated_at)
                .map(ui::age)
                .unwrap_or_else(|| "?".into());
            item(
                "inventory",
                "setup",
                CheckStatus::Ok,
                Some(i.total_files),
                t!("cli.check.inventory_ok", files = i.total_files, age = age),
                None,
            )
        }
        None => item(
            "inventory",
            "setup",
            CheckStatus::Warn,
            None,
            t!("cli.check.inventory_missing"),
            Some("adoc scan".into()),
        ),
    });

    let (tasks_total, tasks_done) = task_counts(project);
    out.push(if tasks_total > 0 {
        item(
            "tasks",
            "setup",
            CheckStatus::Ok,
            Some(tasks_total),
            t!("cli.check.tasks_ok", total = tasks_total, done = tasks_done),
            None,
        )
    } else if inv.is_none() {
        item(
            "tasks",
            "setup",
            CheckStatus::Skip,
            Some(0),
            t!("cli.check.tasks_need_scan"),
            None,
        )
    } else {
        item(
            "tasks",
            "setup",
            CheckStatus::Warn,
            Some(0),
            t!("cli.check.tasks_missing"),
            Some("adoc plan".into()),
        )
    });

    // --- Conocimiento --------------------------------------------------------
    let overview = project.knowledge_dir().join("00-overview.md");
    out.push(if !overview.is_file() {
        item(
            "overview",
            "knowledge",
            CheckStatus::Warn,
            None,
            t!("cli.check.overview_missing"),
            Some("adoc scan".into()),
        )
    } else if super::status::overview_excerpt(project).is_none() {
        item(
            "overview",
            "knowledge",
            CheckStatus::Warn,
            None,
            t!("cli.check.overview_template"),
            Some("adoc inspect".into()),
        )
    } else {
        item(
            "overview",
            "knowledge",
            CheckStatus::Ok,
            None,
            t!("cli.check.overview_ok"),
            None,
        )
    });

    // Frontmatter.
    let d = details(
        project,
        report,
        &[
            IssueKind::Frontmatter,
            IssueKind::Unreadable,
            IssueKind::Output,
        ],
    );
    let notes = report.notes_checked;
    let st = status_of(&d);
    let mut it = item(
        "frontmatter",
        "knowledge",
        st,
        Some(notes),
        if st == CheckStatus::Ok {
            t!("cli.check.frontmatter_ok", count = notes)
        } else {
            t!(
                "cli.check.frontmatter_bad",
                count = notes,
                bad = distinct_paths(&d)
            )
        },
        (st != CheckStatus::Ok).then(|| t!("cli.check.hint.frontmatter")),
    );
    it.details = d;
    out.push(it);

    // Evidencias.
    let d = details(project, report, &[IssueKind::Evidence]);
    let sources = report.sources_checked;
    let st = status_of(&d);
    let fixable = report.repairable.len();
    let mut message = if st == CheckStatus::Ok {
        t!("cli.check.evidence_ok", count = sources)
    } else {
        t!("cli.check.evidence_bad", count = sources, bad = d.len())
    };
    if fixable > 0 {
        message.push_str(&format!(" · {}", counted("cli.check.fix.fixable", fixable)));
    }
    if !adjusted.is_empty() {
        message.push_str(&format!(
            " · {}",
            counted("cli.check.fix.adjusted", adjusted.len())
        ));
    }
    // Rangos que solo se pasan del final del archivo: los ajusta `--fix`.
    let hint = if fixable > 0 {
        Some("adoc check --fix".to_string())
    } else {
        (st != CheckStatus::Ok).then(|| t!("cli.check.hint.evidence"))
    };
    let mut it = item("evidence", "knowledge", st, Some(sources), message, hint);
    it.details = d;
    it.adjusted = adjusted
        .iter()
        .map(|r| Detail {
            path: r.note.clone(),
            line: None,
            severity: "fixed",
            message: r.message(),
        })
        .collect();
    out.push(it);

    // Wikilinks.
    let d = details(project, report, &[IssueKind::Wikilink]);
    let links = report.wikilinks_checked;
    let st = status_of(&d);
    let mut it = item(
        "wikilinks",
        "knowledge",
        st,
        Some(links),
        if st == CheckStatus::Ok {
            t!("cli.check.wikilinks_ok", count = links)
        } else {
            t!("cli.check.wikilinks_bad", count = links, broken = d.len())
        },
        (st != CheckStatus::Ok).then(|| t!("cli.check.hint.wikilinks")),
    );
    it.details = d;
    out.push(it);

    // IDs repetidos y hallazgos equivalentes.
    let d = details(project, report, &[IssueKind::Duplicate]);
    let st = status_of(&d);
    let mut it = item(
        "duplicates",
        "knowledge",
        st,
        Some(d.len()),
        if st == CheckStatus::Ok {
            t!("cli.check.duplicates_ok")
        } else {
            t!("cli.check.duplicates_bad", count = d.len())
        },
        (st != CheckStatus::Ok).then(|| t!("cli.check.hint.duplicates")),
    );
    it.details = d;
    out.push(it);

    // Notas desactualizadas (marcadas por `sync`).
    let d = details(project, report, &[IssueKind::Stale]);
    let st = status_of(&d);
    let mut it = item(
        "stale",
        "knowledge",
        st,
        Some(d.len()),
        if st == CheckStatus::Ok {
            t!("cli.check.stale_ok")
        } else {
            t!("cli.check.stale_bad", count = d.len())
        },
        (st != CheckStatus::Ok).then(|| "adoc inspect".to_string()),
    );
    it.details = d;
    out.push(it);

    // --- Contexto y sitio ----------------------------------------------------
    out.push(
        match archeology_core::context::agents_md_state(project, inv.as_ref()) {
            Ok(AgentsMdState::UpToDate) => item(
                "agents_md",
                "outputs",
                CheckStatus::Ok,
                None,
                t!("cli.check.agents_ok"),
                None,
            ),
            Ok(AgentsMdState::Outdated) => item(
                "agents_md",
                "outputs",
                CheckStatus::Warn,
                None,
                t!("cli.check.agents_outdated"),
                Some("adoc scan".into()),
            ),
            Ok(AgentsMdState::NoRegion) => item(
                "agents_md",
                "outputs",
                CheckStatus::Warn,
                None,
                t!("cli.check.agents_no_region"),
                Some("adoc scan".into()),
            ),
            Ok(AgentsMdState::Missing) => item(
                "agents_md",
                "outputs",
                CheckStatus::Warn,
                None,
                t!("cli.check.agents_missing"),
                Some("adoc scan".into()),
            ),
            Err(e) => item(
                "agents_md",
                "outputs",
                CheckStatus::Warn,
                None,
                e.to_string(),
                None,
            ),
        },
    );

    out.push(site_check(project, site));
    out.push(openspec_check(project));
    out
}

/// Archivos distintos entre los detalles.
fn distinct_paths(details: &[Detail]) -> usize {
    let mut paths: Vec<&str> = details.iter().map(|d| d.path.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    paths.len()
}

/// (total, hechas) de las tareas del plan.
fn task_counts(project: &Project) -> (usize, usize) {
    let Ok(entries) = fs::read_dir(project.tasks_dir()) else {
        return (0, 0);
    };
    let metas: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            fs::read_to_string(e.path())
                .ok()
                .and_then(|t| archeology_core::planner::parse_task(&t))
        })
        .collect();
    let done = metas.iter().filter(|m| m.status == "done").count();
    (metas.len(), done)
}

fn site_check(project: &Project, site: Option<&Result<()>>) -> CheckItem {
    let llms = project.root.join("llms.txt").is_file();
    let llms_text = if llms {
        " · llms.txt ✓".to_string()
    } else {
        String::new()
    };
    if let Some(result) = site {
        let mut it = match result {
            Ok(()) => item(
                "site",
                "outputs",
                CheckStatus::Ok,
                None,
                t!("cli.check.site_built"),
                None,
            ),
            Err(_) => item(
                "site",
                "outputs",
                CheckStatus::Error,
                None,
                t!("cli.check.site_build_failed_short"),
                Some(t!("cli.check.hint.site_build")),
            ),
        };
        if let Err(e) = result {
            it.details.push(Detail {
                path: format!("{}/", project.config.paths.docs.trim_end_matches('/')),
                line: None,
                severity: "error",
                message: e.to_string(),
            });
        }
        return it;
    }
    let vp = project.docs_dir().join(".vitepress");
    let sidebar = vp.join(archeology_core::site::SIDEBAR_FILE);
    if sidebar.is_file() {
        let age = ui::seconds_since_modified(&sidebar)
            .map(ui::age)
            .unwrap_or_else(|| "?".into());
        item(
            "site",
            "outputs",
            CheckStatus::Ok,
            None,
            format!("{}{llms_text}", t!("cli.check.site_ok", age = age)),
            None,
        )
    } else if vp.is_dir() {
        item(
            "site",
            "outputs",
            CheckStatus::Skip,
            None,
            t!("cli.check.site_not_synced"),
            Some("adoc site sync".into()),
        )
    } else {
        item(
            "site",
            "outputs",
            CheckStatus::Skip,
            None,
            t!("cli.check.site_none"),
            None,
        )
    }
}

fn openspec_check(project: &Project) -> CheckItem {
    let skip = || {
        item(
            "openspec",
            "outputs",
            CheckStatus::Skip,
            None,
            t!("cli.check.openspec_none"),
            None,
        )
    };
    if !project.config.openspec_enabled(&project.root) {
        return skip();
    }
    let spec = match super::specs::load_spec(project) {
        Ok(Some(s)) => s,
        Ok(None) => return skip(),
        Err(e) => {
            return item(
                "openspec",
                "outputs",
                CheckStatus::Warn,
                None,
                e.to_string(),
                None,
            )
        }
    };
    let reqs: usize = spec.capabilities.iter().map(|c| c.requirements.len()).sum();
    let stale_days = project.config.openspec.stale_change_days;
    let mut d = Vec::new();
    for ch in &spec.active_changes {
        let rel = project.rel(&ch.path);
        for msg in archeology_openspec::validate_change_structure(&ch.path) {
            d.push(Detail {
                path: rel.clone(),
                line: None,
                severity: "warning",
                message: msg,
            });
        }
        if ch.is_stale(stale_days) {
            d.push(Detail {
                path: rel.clone(),
                line: None,
                severity: "warning",
                message: t!(
                    "cli.check.openspec_stale_change",
                    days = ch.days_since_update.unwrap_or_default()
                ),
            });
        }
    }
    let st = status_of(&d);
    let mut it = item(
        "openspec",
        "outputs",
        st,
        Some(reqs),
        t!(
            "cli.check.openspec_ok",
            specs = spec.capabilities.len(),
            requirements = reqs,
            changes = spec.active_changes.len()
        ),
        (st != CheckStatus::Ok).then(|| t!("cli.check.hint.openspec")),
    );
    it.details = d;
    it
}

// ---------------------------------------------------------------------------
// Texto
// ---------------------------------------------------------------------------

fn mark(status: CheckStatus) -> String {
    match status {
        CheckStatus::Ok => ui::green("✓"),
        CheckStatus::Warn => ui::yellow("!"),
        CheckStatus::Error => ui::red("✗"),
        CheckStatus::Skip => ui::dim("–"),
    }
}

fn render(
    project: &Project,
    checks: &[CheckItem],
    summary: &Summary,
    strict: bool,
    report: &CheckReport,
) -> String {
    let mut out = format!(
        "{} {}\n",
        ui::bold(&t!("cli.check.title")),
        ui::dim(&format!("· {}", project.config.project.name))
    );
    let mut group = "";
    for c in checks {
        if c.group != group {
            group = c.group;
            out.push('\n');
            out.push_str(&ui::bold(&t!(&format!("cli.check.group.{group}"))));
            out.push('\n');
        }
        let name = t!(&format!("cli.check.name.{}", c.id));
        let message = match c.status {
            CheckStatus::Skip => ui::dim(&c.message),
            _ => c.message.clone(),
        };
        out.push_str(&format!(
            "  {} {} {message}\n",
            mark(c.status),
            ui::pad(&name, NAME_WIDTH)
        ));
        let indent = " ".repeat(NAME_WIDTH + 5);
        // Lo que corrigió `--fix`, antes de lo que sigue pendiente.
        for d in c.adjusted.iter().take(MAX_DETAILS) {
            out.push_str(&format!(
                "{indent}{} {}\n",
                ui::dim(&format!("{} —", d.path)),
                ui::green(&d.message)
            ));
        }
        if c.adjusted.len() > MAX_DETAILS {
            out.push_str(&format!(
                "{indent}{}\n",
                ui::dim(&t!(
                    "cli.check.more",
                    count = c.adjusted.len() - MAX_DETAILS
                ))
            ));
        }
        for d in c.details.iter().take(MAX_DETAILS) {
            let loc = match d.line {
                Some(l) => format!("{}:{l}", d.path),
                None => d.path.clone(),
            };
            out.push_str(&format!(
                "{indent}{} {}\n",
                ui::dim(&format!("{loc} —")),
                d.message
            ));
        }
        if c.details.len() > MAX_DETAILS {
            out.push_str(&format!(
                "{indent}{}\n",
                ui::dim(&t!("cli.check.more", count = c.details.len() - MAX_DETAILS))
            ));
        }
        if let Some(h) = &c.hint {
            let hint = if h.starts_with("adoc ") {
                ui::cyan(h)
            } else {
                h.clone()
            };
            out.push_str(&format!("{indent}→ {hint}\n"));
        }
    }

    let mut parts = vec![
        ui::green(&t!("cli.check.sum_ok", count = summary.ok)),
        if summary.warnings > 0 {
            ui::yellow(&t!("cli.check.sum_warnings", count = summary.warnings))
        } else {
            t!("cli.check.sum_warnings", count = summary.warnings)
        },
        if summary.errors > 0 {
            ui::red(&t!("cli.check.sum_errors", count = summary.errors))
        } else {
            t!("cli.check.sum_errors", count = summary.errors)
        },
    ];
    if summary.skipped > 0 {
        parts.push(ui::dim(&t!(
            "cli.check.sum_skipped",
            count = summary.skipped
        )));
    }
    out.push('\n');
    out.push_str(&parts.join(&ui::dim(" · ")));
    out.push('\n');
    if strict && !report.warnings.is_empty() {
        out.push_str(&ui::dim(&t!("cli.check.strict_note")));
        out.push('\n');
    }
    out
}
