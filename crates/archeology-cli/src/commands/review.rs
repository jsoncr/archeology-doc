//! `review` — resumen de estado, hallazgos y progreso (RF-132, RF-133, RF-163).

use anyhow::Result;
use archeology_core::frontmatter::NoteFrontmatter;
use archeology_core::planner;
use archeology_core::t;
use archeology_core::Project;
use clap::Args;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;

#[derive(Args)]
pub struct ReviewArgs {
    /// Filtrar hallazgos por severidad.
    #[arg(long)]
    severity: Option<String>,
    /// Formato de salida.
    #[arg(long, default_value = "text", value_parser = ["text", "json", "markdown"])]
    format: String,
}

#[derive(Serialize)]
pub struct ReviewReport {
    /// Porcentaje de tareas `done`.
    pub progress: usize,
    pub tasks: BTreeMap<String, usize>,
    findings: BTreeMap<String, usize>,
    findings_open: Vec<serde_json::Value>,
    notes: BTreeMap<String, usize>,
    stale_notes: usize,
    features: usize,
    features_without_tests: Vec<String>,
    missing_scenarios: u64,
    pub openspec: Option<serde_json::Value>,
    /// Resumen del grafo del conocimiento (solo `adoc review`; clave nueva y
    /// opcional en el JSON, las integraciones existentes la ignoran).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<archeology_core::graph::Summary>,
}

impl ReviewReport {
    /// Hallazgos con `state: open`.
    pub fn open_findings(&self) -> usize {
        self.findings_open.len()
    }
}

pub fn run(project: &Project, args: ReviewArgs) -> Result<()> {
    let mut report = build_report(project, args.severity.as_deref())?;
    // El grafo solo aquí: `projects list` usa `build_report` para cada proyecto.
    report.graph = Some(archeology_core::graph::summary(
        &archeology_core::graph::build(project),
    ));
    let site_link = site_graph_link(project);
    match args.format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&report)?),
        "markdown" => {
            print_markdown(&report);
            if let Some(g) = &report.graph {
                println!("\n{}", graph_markdown(g, site_link.as_deref()));
            }
        }
        _ => {
            print_text(&report);
            if let Some(g) = &report.graph {
                println!("\n{}", graph_text(g, site_link.as_deref()));
            }
        }
    }
    Ok(())
}

/// Ruta de la página del grafo en el sitio, si `site sync` ya la generó.
fn site_graph_link(project: &Project) -> Option<String> {
    let page = archeology_core::graph_web::SITE_PAGE;
    project
        .docs_dir()
        .join(format!("{page}.md"))
        .is_file()
        .then(|| format!("/{page}"))
}

