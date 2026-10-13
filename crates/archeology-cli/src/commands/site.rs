//! `site` — sitio VitePress (§8.10).

use anyhow::Result;
use archeology_core::t;
use archeology_core::Project;
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
    Dev {
        /// Aprueba ejecutar la configuración de VitePress del proyecto (JavaScript del repositorio).
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Build de producción de VitePress.
    Build {
        /// Aprueba ejecutar la configuración de VitePress del proyecto (JavaScript del repositorio).
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// Páginas del dashboard (`estado.md`, `hallazgos.md`, `preguntas.md`,
/// `grafo.md` con sus recursos en `public/adoc-graph/`) y la región
/// `dashboard` de la portada. El resto de `index.md` es del usuario.
pub fn write_dashboard(project: &Project) -> Result<()> {
    let status = super::status::build(project)?;
    let wiki = project
        .config
        .site
        .include_knowledge
        .then_some(archeology_core::site::WIKI_DIR);
    let pages = super::report::render_markdown(&status, wiki, project.doc_lang());
    let docs = project.docs_dir();
    archeology_core::fsutil::create_dir_in_root(&project.root, &docs)?;
    for (name, content) in [
        ("estado.md", &pages.estado),
        ("hallazgos.md", &pages.hallazgos),
        ("preguntas.md", &pages.preguntas),
    ] {
        let path = docs.join(name);
        archeology_core::fsutil::write_if_changed_in_root(&project.root, &path, content)?;
    }
    // Grafo del conocimiento: `grafo.md` + `public/adoc-graph/` (renderizador y datos).
    archeology_core::graph_web::write_site(project, &archeology_core::graph::build(project))?;
    let portada = format!(
        "{} · {}\n",
        pages.portada.trim_end(),
        archeology_core::tl!(project.doc_lang(), "doc.graph.cover_link")
    );
    let index = docs.join("index.md");
    let existing = archeology_core::managed::read_or_default(&index, || {
        format!("# {}\n", project.config.project.name)
    })?;
    let locked = archeology_core::frontmatter::NoteFrontmatter::parse(&existing)
        .map(|fm| fm.locked)
        .unwrap_or(false);
    if !locked {
        let updated =
            archeology_core::managed::upsert_region(&existing, "dashboard", portada.trim_end())?;
        if updated != existing {
            archeology_core::fsutil::write_in_root(&project.root, &index, updated.as_bytes())?;
        }
    }
    Ok(())
}

/// `adoc preview`: sitio con dashboard en el navegador; sin VitePress, el
/// reporte HTML único.
pub fn preview(project: &Project, port: Option<u16>, no_open: bool, yes: bool) -> Result<()> {
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
        return super::toolchain::run_vitepress_with(project, "dev", &extra, yes);
    }
    let out = crate::ui::with_spinner(t!("cli.site.generating_html"), || {
        super::report::write_html(project)
    })?;
    println!("{}", t!("cli.site.no_vitepress", path = out.display()));
    if !no_open {
        super::toolchain::open_in_browser(&out.display().to_string());
    }
    Ok(())
}

/// `site setup` no necesita un proyecto.
pub fn setup(version: &str) -> Result<()> {
    let vp = super::toolchain::setup(version)?;
    println!(
        "{}",
        t!(
            "cli.site.installed",
            version = vp.version.as_deref().unwrap_or(version),
            path = vp.package_dir.display()
        )
    );
    Ok(())
}

pub fn run(project: &Project, cmd: SiteCmd) -> Result<()> {
    match cmd {
        SiteCmd::Setup { version } => setup(&version),
        SiteCmd::Sync => sync(project),
        SiteCmd::Dev { yes } => {
            sync(project)?;
            super::toolchain::run_vitepress(project, "dev", yes)
        }
        SiteCmd::Build { yes } => {
            sync(project)?;
            super::toolchain::run_vitepress(project, "build", yes)
        }
    }
}

/// Prepara `docs/` (RF-112 a RF-115, RF-164).
pub fn sync(project: &Project) -> Result<()> {
    let vp = project.docs_dir().join(".vitepress");
    if !vp.join("config.mts").is_file() && !vp.join("config.ts").is_file() {
        anyhow::bail!(t!("cli.site.no_config"));
    }
    let report = crate::ui::with_spinner(
        t!("cli.site.preparing"),
        || -> Result<archeology_core::site::SiteReport> {
            write_dashboard(project)?;
            Ok(archeology_core::site::prepare(project)?)
        },
    )?;
    eprintln!(
        "{}",
        t!(
            "cli.site.summary",
            items = report.sidebar_items,
            wiki = report.wiki_pages,
            specs = report.spec_pages
        )
    );
    if !report.unresolved_links.is_empty() {
        eprintln!(
            "{}",
            t!(
                "cli.site.unresolved_links",
                links = report.unresolved_links.join(", ")
            )
        );
    }
    Ok(())
}
