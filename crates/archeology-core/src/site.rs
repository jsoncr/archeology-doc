//! Preparación del sitio VitePress y de `llms.txt` (§8.10).
//!
//! - Copia el vault a `docs/wiki/` y las specs a `docs/specs/`, convirtiendo
//!   wikilinks a enlaces estándar (RF-113, RF-114, RF-164).
//! - Genera `docs/.vitepress/sidebar.generated.json` a partir de los archivos
//!   existentes (RF-112); `config.mts` lo importa y sigue siendo del usuario.
//! - Mantiene el índice de `llms.txt` en una región gestionada (RF-115).

use crate::error::{Error, Result};
use crate::frontmatter::{extract_frontmatter, NoteFrontmatter};
use crate::managed;
use crate::project::Project;
use crate::{t, tl};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Carpeta del sitio con la copia del vault.
pub const WIKI_DIR: &str = "wiki";
/// Carpeta del sitio con la copia de las specs.
pub const SPECS_DIR: &str = "specs";
/// Archivo que marca una carpeta del sitio como generada (se puede borrar).
pub const GENERATED_MARK: &str = ".generated-by-archeology-doc";
/// Marca con el nombre anterior del proyecto.
const LEGACY_GENERATED_MARK: &str = ".generated-by-arqueology-doc";
/// Archivo en el espacio de trabajo que indica que `docs/` lo creó `init`.
pub const GENERATED_DOCS_MARK: &str = "generated-docs";
/// Sidebar generado, importado por `docs/.vitepress/config.mts`.
pub const SIDEBAR_FILE: &str = "sidebar.generated.json";

/// Secciones Diátaxis del sitio (RF-110), en orden. El título es la clave
/// `doc.site.section.<carpeta>` en el idioma de la documentación.
const SECTIONS: &[&str] = &[
    "tutorials",
    "guides",
    "user-manual",
    "reference",
    "explanation",
];

