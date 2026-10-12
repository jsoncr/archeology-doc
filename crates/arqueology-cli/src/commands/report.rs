//! Reporte del resultado: páginas del dashboard para el sitio VitePress y un
//! HTML único autocontenido (`adoc report --html`, y fallback de `preview`).
//!
//! Ambos se generan a partir de `status::build`, la misma fuente que usa la
//! extensión de VS Code.

use super::status::{FaqEntry, FindingInfo, Status};
use anyhow::Result;
use arqueology_core::Project;
use clap::Args;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Args)]
pub struct ReportArgs {
    /// Formato del reporte.
    #[arg(long, default_value = "html", value_parser = ["html", "markdown"])]
    format: String,
    /// Atajo de `--format html`.
    #[arg(long, conflicts_with = "format")]
    html: bool,
    /// Ruta de salida (por defecto, en el espacio de trabajo del proyecto).
    #[arg(long)]
    out: Option<PathBuf>,
    /// No abrir el reporte en el navegador.
    #[arg(long)]
    no_open: bool,
}

pub fn run(project: &Project, args: ReportArgs) -> Result<()> {
    let status = crate::ui::with_spinner("Generando reporte…", || super::status::build(project))?;
    let markdown = args.format == "markdown" && !args.html;
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .state_dir()
            .join(if markdown { "report.md" } else { "report.html" })
    });
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    if markdown {
        let pages = render_markdown(&status, None);
        fs::write(
            &out,
            format!(
                "{}\n\n{}\n\n{}",
                pages.estado, pages.hallazgos, pages.preguntas
            ),
        )?;
    } else {
        fs::write(&out, render_html(&status))?;
    }
    println!("reporte: {}", out.display());
    if !args.no_open {
        super::toolchain::open_in_browser(&out.display().to_string());
    }
    Ok(())
}

/// Escribe el reporte HTML en el espacio de trabajo y devuelve su ruta.
pub fn write_html(project: &Project) -> Result<PathBuf> {
    let status = super::status::build(project)?;
    let out = project.state_dir().join("report.html");
    fs::write(&out, render_html(&status))?;
    Ok(out)
}

// ── Markdown (páginas del sitio) ──────────────────────────────────────────

/// Páginas del dashboard para `docs/`.
pub struct Pages {
    /// Resumen para la región `dashboard` de la portada.
    pub portada: String,
    pub estado: String,
    pub hallazgos: String,
    pub preguntas: String,
}

const SEVERITIES: &[(&str, &str)] = &[
    ("critical", "Críticos"),
    ("high", "Altos"),
    ("medium", "Medios"),
    ("low", "Bajos"),
];

