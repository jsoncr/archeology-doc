//! Reporte del resultado: páginas del dashboard para el sitio VitePress y un
//! HTML único autocontenido (`adoc report --html`, y fallback de `preview`).
//!
//! Ambos se generan a partir de `status::build`, la misma fuente que usa la
//! extensión de VS Code.

use super::status::{FaqEntry, FindingInfo, Status};
use anyhow::Result;
use archeology_core::i18n::Lang;
use archeology_core::{t, tl, Project};
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
    let status = crate::ui::with_spinner(t!("cli.report.generating"), || {
        super::status::build(project)
    })?;
    let markdown = args.format == "markdown" && !args.html;
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .state_dir()
            .join(if markdown { "report.md" } else { "report.html" })
    });
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let graph = archeology_core::graph::build(project);
    if markdown {
        let lang = project.doc_lang();
        let pages = render_markdown(&status, None, lang);
        // El Markdown no puede dibujar el grafo: solo su resumen.
        let graph_md = format!(
            "## {}\n\n{}",
            tl!(lang, "doc.graph.title"),
            archeology_core::graph_web::summary_markdown(
                &archeology_core::graph::summary(&graph),
                lang,
                |_| None
            )
        );
        archeology_core::fsutil::atomic_write(
            &out,
            format!(
                "{}\n\n{}\n\n{}\n\n{}",
                pages.estado, pages.hallazgos, pages.preguntas, graph_md
            )
            .as_bytes(),
        )?;
    } else {
        archeology_core::fsutil::atomic_write(
            &out,
            render_html(&status, Some(&graph), project.doc_lang()).as_bytes(),
        )?;
    }
    println!("{}", t!("cli.report.written", path = out.display()));
    if !args.no_open {
        super::toolchain::open_in_browser(&out.display().to_string());
    }
    Ok(())
}