#[derive(Debug, Default)]
pub struct SiteReport {
    pub wiki_pages: usize,
    pub spec_pages: usize,
    pub sidebar_items: usize,
    /// Wikilinks sin destino, convertidos a texto plano.
    pub unresolved_links: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SidebarGroup {
    pub text: String,
    pub collapsed: bool,
    pub items: Vec<SidebarItem>,
}

#[derive(Debug, Serialize)]
pub struct SidebarItem {
    pub text: String,
    pub link: String,
}

/// Prepara `docs/` para `vitepress dev|build`.
pub fn prepare(project: &Project) -> Result<SiteReport> {
    let docs = project.docs_dir();
    let mut report = SiteReport::default();
    let lang = project.doc_lang();

    // El vault y las specs se copian de cero: son salida derivada.
    let wiki = docs.join(WIKI_DIR);
    let specs_out = docs.join(SPECS_DIR);
    let sources = [
        ("paths.knowledge", project.knowledge_dir()),
        ("openspec.path", project.config.openspec_dir(&project.root)),
    ];
    for out in [&wiki, &specs_out] {
        for (key, src) in &sources {
            if src.starts_with(out) || out.starts_with(src) {
                return Err(Error::InvalidConfig(t!(
                    "core.site.overlap",
                    key = key,
                    path = project.rel(src),
                    output = project.rel(out)
                )));
            }
        }
    }
    clear_generated(project, &wiki)?;
    if project.config.site.include_knowledge && project.knowledge_dir().is_dir() {
        report.wiki_pages = copy_vault(project, &wiki, &mut report.unresolved_links)?;
        mark_generated(project, &wiki)?;
    }
    clear_generated(project, &specs_out)?;
    let specs_src = project.config.openspec_dir(&project.root).join("specs");
    if project.config.site.include_specs && specs_src.is_dir() {
        report.spec_pages = copy_specs(&project.root, &specs_src, &specs_out)?;
        mark_generated(project, &specs_out)?;
    }

    let mut groups = Vec::new();
    // Dashboard generado por `adoc site sync` (estado, hallazgos, preguntas).
    // Los nombres de archivo no se traducen: los genera `adoc site sync`.
    let dashboard: Vec<SidebarItem> = [
        ("estado", "doc.site.status"),
        ("hallazgos", "doc.site.findings"),
        ("preguntas", "doc.site.questions"),
        (crate::graph_web::SITE_PAGE, "doc.graph.title"),
    ]
    .iter()
    .filter(|(f, _)| docs.join(format!("{f}.md")).is_file())
    .map(|(f, key)| SidebarItem {
        text: tl!(lang, key),
        link: format!("/{f}"),
    })
    .collect();
    if !dashboard.is_empty() {
        groups.push(SidebarGroup {
            text: tl!(lang, "doc.site.overview"),
            collapsed: false,
            items: dashboard,
        });
    }
    for dir in SECTIONS {
        push_group(
            &mut groups,
            &docs,
            dir,
            &tl!(lang, &format!("doc.site.section.{dir}")),
            false,
        );
    }
    push_group(
        &mut groups,
        &docs,
        WIKI_DIR,
        &tl!(lang, "doc.site.knowledge_base"),
        true,
    );
    push_group(
        &mut groups,
        &docs,
        SPECS_DIR,
        &tl!(lang, "doc.site.specifications"),
        true,
    );
    report.sidebar_items = groups.iter().map(|g| g.items.len()).sum();

    let vp = docs.join(".vitepress");
    write_if_changed(
        &project.root,
        &vp.join(SIDEBAR_FILE),
        &serde_json::to_string_pretty(&groups)?,
    )?;

    write_llms_txt(project)?;
    Ok(report)
}

fn push_group(groups: &mut Vec<SidebarGroup>, docs: &Path, dir: &str, text: &str, collapsed: bool) {
    let items = section_items(docs, dir);
    if !items.is_empty() {
        groups.push(SidebarGroup {
            text: text.to_string(),
            collapsed,
            items,
        });
    }
}

/// Páginas de una sección, ordenadas por ruta, con su título.
fn section_items(docs: &Path, dir: &str) -> Vec<SidebarItem> {
    let mut files = Vec::new();
    collect_md(&docs.join(dir), &mut files);
    files.sort();
    files
        .iter()
        .map(|f| SidebarItem {
            text: page_title(f),
            link: page_link(docs, f),
        })
        .collect()
}

/// Enlace VitePress de una página: `/guides/setup`, `/guides/` para index.md.
pub fn page_link(docs: &Path, file: &Path) -> String {
    let rel = file
        .strip_prefix(docs)
        .unwrap_or(file)
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/");
    match rel.strip_suffix("index") {
        Some(dir) if dir.is_empty() || dir.ends_with('/') => format!("/{dir}"),
        _ => format!("/{rel}"),
    }
}

/// Título de una página: frontmatter `title`, primer `# ` o nombre de archivo.
pub fn page_title(file: &Path) -> String {
    let text = fs::read_to_string(file).unwrap_or_default();
    if let Some(t) = NoteFrontmatter::parse(&text).and_then(|fm| fm.title) {
        return t;
    }
    let body = extract_frontmatter(&text).map(|(_, b)| b).unwrap_or(&text);
    body.lines()
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        .unwrap_or_else(|| {
            file.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        })
}

fn copy_vault(project: &Project, out: &Path, unresolved: &mut Vec<String>) -> Result<usize> {
    let knowledge = project.knowledge_dir();
    let mut notes = Vec::new();
    collect_md(&knowledge, &mut notes);
    notes.retain(|n| !n.starts_with(knowledge.join("_templates")));

    // Destinos de wikilinks: ruta relativa sin extensión y nombre de archivo.
    let mut targets: BTreeMap<String, String> = BTreeMap::new();
    for n in &notes {
        let rel = n
            .strip_prefix(&knowledge)
            .unwrap_or(n)
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let link = format!("/{WIKI_DIR}/{rel}");
        let stem = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        targets.entry(stem).or_insert_with(|| link.clone());
        targets.insert(rel, link);
    }

    for n in &notes {
        let rel = n.strip_prefix(&knowledge).unwrap_or(n);
        let text = fs::read_to_string(n)?;
        let converted = convert_wikilinks(&text, |t| targets.get(t).cloned(), unresolved);
        let converted = escape_vue_interpolation(&neutralize_external_links(&converted));
        let dest = out.join(rel);
        crate::fsutil::write_in_root(&project.root, &dest, converted.as_bytes())?;
    }
    Ok(notes.len())
}

fn copy_specs(root: &Path, src: &Path, out: &Path) -> Result<usize> {
    let mut n = 0;
    for e in fs::read_dir(src)?.flatten() {
        // Sin seguir symlinks: ni la capability ni su spec.md.
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let spec = e.path().join("spec.md");
        if fs::symlink_metadata(&spec).is_ok_and(|m| m.is_file()) {
            let text = fs::read_to_string(&spec)?;
            crate::fsutil::write_in_root(
                root,
                &out.join(format!("{}.md", e.file_name().to_string_lossy())),
                escape_vue_interpolation(&neutralize_external_links(&text)).as_bytes(),
            )?;
            n += 1;
        }
    }
    Ok(n)
}

/// Convierte `[[destino]]`, `[[destino|texto]]` y `[[destino#sección]]` en
/// enlaces Markdown. Los destinos sin resolver quedan como texto plano.
/// No toca bloques de código.
pub fn convert_wikilinks(
    text: &str,
    resolve: impl Fn(&str) -> Option<String>,
    unresolved: &mut Vec<String>,
) -> String {
    map_outside_code(text, |segment| {
        let mut out = String::with_capacity(segment.len());
        let mut rest = segment;
        while let Some(start) = rest.find("[[") {
            let Some(end) = rest[start + 2..].find("]]") else {
                break;
            };
            out.push_str(&rest[..start]);
            let inner = &rest[start + 2..start + 2 + end];
            let (target, label) = match inner.split_once('|') {
                Some((t, l)) => (t.trim(), l.trim()),
                None => (inner.trim(), inner.trim()),
            };
            let (page, anchor) = match target.split_once('#') {
                Some((p, a)) => (p.trim(), Some(a.trim())),
                None => (target, None),
            };
            let page = page.trim_end_matches(".md");
            match resolve(page) {
                Some(link) => {
                    let anchor = anchor
                        .map(|a| format!("#{}", slug_anchor(a)))
                        .unwrap_or_default();
                    out.push_str(&format!("[{label}]({link}{anchor})"));
                }
                None => {
                    if !unresolved.iter().any(|u| u == target) {
                        unresolved.push(target.to_string());
                    }
                    out.push_str(label);
                }
            }
            rest = &rest[start + 2 + end + 2..];
        }
        out.push_str(rest);
        out
    })
}

/// Enlaces Markdown relativos a archivos fuera del sitio (p. ej. evidencias
/// `../src/a.rs`) se convierten en código en línea para no romper el build.
pub fn neutralize_external_links(text: &str) -> String {
    map_outside_code(text, |segment| {
        let mut out = String::with_capacity(segment.len());
        let mut rest = segment;
        while let Some(mid) = rest.find("](") {
            let Some(close) = rest[mid + 2..].find(')') else {
                break;
            };
            let Some(open) = rest[..mid].rfind('[') else {
                out.push_str(&rest[..mid + 2]);
                rest = &rest[mid + 2..];
                continue;
            };
            let label = &rest[open + 1..mid];
            let href = &rest[mid + 2..mid + 2 + close];
            let keep = href.starts_with('/')
                || href.starts_with('#')
                || href.contains("://")
                || href.starts_with("mailto:")
                || (href.ends_with(".md") && !href.starts_with(".."));
            out.push_str(&rest[..open]);
            if keep {
                out.push_str(&rest[open..mid + 2 + close + 1]);
            } else {
                out.push_str(&format!("{label} (`{href}`)"));
            }
            rest = &rest[mid + 2 + close + 1..];
        }
        out.push_str(rest);
        out
    })
}

/// Escapa `{{` fuera de código para que Vue no lo interprete.
pub fn escape_vue_interpolation(text: &str) -> String {
    map_outside_code(text, |s| s.replace("{{", "&#123;&#123;"))
}

/// Aplica `f` al texto fuera de bloques ``` y de código en línea `...`.
fn map_outside_code(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        for (i, part) in line.split('`').enumerate() {
            if i > 0 {
                out.push('`');
            }
            if i % 2 == 0 {
                out.push_str(&f(part));
            } else {
                out.push_str(part);
            }
        }
    }
    out
}