/// Notas más conectadas: `Título (n), ...`.
fn fmt_top(g: &archeology_core::graph::Summary) -> String {
    g.top_connected
        .iter()
        .map(|c| format!("{} ({})", c.title, c.connections))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Huérfanas listadas en el texto (el resto: `…`).
const MAX_ORPHANS: usize = 10;

fn fmt_orphans(g: &archeology_core::graph::Summary) -> String {
    let mut names: Vec<String> = g
        .orphans
        .iter()
        .take(MAX_ORPHANS)
        .map(|o| o.id.clone())
        .collect();
    if g.orphans.len() > MAX_ORPHANS {
        names.push("…".into());
    }
    names.join(", ")
}

fn graph_hint(site_link: Option<&str>) -> String {
    match site_link {
        Some(link) => t!("graph.review.hint_site", link = link),
        None => t!("graph.review.hint"),
    }
}

/// Sección "Grafo" de `review` en texto (idioma de la interfaz).
pub(crate) fn graph_text(g: &archeology_core::graph::Summary, site_link: Option<&str>) -> String {
    let mut lines = vec![
        t!("graph.review.heading"),
        t!(
            "graph.review.counts",
            notes = g.notes,
            code = g.code_files,
            edges = g.edges
        ),
    ];
    if !g.edges_by_kind.is_empty() {
        lines.push(t!(
            "graph.review.edges",
            counts = archeology_core::graph_web::fmt_counts(&g.edges_by_kind)
        ));
    }
    if !g.top_connected.is_empty() {
        lines.push(t!("graph.review.top", items = fmt_top(g)));
    }
    lines.push(if g.orphans.is_empty() {
        t!("graph.review.no_orphans")
    } else {
        t!(
            "graph.review.orphans",
            count = g.orphans.len(),
            names = fmt_orphans(g)
        )
    });
    lines.push(graph_hint(site_link));
    lines.join("\n")
}

/// Sección "Grafo" de `review --format markdown` (idioma de la interfaz).
pub(crate) fn graph_markdown(
    g: &archeology_core::graph::Summary,
    site_link: Option<&str>,
) -> String {
    let mut out = format!("{}\n\n", t!("graph.review.md_heading"));
    out.push_str(&format!(
        "- {}\n",
        t!(
            "graph.review.counts",
            notes = g.notes,
            code = g.code_files,
            edges = g.edges
        )
    ));
    if !g.edges_by_kind.is_empty() {
        out.push_str(&format!(
            "- {}\n",
            t!(
                "graph.review.edges",
                counts = archeology_core::graph_web::fmt_counts(&g.edges_by_kind)
            )
        ));
    }
    if !g.top_connected.is_empty() {
        out.push_str(&format!(
            "- {}\n",
            t!("graph.review.top", items = fmt_top(g))
        ));
    }
    out.push_str(&format!(
        "- {}\n",
        if g.orphans.is_empty() {
            t!("graph.review.no_orphans")
        } else {
            t!(
                "graph.review.orphans",
                count = g.orphans.len(),
                names = fmt_orphans(g)
            )
        }
    ));
    out.push_str(&format!("\n{}", graph_hint(site_link)));
    out
}

pub fn build_report(project: &Project, severity: Option<&str>) -> Result<ReviewReport> {
    let mut tasks: BTreeMap<String, usize> = BTreeMap::new();
    let dir = project.tasks_dir();
    if dir.is_dir() {
        for e in fs::read_dir(&dir)?.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "md").unwrap_or(false) {
                if let Ok(text) = fs::read_to_string(&p) {
                    if let Some((_id, _ty, st)) = planner::task_from_markdown(&text) {
                        *tasks.entry(st).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut findings: BTreeMap<String, usize> = BTreeMap::new();
    let mut open_list = Vec::new();
    let mut notes: BTreeMap<String, usize> = BTreeMap::new();
    let findings_dir = project.knowledge_dir().join("findings");
    collect_notes(&project.knowledge_dir(), &mut notes);
    if findings_dir.is_dir() {
        for e in fs::read_dir(&findings_dir)?.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "md").unwrap_or(false) {
                let text = fs::read_to_string(&p)?;
                if let Some(fm) = NoteFrontmatter::parse(&text) {
                    let sev = fm
                        .get("severity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let st = fm
                        .get("state")
                        .and_then(|v| v.as_str())
                        .unwrap_or("open")
                        .to_string();
                    *findings.entry(format!("{sev}/{st}")).or_insert(0) += 1;
                    if st == "open" && severity.map(|s| s == sev).unwrap_or(true) {
                        let kind = fm
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        open_list.push(serde_json::json!({
                            "id": fm.id.unwrap_or_default(),
                            "title": fm.title.unwrap_or_default(),
                            "severity": sev,
                            "kind": kind,
                        }));
                    }
                }
            }
        }
    }

    let openspec = if project.config.openspec_enabled(&project.root) {
        match super::spec::load_spec(project) {
            Ok(Some(spec)) => {
                let summary = super::spec::compliance(project, &spec);
                Some(serde_json::json!({
                    "capabilities": spec.capabilities.len(),
                    "requirements": summary.total_requirements,
                    "by_status": summary.totals,
                    "by_capability": summary.by_capability,
                    "active_changes": spec.active_changes.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                    "stale_changes": summary.stale_changes,
                }))
            }
            _ => None,
        }
    } else {
        None
    };

    let (features, features_without_tests, missing_scenarios) = test_coverage(project);
    let total: usize = tasks.values().sum();
    let done = tasks.get("done").copied().unwrap_or(0);

    Ok(ReviewReport {
        progress: (done * 100).checked_div(total).unwrap_or(0),
        tasks,
        findings,
        findings_open: open_list,
        notes,
        stale_notes: archeology_core::index::stale_notes(project).len(),
        features,
        features_without_tests,
        missing_scenarios,
        openspec,
        graph: None,
    })
}

/// Funcionalidades sin `test-report` y escenarios faltantes (RF-090, RF-091).
pub(crate) fn test_coverage(project: &Project) -> (usize, Vec<String>, u64) {
    let mut feature_ids = Vec::new();
    let mut covered = std::collections::BTreeSet::new();
    let mut missing = 0u64;
    for note in archeology_core::index::vault_notes(project) {
        let Some(fm) = fs::read_to_string(&note)
            .ok()
            .and_then(|t| NoteFrontmatter::parse(&t))
        else {
            continue;
        };
        match fm.note_type.as_deref() {
            Some("feature") => feature_ids.extend(fm.id.clone()),
            Some("test-report") => {
                if let Some(f) = fm.get("feature").and_then(|v| v.as_str()) {
                    covered.insert(f.to_string());
                }
                missing += fm
                    .get("missing_scenarios")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
            _ => {}
        }
    }
    let without: Vec<String> = feature_ids
        .iter()
        .filter(|f| !covered.contains(*f))
        .cloned()
        .collect();
    (feature_ids.len(), without, missing)
}

fn collect_notes(dir: &std::path::Path, out: &mut BTreeMap<String, usize>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            // Sin seguir symlinks: evita ciclos y leer fuera del árbol.
            let kind = e.file_type().ok();
            if kind.is_some_and(|t| t.is_dir()) {
                if p.file_name().map(|n| n == "_templates").unwrap_or(false) {
                    continue;
                }
                collect_notes(&p, out);
            } else if kind.is_some_and(|t| t.is_file())
                && p.extension().map(|x| x == "md").unwrap_or(false)
            {
                if let Ok(text) = fs::read_to_string(&p) {
                    if let Some(fm) = NoteFrontmatter::parse(&text) {
                        *out.entry(fm.note_type.unwrap_or_else(|| "unknown".into()))
                            .or_insert(0) += 1;
                    }
                }
            }
        }
    }
}

fn print_text(r: &ReviewReport) {
    println!("{}", t!("cli.review.heading"));
    println!("{}", t!("cli.review.progress", value = r.progress));
    println!("{}", t!("cli.review.tasks", value = fmt_map(&r.tasks)));
    println!("{}", t!("cli.review.notes", value = fmt_map(&r.notes)));
    println!(
        "{}",
        t!("cli.review.findings", value = fmt_map(&r.findings))
    );
    if r.stale_notes > 0 {
        println!("{}", t!("cli.review.stale_notes", count = r.stale_notes));
    }
    if r.features > 0 {
        println!(
            "{}",
            t!(
                "cli.review.features",
                total = r.features,
                without = r.features_without_tests.len(),
                missing = r.missing_scenarios
            )
        );
    }
    if let Some(os) = &r.openspec {
        println!(
            "{}",
            t!(
                "cli.review.openspec",
                capabilities = os["capabilities"],
                requirements = os["requirements"],
                counts = fmt_json_counts(&os["by_status"])
            )
        );
        if let Some(stale) = os["stale_changes"].as_array().filter(|a| !a.is_empty()) {
            let names: Vec<&str> = stale.iter().filter_map(|v| v.as_str()).collect();
            println!(
                "{}",
                t!("cli.review.stale_changes", names = names.join(", "))
            );
        }
    }
    if !r.features_without_tests.is_empty() {
        println!("\n{}", t!("cli.review.features_without_tests_heading"));
        for f in &r.features_without_tests {
            println!("  - {f}");
        }
    }
    if !r.findings_open.is_empty() {
        println!("\n{}", t!("cli.review.open_findings_heading"));
        for f in &r.findings_open {
            println!(
                "  [{}] {} — {}",
                f["severity"].as_str().unwrap_or("?"),
                f["id"].as_str().unwrap_or("?"),
                f["title"].as_str().unwrap_or("")
            );
        }
    }
}

fn fmt_json_counts(v: &serde_json::Value) -> String {
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("  ")
        })
        .unwrap_or_default()
}

