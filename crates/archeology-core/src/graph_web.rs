//! Vistas web del grafo del conocimiento: datos para el renderizador
//! compartido (`assets/graph.js`), HTML autocontenido (`adoc graph --format
//! html`, reporte HTML) y la página del sitio VitePress (`adoc site sync`).
//!
//! El renderizador es una sola copia: este crate lo embebe con
//! `include_str!` y la extensión de VS Code lo copia al compilar.
//!
//! Seguridad del HTML generado: los datos (títulos escritos por agentes o
//! personas) van en un `<script type="application/json">` con `<`, `>` y `&`
//! escapados como `<`… ([`script_json`]), así un título con
//! `</script>` o `<!--` no puede cerrar el bloque. La página independiente
//! lleva una CSP estricta con nonce y no carga nada externo.

use crate::error::{Error, Result};
use crate::graph::{Graph, Summary};
use crate::i18n::{translate, Lang};
use crate::project::Project;
use crate::site::{page_link, GENERATED_MARK, WIKI_DIR};
use crate::{t, tl};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Renderizador del grafo (copia canónica, sin dependencias).
pub const GRAPH_JS: &str = include_str!("../assets/graph.js");
/// Estilos del renderizador.
pub const GRAPH_CSS: &str = include_str!("../assets/graph.css");
/// Página del grafo en el sitio: `docs/grafo.md` (como `estado.md`, el
/// nombre no se traduce).
pub const SITE_PAGE: &str = "grafo";
/// Carpeta de `docs/` con los recursos del grafo (VitePress copia `public/` tal cual).
pub const SITE_ASSETS_DIR: &str = "public/adoc-graph";
/// Ruta pública de los recursos del grafo (relativa a la base del sitio).
const SITE_ASSETS_URL: &str = "adoc-graph/";

/// Tipos de nota con etiqueta traducida (`graph.kind.<tipo>`).
const KINDS: &[&str] = &[
    "overview",
    "module",
    "feature",
    "finding",
    "test-report",
    "spec-report",
    "glossary",
    "decision",
    "memory",
    "faq",
    "moc",
    "code",
    "unknown",
];

/// Claves de los textos del renderizador (`graph.web.<clave>`).
const WEB_KEYS: &[&str] = &[
    "search",
    "fit",
    "refresh",
    "zoom_in",
    "zoom_out",
    "stats",
    "stale",
    "legend",
    "open",
    "close",
    "path",
    "empty",
    "edge_relation",
    "edge_evidence",
];

/// Textos del renderizador en `lang` (diccionario `{clave: texto}`; los
/// marcadores `{nodes}`/`{edges}` los sustituye el navegador).
pub fn strings(lang: Lang) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in WEB_KEYS {
        out.insert(
            key.to_string(),
            translate(lang, &format!("graph.web.{key}"), &[]),
        );
    }
    for kind in KINDS {
        out.insert(
            format!("kind.{kind}"),
            translate(lang, &format!("graph.kind.{kind}"), &[]),
        );
    }
    out
}

/// Cómo enlaza cada nodo.
#[derive(Debug, Clone, Copy)]
pub enum Links<'a> {
    /// Archivo local (`file://`): página independiente y reporte HTML.
    Local,
    /// Sitio VitePress: las notas enlazan a su página del vault publicado
    /// (`wiki`: `Some("wiki")` si se publica). Sin rutas absolutas: el sitio
    /// puede publicarse.
    Site {
        docs: &'a Path,
        wiki: Option<&'a str>,
    },
}

/// Nodo tal como lo recibe el renderizador.
#[derive(Debug, Serialize)]
struct WebNode<'a> {
    id: &'a str,
    title: &'a str,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<&'a str>,
    stale: bool,
    /// Ruta relativa a la raíz del proyecto.
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    /// Enlace de la ficha del nodo (`file://…`).
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    /// Página del sitio a la que navegar al hacer clic.
    #[serde(skip_serializing_if = "Option::is_none")]
    link: Option<String>,
}