/// Ancla de encabezado al estilo VitePress (minúsculas, guiones).
fn slug_anchor(heading: &str) -> String {
    let mut out = String::new();
    for c in heading.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if (c == ' ' || c == '-') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Índice de `llms.txt` en una región gestionada (RF-115).
pub fn write_llms_txt(project: &Project) -> Result<()> {
    let path = project.root.join("llms.txt");
    crate::fsutil::check_no_symlinks(&project.root, &path)?;
    let lang = project.doc_lang();
    let existing = managed::read_or_default(&path, || {
        format!(
            "# {}\n\n> {}\n",
            project.config.project.name,
            tl!(lang, "doc.llms.intro")
        )
    })?;
    let mut body = String::new();
    let mut section = |title: &str, files: Vec<PathBuf>| {
        if files.is_empty() {
            return;
        }
        body.push_str(&format!("## {title}\n\n"));
        for f in files {
            body.push_str(&format!("- [{}]({})\n", page_title(&f), project.rel(&f)));
        }
        body.push('\n');
    };

    let knowledge = project.knowledge_dir();
    let mut vault = Vec::new();
    collect_md(&knowledge, &mut vault);
    vault.retain(|n| {
        !n.starts_with(knowledge.join("_templates"))
            && !n.starts_with(knowledge.join("memory").join("sessions"))
    });
    vault.sort();
    section(&tl!(lang, "doc.llms.knowledge"), vault);

    let docs = project.docs_dir();
    let mut pages = Vec::new();
    for dir in SECTIONS {
        collect_md(&docs.join(dir), &mut pages);
    }
    pages.sort();
    section(&tl!(lang, "doc.llms.documentation"), pages);

    let specs = project.config.openspec_dir(&project.root).join("specs");
    let mut spec_files = Vec::new();
    collect_md(&specs, &mut spec_files);
    spec_files.sort();
    section(&tl!(lang, "doc.llms.specifications"), spec_files);

    let updated = managed::upsert_region(&existing, "index", body.trim_end())?;
    write_if_changed(&project.root, &path, &updated)
}

/// Borra una carpeta del sitio solo si la generó archeology-doc: debe tener
/// su propia marca ([`GENERATED_MARK`] o la anterior). La marca del espacio
/// de trabajo ([`GENERATED_DOCS_MARK`], `docs/` creado por `init`) no basta:
/// el usuario pudo crear `docs/wiki/` a mano después. Nunca sigue un symlink.
fn clear_generated(project: &Project, dir: &Path) -> Result<()> {
    let Ok(meta) = fs::symlink_metadata(dir) else {
        return Ok(());
    };
    let is_mark =
        |name: &str| fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.file_type().is_file());
    let ours = meta.is_dir() && (is_mark(GENERATED_MARK) || is_mark(LEGACY_GENERATED_MARK));
    if !ours {
        return Err(Error::Validation(t!(
            "core.site.not_generated",
            path = project.rel(dir)
        )));
    }
    fs::remove_dir_all(dir)?;
    Ok(())
}