/// Escribe el reporte HTML en el espacio de trabajo y devuelve su ruta.
pub fn write_html(project: &Project) -> Result<PathBuf> {
    let status = super::status::build(project)?;
    let graph = archeology_core::graph::build(project);
    let out = project.state_dir().join("report.html");
    fs::create_dir_all(project.state_dir())?;
    archeology_core::fsutil::atomic_write(
        &out,
        render_html(&status, Some(&graph), project.doc_lang()).as_bytes(),
    )?;
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

/// Severidades en orden y la clave de su etiqueta (idioma del proyecto).
const SEVERITIES: &[(&str, &str)] = &[
    ("critical", "doc.report.severity_critical"),
    ("high", "doc.report.severity_high"),
    ("medium", "doc.report.severity_medium"),
    ("low", "doc.report.severity_low"),
];

/// Enlace a una nota del vault dentro del sitio (`/wiki/...`), si se publica.
fn wiki_link(status: &Status, file: &Path, wiki: Option<&str>) -> Option<String> {
    let base = wiki?;
    let rel = file.strip_prefix(&status.project.knowledge).ok()?;
    let rel = rel.with_extension("").to_string_lossy().replace('\\', "/");
    Some(format!("/{base}/{rel}"))
}

/// Texto escrito por agentes dentro de una celda Markdown. Se escapa el HTML:
/// VitePress compila la página como plantilla Vue y un `</script>` rompe el build.
fn md_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

/// Encabezado de una tabla Markdown con las columnas dadas.
fn md_table_header(columns: &[String]) -> String {
    format!(
        "| {} |\n|{}",
        columns.join(" | "),
        "---|".repeat(columns.len())
    )
}

/// `wiki`: carpeta del vault en el sitio (`Some("wiki")`) o `None` sin enlaces.
/// `lang`: idioma de la documentación del proyecto.
pub fn render_markdown(status: &Status, wiki: Option<&str>, lang: Lang) -> Pages {
    let open: Vec<&FindingInfo> = status
        .findings
        .iter()
        .filter(|f| f.state == "open" || f.state == "confirmed")
        .collect();
    let q = &status.questions;
    let counts = SEVERITIES
        .iter()
        .filter_map(|(s, key)| {
            status
                .severity_totals
                .get(*s)
                .map(|n| format!("{}: {n}", tl!(lang, key)))
        })
        .collect::<Vec<_>>()
        .join(" · ");

    // Portada: resumen breve y enlaces.
    let mut portada = format!(
        "## {}\n\n{}\n| **{}%** | {} | {} | {} |\n\n",
        tl!(lang, "doc.report.project_status"),
        md_table_header(&[
            tl!(lang, "doc.report.progress"),
            tl!(lang, "doc.report.open_findings"),
            tl!(lang, "doc.report.features_without_tests"),
            tl!(lang, "doc.report.pending_questions"),
        ]),
        status.progress,
        if open.is_empty() {
            "0".into()
        } else {
            counts.clone()
        },
        status.features_without_tests.len(),
        q.agent.len() + q.team.len(),
    );
    portada.push_str(&tl!(lang, "doc.report.cover_links"));
    portada.push('\n');

    // Estado.
    let mut estado = format!(
        "---\ntitle: {}\n---\n\n# {}\n\n{}\n{}\n{}\n{}\n\n",
        tl!(lang, "doc.report.status"),
        tl!(lang, "doc.report.status_of", name = status.project.name),
        tl!(
            lang,
            "doc.report.plan_progress_item",
            progress = status.progress,
            tasks = status.tasks.len()
        ),
        tl!(
            lang,
            "doc.report.agent_item",
            agent = status.agent.active,
            model = status
                .agent
                .model
                .as_deref()
                .map(|m| format!(" ({m})"))
                .unwrap_or_default()
        ),
        tl!(lang, "doc.report.profile_item", profile = status.profile),
        tl!(
            lang,
            "doc.report.notes_item",
            notes = status.notes.len(),
            stale = status.stale_notes.len()
        ),
    );
    let _ = writeln!(
        estado,
        "## {}\n\n{}",
        tl!(lang, "doc.report.tasks"),
        md_table_header(&[
            tl!(lang, "doc.report.task"),
            tl!(lang, "doc.report.type"),
            tl!(lang, "doc.report.status"),
            tl!(lang, "doc.report.modules"),
        ])
    );
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
        let _ = writeln!(
            estado,
            "\n## {}\n",
            tl!(lang, "doc.report.features_without_report")
        );
        for f in &status.features_without_tests {
            let _ = writeln!(estado, "- {f}");
        }
        let _ = writeln!(
            estado,
            "\n{}",
            tl!(
                lang,
                "doc.report.missing_scenarios_recorded",
                count = status.missing_scenarios
            )
        );
    }
    if let Some(specs) = &status.specs {
        let _ = writeln!(estado, "\n## {}\n", tl!(lang, "doc.report.spec_compliance"));
        if let Some(totals) = specs["totals"].as_object() {
            for (k, v) in totals {
                let _ = writeln!(estado, "- {k}: {v}");
            }
        }
    }

    // Hallazgos.
    let findings_title = tl!(lang, "doc.report.findings");
    let mut hallazgos = format!("---\ntitle: {findings_title}\n---\n\n# {findings_title}\n\n");
    if status.findings.is_empty() {
        let _ = writeln!(hallazgos, "{}", tl!(lang, "doc.report.no_findings"));
    }
    let findings_header = md_table_header(&[
        tl!(lang, "doc.report.finding"),
        tl!(lang, "doc.report.type"),
        tl!(lang, "doc.report.status"),
        tl!(lang, "doc.report.evidence"),
    ]);
    for (sev, key) in SEVERITIES
        .iter()
        .chain([&("unknown", "doc.report.severity_unknown")])
    {
        let items: Vec<&FindingInfo> = status
            .findings
            .iter()
            .filter(|f| f.severity == *sev)
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(hallazgos, "## {}\n\n{findings_header}", tl!(lang, key));
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
    let questions_title = tl!(lang, "doc.report.questions");
    let mut preguntas = format!("---\ntitle: {questions_title}\n---\n\n# {questions_title}\n\n");
    let _ = writeln!(
        preguntas,
        "## {}\n",
        tl!(lang, "doc.report.agent_questions_heading")
    );
    if q.agent.is_empty() {
        let _ = writeln!(
            preguntas,
            "{}\n",
            tl!(lang, "doc.report.no_agent_questions")
        );
    }
    let answer_hint = tl!(lang, "doc.report.answer_hint");
    let answer_placeholder = tl!(lang, "doc.report.answer_placeholder");
    for a in &q.agent {
        let _ = writeln!(
            preguntas,
            "- **{}**: {}\n  - {answer_hint}: `adoc task answer {} \"<{answer_placeholder}>\"`",
            a.task, a.question, a.task
        );
    }
    let _ = writeln!(
        preguntas,
        "\n## {}\n",
        tl!(lang, "doc.report.team_questions_heading")
    );
    if q.team.is_empty() {
        let _ = writeln!(preguntas, "{}\n", tl!(lang, "doc.report.no_team_questions"));
    }
    for t in &q.team {
        let _ = writeln!(preguntas, "- {t}");
    }
    let _ = writeln!(preguntas, "\n## {}\n", tl!(lang, "doc.report.faq_heading"));
    if q.faq.is_empty() {
        let _ = writeln!(preguntas, "{}", tl!(lang, "doc.report.no_faq"));
    }
    for f in &q.faq {
        let _ = writeln!(
            preguntas,
            "### {}\n\n{}\n\n_{}_\n",
            f.question,
            f.answer,
            // `origin` es un valor estable del JSON (`agente` o `equipo`).
            if f.origin == "agente" {
                tl!(lang, "doc.report.answered_by_agent_ask")
            } else {
                tl!(lang, "doc.report.answered_by_team")
            }
        );
    }

    let esc = archeology_core::site::escape_vue_interpolation;
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
    archeology_core::graph_web::file_url(path)
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

/// Reporte HTML único. `lang`: idioma de la documentación del proyecto (textos
/// y atributo `lang` del documento). Los textos de los catálogos son de
/// confianza (pueden llevar HTML); los datos del proyecto se escapan con [`h`].
/// `graph`: grafo del conocimiento a incrustar (sección "Grafo").
///
/// CSP: solo los scripts con el nonce del documento (los del grafo) y nada
/// externo; los estilos en línea se permiten (barras de progreso).
pub fn render_html(
    status: &Status,
    graph: Option<&archeology_core::graph::Graph>,
    lang: Lang,
) -> String {
    let nonce = archeology_core::graph_web::nonce();
    let q = &status.questions;
    let open = status
        .findings
        .iter()
        .filter(|f| f.state == "open" || f.state == "confirmed")
        .count();
    let mut b = String::new();
    let _ = write!(
        b,
        "<!doctype html><html lang=\"{code}\"><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{name} · archeology-doc</title><style>{CSS}{graph_css}</style></head><body><main>\
         <h1>{name}</h1><div class=\"muted\">{meta}</div>\
         <nav style=\"margin-top:12px\"><a href=\"#hallazgos\">{nav_findings}</a><a href=\"#preguntas\">{nav_questions}</a>\
         <a href=\"#faq\">{nav_faq}</a><a href=\"#funcionalidades\">{nav_features}</a>{nav_graph}<a href=\"#tareas\">{nav_tasks}</a></nav>",
        code = lang.code(),
        csp = archeology_core::graph_web::csp(&nonce, true),
        graph_css = if graph.is_some() {
            archeology_core::graph_web::GRAPH_CSS
        } else {
            ""
        },
        nav_graph = if graph.is_some() {
            format!("<a href=\"#grafo\">{}</a>", tl!(lang, "doc.graph.title"))
        } else {
            String::new()
        },
        name = h(&status.project.name),
        meta = tl!(
            lang,
            "doc.report.html_meta",
            agent = h(&status.agent.active),
            model = status
                .agent
                .model
                .as_deref()
                .map(|m| format!(" ({})", h(m)))
                .unwrap_or_default(),
            profile = h(&status.profile),
            tasks = status.tasks.len(),
            notes = status.notes.len()
        ),
        nav_findings = tl!(lang, "doc.report.findings"),
        nav_questions = tl!(lang, "doc.report.questions"),
        nav_faq = tl!(lang, "doc.report.faq"),
        nav_features = tl!(lang, "doc.report.features"),
        nav_tasks = tl!(lang, "doc.report.tasks"),
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
        &tl!(lang, "doc.report.plan_progress"),
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
        &tl!(lang, "doc.report.open_findings"),
        &format!("<div class=\"l\">{sev}</div>"),
    ));
    b.push_str(&card(
        status.features_without_tests.len().to_string(),
        &tl!(lang, "doc.report.features_without_tests"),
        &format!(
            "<div class=\"l\">{}</div>",
            tl!(
                lang,
                "doc.report.missing_scenarios",
                count = status.missing_scenarios
            )
        ),
    ));
    b.push_str(&card(
        (q.agent.len() + q.team.len()).to_string(),
        &tl!(lang, "doc.report.pending_questions"),
        &format!(
            "<div class=\"l\">{}</div>",
            tl!(
                lang,
                "doc.report.questions_split",
                agent = q.agent.len(),
                team = q.team.len()
            )
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
            &tl!(lang, "doc.report.requirements_implemented"),
            "",
        ));
    }
    b.push_str("</div>");

    // Hallazgos.
    let _ = write!(
        b,
        "<h2 id=\"hallazgos\">{}</h2>",
        tl!(lang, "doc.report.findings")
    );
    if status.findings.is_empty() {
        let _ = write!(
            b,
            "<p class=\"muted\">{}</p>",
            tl!(lang, "doc.report.no_findings")
        );
    } else {
        let _ = write!(
            b,
            "<table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
            tl!(lang, "doc.report.severity"),
            tl!(lang, "doc.report.finding"),
            tl!(lang, "doc.report.type"),
            tl!(lang, "doc.report.status"),
            tl!(lang, "doc.report.evidence"),
        );
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
    let _ = write!(
        b,
        "<h2 id=\"preguntas\">{}</h2><h3>{}</h3>",
        tl!(lang, "doc.report.pending_questions"),
        tl!(lang, "doc.report.from_agent")
    );
    let none = tl!(lang, "doc.report.none");
    if q.agent.is_empty() {
        let _ = write!(b, "<p class=\"muted\">{none}</p>");
    } else {
        b.push_str("<ul>");
        let placeholder = h(&tl!(lang, "doc.report.answer_placeholder"));
        for a in &q.agent {
            let _ = write!(
                b,
                "<li><b>{task}</b>: {q}<br><code>adoc task answer {task} \"&lt;{placeholder}&gt;\"</code></li>",
                task = h(&a.task),
                q = h(&a.question)
            );
        }
        b.push_str("</ul>");
    }
    let _ = write!(b, "<h3>{}</h3>", tl!(lang, "doc.report.for_team"));
    if q.team.is_empty() {
        let _ = write!(
            b,
            "<p class=\"muted\">{}</p>",
            tl!(lang, "doc.report.none_recorded")
        );
    } else {
        b.push_str("<ul>");
        for t in &q.team {
            let _ = write!(b, "<li>{}</li>", h(t));
        }
        b.push_str("</ul>");
    }

    // FAQ.
    let _ = write!(
        b,
        "<h2 id=\"faq\">{}</h2>",
        tl!(lang, "doc.report.faq_title")
    );
    if q.faq.is_empty() {
        let _ = write!(
            b,
            "<p class=\"muted\">{}</p>",
            tl!(lang, "doc.report.no_faq_html")
        );
    }
    for f in &q.faq {
        b.push_str(&faq_html(f, lang));
    }

    // Funcionalidades y módulos.
    let _ = write!(
        b,
        "<h2 id=\"funcionalidades\">{}</h2>",
        tl!(lang, "doc.report.features_and_modules")
    );
    let without_tests = tl!(lang, "doc.report.without_tests");
    for (kind, key) in [
        ("feature", "doc.report.features"),
        ("module", "doc.report.modules"),
    ] {
        let notes: Vec<_> = status.notes.iter().filter(|n| n.kind == kind).collect();
        if notes.is_empty() {
            continue;
        }
        let _ = write!(b, "<h3>{} ({})</h3><ul>", tl!(lang, key), notes.len());
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
                    format!(" <span class=\"pill medium\">{without_tests}</span>")
                } else {
                    String::new()
                }
            );
        }
        b.push_str("</ul>");
    }

    // Grafo del conocimiento.
    if let Some(graph) = graph {
        b.push_str(&graph_section(status, graph, lang, &nonce));
    }

    // Tareas.
    let _ = write!(
        b,
        "<h2 id=\"tareas\">{}</h2><table><tr><th>{}</th><th>{}</th><th>{}</th><th>{}</th></tr>",
        tl!(lang, "doc.report.tasks"),
        tl!(lang, "doc.report.task"),
        tl!(lang, "doc.report.type"),
        tl!(lang, "doc.report.status"),
        tl!(lang, "doc.report.modules"),
    );
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
        "<p class=\"muted\" style=\"margin-top:28px\">{}</p></main></body></html>",
        tl!(
            lang,
            "doc.report.generated_by",
            date = chrono::Local::now().format("%Y-%m-%d %H:%M %:z")
        )
    );
    b
}