/// Datos `{nodes, edges}` para el renderizador. `root`: raíz del proyecto.
pub fn data(root: &Path, graph: &Graph, links: Links) -> serde_json::Value {
    let nodes: Vec<WebNode> = graph
        .nodes
        .iter()
        .map(|n| {
            let path = n
                .file
                .strip_prefix(root)
                .unwrap_or(&n.file)
                .to_string_lossy()
                .replace('\\', "/");
            let (href, link) = match links {
                Links::Local => (Some(file_url(&n.file)), None),
                Links::Site { docs, wiki } => {
                    let link = wiki
                        .filter(|_| n.kind != "code")
                        .map(|w| page_link(docs, &docs.join(w).join(format!("{}.md", n.id))));
                    (None, link)
                }
            };
            WebNode {
                id: &n.id,
                title: &n.title,
                kind: &n.kind,
                status: n.status.as_deref(),
                severity: n.severity.as_deref(),
                state: n.state.as_deref(),
                stale: n.stale,
                path,
                line: n.line,
                href,
                link,
            }
        })
        .collect();
    serde_json::json!({ "nodes": nodes, "edges": graph.edges })
}

/// URL `file://` de una ruta local (espacios, `#` y `?` codificados).
pub fn file_url(path: &Path) -> String {
    let p = path.display().to_string().replace('\\', "/");
    let mut encoded = String::with_capacity(p.len());
    for c in p.chars() {
        match c {
            ' ' => encoded.push_str("%20"),
            '#' => encoded.push_str("%23"),
            '?' => encoded.push_str("%3F"),
            '%' => encoded.push_str("%25"),
            _ => encoded.push(c),
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// JSON seguro dentro de `<script>`: `<`, `>`, `&` y los separadores de
/// línea U+2028/U+2029 se escapan como `\uXXXX` (sigue siendo JSON válido).
/// Ni `</script>` ni `<!--` pueden aparecer en la salida.
pub fn script_json(value: &impl Serialize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapa texto para HTML (contenido y atributos).
pub fn escape_html(s: &str) -> String {
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

/// Nonce aleatorio para la CSP de un HTML generado.
pub fn nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Política CSP de un HTML generado: solo los scripts con `nonce`, sin
/// recursos externos. `inline_styles`: permite atributos `style` (el reporte
/// los usa); si no, solo los `<style>` con nonce.
pub fn csp(nonce: &str, inline_styles: bool) -> String {
    let style = if inline_styles {
        "'unsafe-inline'".to_string()
    } else {
        format!("'nonce-{nonce}'")
    };
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; style-src {style}; img-src data:; \
         base-uri 'none'; form-action 'none'"
    )
}

/// Contenedor del grafo + datos + renderizador + arranque, para incrustar en
/// un HTML con la CSP de [`csp`]. `id`: id del contenedor (único en la página).
pub fn embed(id: &str, data: &serde_json::Value, lang: Lang, nonce: &str, class: &str) -> String {
    let payload = serde_json::json!({ "data": data, "strings": strings(lang) });
    let id = escape_html(id);
    format!(
        "<div id=\"{id}\" class=\"adoc-graph {class}\"><noscript>{no_script}</noscript></div>\
         <script type=\"application/json\" id=\"{id}-data\">{json}</script>\
         <script nonce=\"{nonce}\">{js}</script>\
         <script nonce=\"{nonce}\">(function(){{var host=document.getElementById('{id}');\
         var p=JSON.parse(document.getElementById('{id}-data').textContent);\
         var g=window.AdocGraph.mount(host,{{t:p.strings}});g.setData(p.data);}})();</script>",
        class = escape_html(class),
        no_script = escape_html(&tl!(lang, "graph.page.no_script")),
        json = script_json(&payload),
        js = GRAPH_JS,
    )
}

/// Página HTML autocontenida con el grafo (`adoc graph --format html`).
/// `lang`: idioma de los textos (el de la interfaz).
pub fn standalone_html(project: &Project, graph: &Graph, lang: Lang) -> String {
    let s = crate::graph::summary(graph);
    let nonce = nonce();
    let data = data(&project.root, graph, Links::Local);
    let title = tl!(lang, "graph.page.title", name = project.config.project.name);
    format!(
        "<!doctype html><html lang=\"{code}\"><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"light dark\">\
         <title>{title}</title><style nonce=\"{nonce}\">{page_css}{graph_css}</style></head>\
         <body><header><h1>{title}</h1><div class=\"meta\">{counts} · {hint}</div></header>\
         <main>{embed}</main></body></html>",
        code = lang.code(),
        csp = csp(&nonce, false),
        title = escape_html(&title),
        page_css = PAGE_CSS,
        graph_css = GRAPH_CSS,
        counts = escape_html(&tl!(
            lang,
            "graph.page.counts",
            notes = s.notes,
            code = s.code_files,
            edges = s.edges
        )),
        hint = escape_html(&tl!(lang, "graph.page.hint")),
        embed = embed("adoc-graph", &data, lang, &nonce, "adoc-graph-page"),
    )
}

/// Estilos de la página independiente (claro/oscuro según el sistema).
const PAGE_CSS: &str = r#"
:root{color-scheme:light dark;--page-bg:#f7f7f5;--page-fg:#1d1d1f;--page-muted:#6b6b70;--page-line:#e3e3e0}
@media (prefers-color-scheme:dark){:root{--page-bg:#141416;--page-fg:#ececee;--page-muted:#9a9aa1;--page-line:#2c2c31}}
html,body{height:100%;margin:0}
body{display:flex;flex-direction:column;background:var(--page-bg);color:var(--page-fg);
font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
header{padding:12px 18px;border-bottom:1px solid var(--page-line)}
h1{font-size:18px;margin:0}.meta{color:var(--page-muted);font-size:13px}
main{flex:1;min-height:0;display:flex}
.adoc-graph-page{flex:1;min-height:320px}
"#;

/// Escapa un título para Markdown: tablas, enlaces y HTML. El sitio aplica
/// además `escape_vue_interpolation` a la página completa.
fn md_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
            '|' | '[' | ']' | '*' | '_' | '`' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// Huérfanas listadas como máximo (el resto se resume con "… y N más").
const MAX_ORPHANS: usize = 30;

/// Resumen en Markdown (reporte y página del sitio), sin encabezado
/// principal. `link`: página de una nota por su id (sitio) o `None`.
pub fn summary_markdown(s: &Summary, lang: Lang, link: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}\n",
        tl!(
            lang,
            "doc.graph.counts",
            notes = s.notes,
            code = s.code_files,
            edges = s.edges
        )
    );
    if !s.edges_by_kind.is_empty() {
        let _ = writeln!(
            out,
            "{}\n",
            tl!(
                lang,
                "doc.graph.edges",
                counts = fmt_counts(&s.edges_by_kind)
            )
        );
    }
    let label = |id: &str, title: &str| match link(id) {
        Some(l) => format!("[{}]({l})", md_text(title)),
        None => md_text(title),
    };
    if !s.top_connected.is_empty() {
        let _ = writeln!(out, "### {}\n", tl!(lang, "doc.graph.top_heading"));
        for (i, c) in s.top_connected.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}. {}",
                i + 1,
                tl!(
                    lang,
                    "doc.graph.top_item",
                    title = label(&c.id, &c.title),
                    count = c.connections
                )
            );
        }
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "### {}\n\n{}\n",
        tl!(lang, "doc.graph.orphans_heading"),
        tl!(lang, "doc.graph.orphans_hint")
    );
    if s.orphans.is_empty() {
        let _ = writeln!(out, "{}", tl!(lang, "doc.graph.no_orphans"));
    }
    for o in s.orphans.iter().take(MAX_ORPHANS) {
        let _ = writeln!(
            out,
            "- {} (`{}`)",
            label(&o.id, &o.title),
            o.id.replace('`', "'")
        );
    }
    if s.orphans.len() > MAX_ORPHANS {
        let _ = writeln!(
            out,
            "- {}",
            tl!(
                lang,
                "doc.graph.more",
                count = s.orphans.len() - MAX_ORPHANS
            )
        );
    }
    out
}

/// `link=3  evidence=5` (orden alfabético).
pub fn fmt_counts(m: &BTreeMap<String, usize>) -> String {
    m.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("  ")
}

/// Página del grafo y sus recursos en el sitio (`adoc site sync`):
/// `docs/grafo.md` y `docs/public/adoc-graph/{graph.js,graph.css,graph.json}`.
/// La carpeta de recursos lleva [`GENERATED_MARK`]: si existe sin la marca,
/// es del usuario y no se toca (error).
pub fn write_site(project: &Project, graph: &Graph) -> Result<()> {
    let docs = project.docs_dir();
    let lang = project.doc_lang();
    let assets = docs.join(SITE_ASSETS_DIR);
    if let Ok(meta) = fs::symlink_metadata(&assets) {
        let marked = fs::symlink_metadata(assets.join(GENERATED_MARK))
            .is_ok_and(|m| m.file_type().is_file());
        if !meta.is_dir() || !marked {
            return Err(Error::Validation(t!(
                "core.site.not_generated",
                path = project.rel(&assets)
            )));
        }
    }
    crate::fsutil::create_dir_in_root(&project.root, &assets)?;
    let wiki = project.config.site.include_knowledge.then_some(WIKI_DIR);
    let mut payload = data(&project.root, graph, Links::Site { docs: &docs, wiki });
    payload["strings"] = serde_json::to_value(strings(lang))?;
    let write = |name: &str, content: &str| {
        crate::fsutil::write_if_changed_in_root(&project.root, &assets.join(name), content)
    };
    write(
        GENERATED_MARK,
        &format!("{}\n", tl!(lang, "doc.site.generated_mark")),
    )?;
    write("graph.js", GRAPH_JS)?;
    write("graph.css", GRAPH_CSS)?;
    write("graph.json", &serde_json::to_string(&payload)?)?;

    let summary = crate::graph::summary(graph);
    let page_of = |id: &str| wiki.map(|w| page_link(&docs, &docs.join(w).join(format!("{id}.md"))));
    let page = site_page(lang, &summary_markdown(&summary, lang, page_of));
    crate::fsutil::write_if_changed_in_root(
        &project.root,
        &docs.join(format!("{SITE_PAGE}.md")),
        &crate::site::escape_vue_interpolation(&page),
    )?;
    Ok(())
}

/// `docs/grafo.md`: el contenedor y un componente sin imports (funciona con
/// el VitePress compartido) que carga `graph.js` y `graph.json` de `public/`.
/// Al hacer clic en una nota navega a su página; el código muestra su ruta.
fn site_page(lang: Lang, summary_md: &str) -> String {
    let title = tl!(lang, "doc.graph.title");
    format!(
        "---\ntitle: {title}\naside: false\n---\n\n# {title}\n\n{intro}\n\n\
         <div id=\"adoc-graph\" class=\"adoc-graph adoc-graph-frame vp-raw\"></div>\n\n\
         {summary_md}\n\
         <script>\n\
         let graph = null\n\
         export default {{\n\
         \x20 mounted() {{\n\
         \x20   const base = (import.meta.env.BASE_URL || '/').replace(/\\/?$/, '/')\n\
         \x20   const assets = base + '{SITE_ASSETS_URL}'\n\
         \x20   const host = document.getElementById('adoc-graph')\n\
         \x20   if (!host) return\n\
         \x20   if (!document.querySelector('link[data-adoc-graph]')) {{\n\
         \x20     const css = document.createElement('link')\n\
         \x20     css.rel = 'stylesheet'\n\
         \x20     css.href = assets + 'graph.css'\n\
         \x20     css.setAttribute('data-adoc-graph', '')\n\
         \x20     document.head.appendChild(css)\n\
         \x20   }}\n\
         \x20   const start = () =>\n\
         \x20     fetch(assets + 'graph.json')\n\
         \x20       .then((r) => r.json())\n\
         \x20       .then((data) => {{\n\
         \x20         if (!document.body.contains(host)) return\n\
         \x20         graph = window.AdocGraph.mount(host, {{\n\
         \x20           t: data.strings,\n\
         \x20           onOpen: (node) => {{\n\
         \x20             if (!node.link) return false\n\
         \x20             window.location.assign(base.replace(/\\/$/, '') + node.link)\n\
         \x20           }},\n\
         \x20         }})\n\
         \x20         graph.setData(data)\n\
         \x20       }})\n\
         \x20       .catch((err) => console.error('archeology-doc: graph', err))\n\
         \x20   if (window.AdocGraph) start()\n\
         \x20   else {{\n\
         \x20     const js = document.createElement('script')\n\
         \x20     js.src = assets + 'graph.js'\n\
         \x20     js.onload = start\n\
         \x20     document.head.appendChild(js)\n\
         \x20   }}\n\
         \x20 }},\n\
         \x20 beforeUnmount() {{\n\
         \x20   if (graph) graph.destroy()\n\
         \x20   graph = null\n\
         \x20 }},\n\
         }}\n\
         </script>\n",
        intro = tl!(lang, "doc.graph.intro"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Node};
    use std::path::PathBuf;

    fn hostile_graph() -> Graph {
        Graph {
            nodes: vec![
                Node {
                    id: "findings/x".into(),
                    title: "</script><script>alert(1)</script><!-- & \u{2028}".into(),
                    kind: "finding".into(),
                    status: None,
                    severity: Some("high".into()),
                    state: Some("open".into()),
                    stale: false,
                    file: PathBuf::from("/p/knowledge/findings/x.md"),
                    line: None,
                },
                Node {
                    id: "code:src/a b.rs".into(),
                    title: "src/a b.rs".into(),
                    kind: "code".into(),
                    status: None,
                    severity: None,
                    state: None,
                    stale: false,
                    file: PathBuf::from("/p/src/a b.rs"),
                    line: Some(3),
                },
            ],
            edges: vec![Edge {
                source: "findings/x".into(),
                target: "code:src/a b.rs".into(),
                kind: "evidence".into(),
            }],
        }
    }

    #[test]
    fn script_json_cannot_break_out_of_the_script_block() {
        let g = hostile_graph();
        let json = script_json(&data(Path::new("/p"), &g, Links::Local));
        assert!(!json.contains("</script"), "{json}");
        assert!(!json.contains("<!--"), "{json}");
        assert!(!json.contains('<') && !json.contains('>') && !json.contains('&'));
        assert!(!json.contains('\u{2028}'));
        // Sigue siendo JSON válido con el mismo contenido.
        let back: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(back["nodes"][0]["title"], g.nodes[0].title.as_str());
        assert_eq!(back["nodes"][0]["path"], "knowledge/findings/x.md");
        assert_eq!(back["nodes"][1]["href"], "file:///p/src/a%20b.rs");

        let html = embed("g", &back, Lang::En, "n0nce", "frame");
        // Un solo cierre por bloque: datos, renderizador y arranque.
        assert_eq!(html.matches("</script>").count(), 3, "{html}");
        assert!(!html.contains("<script>alert"));
        assert!(!GRAPH_JS.contains("</script") && !GRAPH_JS.contains("<!--"));
        assert!(!GRAPH_CSS.contains("</style"));
    }

    #[test]
    fn site_data_links_notes_to_wiki_pages_without_local_paths() {
        let g = hostile_graph();
        let docs = Path::new("/p/docs");
        let v = data(
            Path::new("/p"),
            &g,
            Links::Site {
                docs,
                wiki: Some(WIKI_DIR),
            },
        );
        assert_eq!(v["nodes"][0]["link"], "/wiki/findings/x");
        assert!(v["nodes"][1].get("link").is_none(), "el código no enlaza");
        assert!(!v.to_string().contains("file://"));
        assert!(!v.to_string().contains("\"/p/"));
        let none = data(Path::new("/p"), &g, Links::Site { docs, wiki: None });
        assert!(none["nodes"][0].get("link").is_none());
    }

    #[test]
    fn strings_cover_both_languages() {
        let en = strings(Lang::En);
        let es = strings(Lang::Es);
        assert_eq!(en.keys().collect::<Vec<_>>(), es.keys().collect::<Vec<_>>());
        assert_eq!(es["kind.finding"], "Hallazgos");
        assert!(en["stats"].contains("{nodes}"));
        // Sin claves sin traducir.
        assert!(en
            .values()
            .chain(es.values())
            .all(|v| !v.starts_with("graph.")));
    }

    #[test]
    fn summary_markdown_escapes_titles() {
        let s = crate::graph::summary(&hostile_graph());
        let md = summary_markdown(&s, Lang::Es, |id| Some(format!("/wiki/{id}")));
        assert!(md.contains("Notas más conectadas"), "{md}");
        assert!(md.contains("&lt;/script&gt;"), "{md}");
        assert!(!md.contains("<script>"), "{md}");
        assert!(md.contains("](/wiki/findings/x)"), "{md}");
    }
}