fn mark_generated(project: &Project, dir: &Path) -> Result<()> {
    if fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        crate::fsutil::write_in_root(
            &project.root,
            &dir.join(GENERATED_MARK),
            format!("{}\n", tl!(project.doc_lang(), "doc.site.generated_mark")).as_bytes(),
        )?;
    }
    Ok(())
}

/// Escritura segura (sin symlinks, atómica) solo si el contenido cambió.
fn write_if_changed(root: &Path, path: &Path, content: &str) -> Result<()> {
    crate::fsutil::write_if_changed_in_root(root, path, content)?;
    Ok(())
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        // Sin seguir symlinks: evita ciclos y leer fuera del árbol.
        let kind = e.file_type().ok();
        if kind.is_some_and(|t| t.is_dir()) {
            collect_md(&p, out);
        } else if kind.is_some_and(|t| t.is_file())
            && p.extension().map(|x| x == "md").unwrap_or(false)
        {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_wikilinks_outside_code() {
        let resolve = |t: &str| (t == "billing").then(|| "/wiki/modules/billing".to_string());
        let mut unresolved = Vec::new();
        let out = convert_wikilinks(
            "Ver [[billing|Facturación]] y [[billing#Reglas de negocio]], [[nada]].\n```\n[[billing]]\n```\n`[[billing]]`\n",
            resolve,
            &mut unresolved,
        );
        assert_eq!(
            out,
            "Ver [Facturación](/wiki/modules/billing) y [billing#Reglas de negocio](/wiki/modules/billing#reglas-de-negocio), nada.\n```\n[[billing]]\n```\n`[[billing]]`\n"
        );
        assert_eq!(unresolved, vec!["nada"]);
    }

    #[test]
    fn neutralizes_links_outside_site() {
        let out = neutralize_external_links("[a](../src/a.rs) [b](/wiki/x) [c](https://x.y)");
        assert_eq!(out, "a (`../src/a.rs`) [b](/wiki/x) [c](https://x.y)");
        assert_eq!(
            escape_vue_interpolation("{{ x }} `{{ y }}`"),
            "&#123;&#123; x }} `{{ y }}`"
        );
    }

    #[test]
    fn page_links_follow_vitepress_routes() {
        let docs = Path::new("/d");
        assert_eq!(page_link(docs, Path::new("/d/guides/index.md")), "/guides/");
        assert_eq!(
            page_link(docs, Path::new("/d/guides/setup.md")),
            "/guides/setup"
        );
    }
}