/// Sección "Grafo" del reporte: el renderizador compartido con los datos
/// incrustados (JSON escapado) y el resumen (más conectadas, huérfanas).
fn graph_section(
    status: &Status,
    graph: &archeology_core::graph::Graph,
    lang: Lang,
    nonce: &str,
) -> String {
    use archeology_core::graph_web;
    let s = archeology_core::graph::summary(graph);
    let data = graph_web::data(&status.project.root, graph, graph_web::Links::Local);
    let mut b = format!(
        "<h2 id=\"grafo\">{}</h2><p class=\"muted\">{} {}</p>",
        tl!(lang, "doc.graph.title"),
        tl!(
            lang,
            "doc.graph.counts_html",
            notes = s.notes,
            code = s.code_files,
            edges = s.edges
        ),
        tl!(lang, "doc.graph.report_intro"),
    );
    b.push_str(&graph_web::embed(
        "adoc-graph",
        &data,
        lang,
        nonce,
        "adoc-graph-frame",
    ));
    if !s.top_connected.is_empty() {
        let _ = write!(b, "<h3>{}</h3><ol>", tl!(lang, "doc.graph.top_heading"));
        for c in &s.top_connected {
            let _ = write!(
                b,
                "<li>{}</li>",
                tl!(
                    lang,
                    "doc.graph.top_item",
                    title = h(&c.title),
                    count = c.connections
                )
            );
        }
        b.push_str("</ol>");
    }
    let _ = write!(
        b,
        "<h3>{}</h3><p class=\"muted\">{}</p>",
        tl!(lang, "doc.graph.orphans_heading"),
        tl!(lang, "doc.graph.orphans_hint")
    );
    if s.orphans.is_empty() {
        let _ = write!(b, "<p>{}</p>", tl!(lang, "doc.graph.no_orphans"));
    } else {
        b.push_str("<ul>");
        for o in &s.orphans {
            let _ = write!(b, "<li>{} <code>{}</code></li>", h(&o.title), h(&o.id));
        }
        b.push_str("</ul>");
    }
    b
}

fn faq_html(f: &FaqEntry, lang: Lang) -> String {
    format!(
        "<div class=\"faq\"><h3>{q}</h3><p>{a}</p><div class=\"muted\">{origin} · <a href=\"{url}\">{source}</a></div></div>",
        q = h(&f.question),
        a = h(&f.answer),
        // `origin` es un valor estable del JSON (`agente` o `equipo`).
        origin = if f.origin == "agente" {
            tl!(lang, "doc.report.answered_by_agent")
        } else {
            tl!(lang, "doc.report.answered_by_team")
        },
        url = h(&file_url(&f.file)),
        source = tl!(lang, "doc.report.source"),
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

    #[test]
    fn markdown_cells_escape_html() {
        assert_eq!(
            md_escape("a </script> | b & c\nd"),
            "a &lt;/script&gt; \\| b &amp; c d"
        );
    }
}
