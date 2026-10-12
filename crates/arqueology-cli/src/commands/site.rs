//! `site` — sitio VitePress (§8.10).

use anyhow::Result;
use arqueology_core::Project;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum SiteCmd {
    /// Instala VitePress en el directorio de configuración, compartido por todos los proyectos.
    Setup {
        /// Versión de VitePress.
        #[arg(long, default_value = super::toolchain::VITEPRESS_VERSION)]
        version: String,
    },
    /// Regenera wiki, specs, sidebar y llms.txt sin lanzar VitePress.
    Sync,
    /// Servidor de desarrollo de VitePress.
    Dev,
    /// Build de producción de VitePress.
    Build,
}

/// Páginas del dashboard (`estado.md`, `hallazgos.md`, `preguntas.md`) y la
/// región `dashboard` de la portada. El resto de `index.md` es del usuario.
pub fn write_dashboard(project: &Project) -> Result<()> {
    let status = super::status::build(project)?;
    let wiki = project
        .config
        .site
        .include_knowledge
        .then_some(arqueology_core::site::WIKI_DIR);
    let pages = super::report::render_markdown(&status, wiki);
    let docs = project.docs_dir();
    std::fs::create_dir_all(&docs)?;
    for (name, content) in [
        ("estado.md", &pages.estado),
        ("hallazgos.md", &pages.hallazgos),
        ("preguntas.md", &pages.preguntas),
    ] {
        let path = docs.join(name);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
            std::fs::write(&path, content)?;
        }
    }
    let index = docs.join("index.md");
    let existing = arqueology_core::managed::read_or_default(&index, || {
        format!("# {}\n", project.config.project.name)
    })?;
    let locked = arqueology_core::frontmatter::NoteFrontmatter::parse(&existing)
        .map(|fm| fm.locked)
        .unwrap_or(false);
    if !locked {
        let updated = arqueology_core::managed::upsert_region(
            &existing,
            "dashboard",
            pages.portada.trim_end(),
        )?;
        if updated != existing {
            std::fs::write(&index, updated)?;
        }
    }
    Ok(())
}

/// `adoc preview`: sitio con dashboard en el navegador; sin VitePress, el
/// reporte HTML único.
pub fn preview(project: &Project, port: Option<u16>, no_open: bool) -> Result<()> {
    sync(project)?;
    if super::toolchain::find_vitepress(Some(project)).is_some() {
        let mut extra = Vec::new();
        if !no_open {
            extra.push("--open".to_string());
        }
        if let Some(p) = port {
            extra.push("--port".to_string());
            extra.push(p.to_string());
        }
        return super::toolchain::run_vitepress_with(project, "dev", &extra);
    }
    let out = crate::ui::with_spinner("Generando reporte HTML…", || {
        super::report::write_html(project)
    })?;
    println!(
        "VitePress no disponible (`adoc site setup` lo instala); mostrando el reporte HTML:\n  {}",
        out.display()
    );
    if !no_open {
        super::toolchain::open_in_browser(&out.display().to_string());
    }
    Ok(())
}

/// `site setup` no necesita un proyecto.
pub fn setup(version: &str) -> Result<()> {
    let vp = super::toolchain::setup(version)?;
    println!(
        "vitepress {} instalado en {}",
        vp.version.as_deref().unwrap_or(version),
        vp.package_dir.display()
    );
    Ok(())
}

pub fn run(project: &Project, cmd: SiteCmd) -> Result<()> {
    match cmd {
        SiteCmd::Setup { version } => setup(&version),
        SiteCmd::Sync => sync(project),
        SiteCmd::Dev => {
            sync(project)?;
            super::toolchain::run_vitepress(project, "dev")
        }
        SiteCmd::Build => {
            sync(project)?;
            super::toolchain::run_vitepress(project, "build")
        }
    }
}

/// Prepara `docs/` (RF-112 a RF-115, RF-164).
pub fn sync(project: &Project) -> Result<()> {
    let vp = project.docs_dir().join(".vitepress");
    if !vp.join("config.mts").is_file() && !vp.join("config.ts").is_file() {
        anyhow::bail!("no hay docs/.vitepress/config.mts; ejecuta `adoc init` primero");
    }
    let report = crate::ui::with_spinner(
        "Preparando sitio: wiki, specs, dashboard, sidebar y llms.txt…",
        || -> Result<arqueology_core::site::SiteReport> {
            write_dashboard(project)?;
            Ok(arqueology_core::site::prepare(project)?)
        },
    )?;
    eprintln!(
        "sitio: {} páginas en el sidebar ({} del vault, {} specs)",
        report.sidebar_items, report.wiki_pages, report.spec_pages
    );
    if !report.unresolved_links.is_empty() {
        eprintln!(
            "aviso: wikilinks sin destino (quedan como texto): {}",
            report.unresolved_links.join(", ")
        );
    }
    Ok(())
}