/// Enlace a una nota del vault dentro del sitio (`/wiki/...`), si se publica.
fn wiki_link(status: &Status, file: &Path, wiki: Option<&str>) -> Option<String> {
    let base = wiki?;
    let rel = file.strip_prefix(&status.project.knowledge).ok()?;
    let rel = rel.with_extension("").to_string_lossy().replace('\\', "/");
    Some(format!("/{base}/{rel}"))
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// `wiki`: carpeta del vault en el sitio (`Some("wiki")`) o `None` sin enlaces.
pub fn render_markdown(status: &Status, wiki: Option<&str>) -> Pages {
    let open: Vec<&FindingInfo> = status
        .findings
        .iter()
        .filter(|f| f.state == "open" || f.state == "confirmed")
        .collect();
    let q = &status.questions;
    let counts = SEVERITIES
        .iter()
        .filter_map(|(s, label)| {
            status
                .severity_totals
                .get(*s)
                .map(|n| format!("{label}: {n}"))
        })
        .collect::<Vec<_>>()
        .join(" · ");

    // Portada: resumen breve y enlaces.
    let mut portada = format!(
        "## Estado del proyecto\n\n\
         | Progreso | Hallazgos abiertos | Funcionalidades sin tests | Preguntas pendientes |\n\
         |---|---|---|---|\n\
         | **{}%** | {} | {} | {} |\n\n",
        status.progress,
        if open.is_empty() {
            "0".into()
        } else {
            counts.clone()
        },
        status.features_without_tests.len(),
        q.agent.len() + q.team.len(),
    );
    portada.push_str(
        "[Ver estado](/estado) · [Hallazgos](/hallazgos) · [Preguntas y FAQ](/preguntas)\n",
    );

    // Estado.
    let mut estado = format!(
        "---\ntitle: Estado\n---\n\n# Estado de {}\n\n\
         - **Progreso del plan:** {}% ({} tareas)\n\
         - **Agente:** {}{}\n\
         - **Perfil:** {}\n\
         - **Notas:** {} ({} desactualizadas)\n\n",
        status.project.name,
        status.progress,
        status.tasks.len(),
        status.agent.active,
        status
            .agent
            .model
            .as_deref()
            .map(|m| format!(" ({m})"))
            .unwrap_or_default(),
        status.profile,
        status.notes.len(),
        status.stale_notes.len(),
    );
    estado.push_str("## Tareas\n\n| Tarea | Tipo | Estado | Módulos |\n|---|---|---|---|\n");
    for t in &status.tasks {
        let _ = writeln!(
            estado,
            "| {} | {} | `{}` | {} |",
            t.id,
            t.kind,
            t.status,
            md_escape(t.module.as_deref().unwrap_or("—"))
        );
    }
    if !status.features_without_tests.is_empty() {
        estado.push_str("\n## Funcionalidades sin reporte de tests\n\n");
        for f in &status.features_without_tests {
            let _ = writeln!(estado, "- {f}");
        }
        let _ = writeln!(
            estado,
            "\nEscenarios faltantes registrados: {}",
            status.missing_scenarios
        );
    }
    if let Some(specs) = &status.specs {
        estado.push_str("\n## Cumplimiento de specs\n\n");
        if let Some(totals) = specs["totals"].as_object() {
            for (k, v) in totals {
                let _ = writeln!(estado, "- {k}: {v}");
            }
        }
    }

    // Hallazgos.
    let mut hallazgos = String::from("---\ntitle: Hallazgos\n---\n\n# Hallazgos\n\n");
    if status.findings.is_empty() {
        hallazgos.push_str("Todavía no hay hallazgos.\n");
    }
    for (sev, label) in SEVERITIES.iter().chain([&("unknown", "Sin severidad")]) {
        let items: Vec<&FindingInfo> = status
            .findings
            .iter()
            .filter(|f| f.severity == *sev)
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(
            hallazgos,
            "## {label}\n\n| Hallazgo | Tipo | Estado | Evidencia |\n|---|---|---|---|"
        );
        for f in items {
            let title = match wiki_link(status, &f.file, wiki) {
                Some(link) => format!("[{}]({link})", md_escape(&f.title)),
                None => md_escape(&f.title),
            };
            let evidence = f
                .sources
                .iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(
                hallazgos,
                "| {title} | {} | {} | {evidence} |",
                f.kind, f.state
            );
        }
        hallazgos.push('\n');
    }

    // Preguntas y FAQ.
    let mut preguntas = String::from("---\ntitle: Preguntas\n---\n\n# Preguntas\n\n");
    preguntas.push_str("## Preguntas de agentes pendientes\n\n");
    if q.agent.is_empty() {
        preguntas.push_str("Ninguna: los agentes no están esperando respuesta.\n\n");
    }
    for a in &q.agent {
        let _ = writeln!(
            preguntas,
            "- **{}**: {}\n  - Responder: `adoc task answer {} \"<respuesta>\"`",
            a.task, a.question, a.task
        );
    }
    preguntas.push_str("\n## Preguntas abiertas para el equipo técnico\n\n");
    if q.team.is_empty() {
        preguntas.push_str("Ninguna registrada en `knowledge/memory/open-questions.md`.\n\n");
    }
    for t in &q.team {
        let _ = writeln!(preguntas, "- {t}");
    }
    preguntas.push_str("\n## Preguntas frecuentes (FAQ)\n\n");
    if q.faq.is_empty() {
        preguntas.push_str(
            "Aún no hay preguntas respondidas. Pregunta al agente con `adoc ask \"<pregunta>\"`.\n",
        );
    }
    for f in &q.faq {
        let _ = writeln!(
            preguntas,
            "### {}\n\n{}\n\n_{}_\n",
            f.question,
            f.answer,
            if f.origin == "agente" {
                "Respondida por el agente (adoc ask)"
            } else {
                "Respondida por el equipo"
            }
        );
    }

    let esc = arqueology_core::site::escape_vue_interpolation;
    Pages {
        portada: esc(&portada),
        estado: esc(&estado),
        hallazgos: esc(&hallazgos),
        preguntas: esc(&preguntas),
    }
}

// ── HTML único ────────────────────────────────────────────────────────────

/// Escapa texto para HTML (contenido escrito por agentes o personas).
pub fn h(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn file_url(path: &Path) -> String {
    let p = path.display().to_string().replace('\\', "/");
    let encoded: String = p
        .chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            _ => c.to_string(),
        })
        .collect();
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

const CSS: &str = r#"
:root{--bg:#f7f7f5;--card:#fff;--fg:#1d1d1f;--muted:#6b6b70;--line:#e3e3e0;--accent:#2f6fde;
--crit:#b3261e;--high:#d1453b;--med:#b7791f;--low:#2f6fde;--ok:#2e7d4f}
@media (prefers-color-scheme:dark){:root{--bg:#141416;--card:#1d1d20;--fg:#ececee;--muted:#9a9aa1;
--line:#2c2c31;--accent:#7aa2f7;--crit:#ff6b60;--high:#f28b82;--med:#f2c46d;--low:#8ab4f8;--ok:#7ccf9a}}
*{box-sizing:border-box}body{margin:0;font:15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
background:var(--bg);color:var(--fg)}main{max-width:1080px;margin:0 auto;padding:32px 20px 64px}
h1{font-size:26px;margin:0 0 4px}h2{font-size:18px;margin:36px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--line)}
h3{font-size:15px;margin:18px 0 6px}.muted{color:var(--muted)}a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(170px,1fr));gap:12px;margin-top:20px}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px}
.card .n{font-size:26px;font-weight:650}.card .l{color:var(--muted);font-size:13px}
.bar{height:8px;background:var(--line);border-radius:4px;overflow:hidden;margin-top:8px}.bar>i{display:block;height:100%;background:var(--ok)}
table{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:10px;overflow:hidden}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line);vertical-align:top;font-size:14px}th{color:var(--muted);font-weight:600}
tr:last-child td{border-bottom:0}code{font:12.5px ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--bg);padding:1px 5px;border-radius:4px}
.pill{display:inline-block;padding:1px 8px;border-radius:999px;font-size:12px;font-weight:600;border:1px solid currentColor}
.critical{color:var(--crit)}.high{color:var(--high)}.medium{color:var(--med)}.low{color:var(--low)}.done{color:var(--ok)}
.question,.failed{color:var(--high)}ul{padding-left:20px}li{margin:4px 0}
.faq{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:4px 16px 12px;margin-bottom:10px}
.faq p{white-space:pre-wrap;margin:6px 0}nav a{margin-right:14px}
"#;