fn print_markdown(r: &ReviewReport) {
    println!("{}\n", t!("cli.review.md_title"));
    println!("{}\n", t!("cli.review.md_progress", progress = r.progress));
    if let Some(os) = &r.openspec {
        println!("{}\n", t!("cli.review.md_specs"));
        println!("{}\n", fmt_json_counts(&os["by_status"]));
    }
    if !r.features_without_tests.is_empty() {
        println!("{}\n", t!("cli.review.md_features_without_tests"));
        for f in &r.features_without_tests {
            println!("- {f}");
        }
        println!();
    }
    println!("{}\n", t!("cli.review.md_tasks"));
    for (k, v) in &r.tasks {
        println!("- {k}: {v}");
    }
    println!("\n{}\n", t!("cli.review.md_findings"));
    for (k, v) in &r.findings {
        println!("- {k}: {v}");
    }
    if !r.findings_open.is_empty() {
        println!("\n{}\n", t!("cli.review.md_open"));
        for f in &r.findings_open {
            println!(
                "- **{}** {} ({})",
                f["severity"].as_str().unwrap_or("?"),
                f["title"].as_str().unwrap_or(""),
                f["id"].as_str().unwrap_or("")
            );
        }
    }
}

fn fmt_map(m: &BTreeMap<String, usize>) -> String {
    if m.is_empty() {
        return "—".into();
    }
    m.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("  ")
}