pub fn render_html(status: &Status) -> String {
    let q = &status.questions;
    let open = status
        .findings
        .iter()
        .filter(|f| f.state == "open" || f.state == "confirmed")
        .count();
    let mut b = String::new();
    let _ = write!(
        b,
        "<!doctype html><html lang=\"es\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{name} · arqueology-doc</title><style>{CSS}</style></head><body><main>\
         <h1>{name}</h1><div class=\"muted\">Agente <b>{agent}</b>{model} · perfil {profile} · \
         {tasks} tareas · {notes} notas</div>\
         <nav style=\"margin-top:12px\"><a href=\"#hallazgos\">Hallazgos</a><a href=\"#preguntas\">Preguntas</a>\
         <a href=\"#faq\">FAQ</a><a href=\"#funcionalidades\">Funcionalidades</a><a href=\"#tareas\">Tareas</a></nav>",
        name = h(&status.project.name),
        agent = h(&status.agent.active),
        model = status.agent.model.as_deref().map(|m| format!(" ({})", h(m))).unwrap_or_default(),
        profile = h(&status.profile),
        tasks = status.tasks.len(),
        notes = status.notes.len(),
    );

    // Tarjetas.
    let card = |n: String, l: &str, extra: &str| {
        format!(
            "<div class=\"card\"><div class=\"n\">{n}</div><div class=\"l\">{l}</div>{extra}</div>"
        )
    };
    b.push_str("<div class=\"cards\">");
    b.push_str(&card(
        format!("{}%", status.progress),
        "Progreso del plan",
        &format!(
            "<div class=\"bar\"><i style=\"width:{}%\"></i></div>",
            status.progress.min(100)
        ),
    ));
    let sev = SEVERITIES
        .iter()
        .filter_map(|(s, _)| {
            status
                .severity_totals
                .get(*s)
                .map(|n| format!("<span class=\"{s}\">{n} {s}</span>"))
        })
        .collect::<Vec<_>>()
        .join(" · ");
    b.push_str(&card(
        open.to_string(),
        "Hallazgos abiertos",
        &format!("<div class=\"l\">{sev}</div>"),
    ));
    b.push_str(&card(
        status.features_without_tests.len().to_string(),
        "Funcionalidades sin tests",
        &format!(
            "<div class=\"l\">{} escenarios faltantes</div>",
            status.missing_scenarios
        ),
    ));
    b.push_str(&card(
        (q.agent.len() + q.team.len()).to_string(),
        "Preguntas pendientes",
        &format!(
            "<div class=\"l\">{} del agente · {} para el equipo</div>",
            q.agent.len(),
            q.team.len()
        ),
    ));
    if let Some(totals) = status.specs.as_ref().and_then(|s| s["totals"].as_object()) {
        let implemented = totals
            .get("implemented")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let total: u64 = totals.values().filter_map(|v| v.as_u64()).sum();
        b.push_str(&card(
            format!("{implemented}/{total}"),
            "Requisitos implementados",
            "",
        ));
    }
    b.push_str("</div>");

    // Hallazgos.
    b.push_str("<h2 id=\"hallazgos\">Hallazgos</h2>");
    if status.findings.is_empty() {
        b.push_str("<p class=\"muted\">Todavía no hay hallazgos.</p>");
    } else {
        b.push_str("<table><tr><th>Severidad</th><th>Hallazgo</th><th>Tipo</th><th>Estado</th><th>Evidencia</th></tr>");
        let rank = |s: &str| SEVERITIES.iter().position(|(x, _)| *x == s).unwrap_or(9);
        let mut sorted: Vec<&FindingInfo> = status.findings.iter().collect();
        sorted.sort_by_key(|f| (rank(&f.severity), f.state != "open"));
        for f in sorted {
            let evidence = f
                .sources
                .iter()
                .map(|s| {
                    let ev = s.rsplit_once(':').map(|(p, _)| p).unwrap_or(s);
                    format!(
                        "<a href=\"{}\"><code>{}</code></a>",
                        h(&file_url(&status.project.root.join(ev))),
                        h(s)
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            let _ = write!(
                b,
                "<tr><td><span class=\"pill {s}\">{s}</span></td><td><a href=\"{url}\">{title}</a></td>\
                 <td>{kind}</td><td>{state}</td><td>{evidence}</td></tr>",
                s = h(&f.severity),
                url = h(&file_url(&f.file)),
                title = h(&f.title),
                kind = h(&f.kind),
                state = h(&f.state),
            );
        }
        b.push_str("</table>");
    }

    // Preguntas.
    b.push_str(
        "<h2 id=\"preguntas\">Preguntas pendientes</h2><h3>Del agente (bloquean tareas)</h3>",
    );
    if q.agent.is_empty() {
        b.push_str("<p class=\"muted\">Ninguna.</p>");
    } else {
        b.push_str("<ul>");
        for a in &q.agent {
            let _ = write!(
                b,
                "<li><b>{task}</b>: {q}<br><code>adoc task answer {task} \"&lt;respuesta&gt;\"</code></li>",
                task = h(&a.task),
                q = h(&a.question)
            );
        }
        b.push_str("</ul>");
    }
    b.push_str("<h3>Para el equipo técnico</h3>");
    if q.team.is_empty() {
        b.push_str("<p class=\"muted\">Ninguna registrada.</p>");
    } else {
        b.push_str("<ul>");
        for t in &q.team {
            let _ = write!(b, "<li>{}</li>", h(t));
        }
        b.push_str("</ul>");
    }

    // FAQ.
    b.push_str("<h2 id=\"faq\">Preguntas frecuentes</h2>");
    if q.faq.is_empty() {
        b.push_str("<p class=\"muted\">Aún no hay preguntas respondidas. Pregunta al agente con <code>adoc ask \"&lt;pregunta&gt;\"</code>.</p>");
    }
    for f in &q.faq {
        b.push_str(&faq_html(f));
    }

    // Funcionalidades y módulos.
    b.push_str("<h2 id=\"funcionalidades\">Funcionalidades y módulos</h2>");
    for (kind, label) in [("feature", "Funcionalidades"), ("module", "Módulos")] {
        let notes: Vec<_> = status.notes.iter().filter(|n| n.kind == kind).collect();
        if notes.is_empty() {
            continue;
        }
        let _ = write!(b, "<h3>{label} ({})</h3><ul>", notes.len());
        for n in notes {
            let without =
                n.id.as_ref()
                    .map(|id| status.features_without_tests.contains(id))
                    .unwrap_or(false);
            let _ = write!(
                b,
                "<li><a href=\"{}\">{}</a>{}</li>",
                h(&file_url(&n.file)),
                h(&n.title),
                if without {
                    " <span class=\"pill medium\">sin tests</span>"
                } else {
                    ""
                }
            );
        }
        b.push_str("</ul>");
    }

    // Tareas.
    b.push_str("<h2 id=\"tareas\">Tareas</h2><table><tr><th>Tarea</th><th>Tipo</th><th>Estado</th><th>Módulos</th></tr>");
    for t in &status.tasks {
        let _ = write!(
            b,
            "<tr><td>{id}</td><td>{kind}</td><td><span class=\"pill {st}\">{st}</span></td><td>{m}</td></tr>",
            id = h(&t.id),
            kind = h(&t.kind),
            st = h(&t.status),
            m = h(&t.module.clone().unwrap_or_default().replace(',', ", ")),
        );
    }
    b.push_str("</table>");
    let _ = write!(
        b,
        "<p class=\"muted\" style=\"margin-top:28px\">Generado por arqueology-doc · {}</p></main></body></html>",
        chrono::Local::now().format("%Y-%m-%d %H:%M %:z")
    );
    b
}

fn faq_html(f: &FaqEntry) -> String {
    format!(
        "<div class=\"faq\"><h3>{q}</h3><p>{a}</p><div class=\"muted\">{origin} · <a href=\"{url}\">fuente</a></div></div>",
        q = h(&f.question),
        a = h(&f.answer),
        origin = if f.origin == "agente" { "Respondida por el agente" } else { "Respondida por el equipo" },
        url = h(&file_url(&f.file)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html() {
        assert_eq!(
            h("<script>a&\"b\"</script>"),
            "&lt;script&gt;a&amp;&quot;b&quot;&lt;/script&gt;"
        );
        assert_eq!(file_url(Path::new("/a b/c#d.md")), "file:///a%20b/c%23d.md");
    }
}
