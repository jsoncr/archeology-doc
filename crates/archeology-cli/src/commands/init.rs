//! `init` — crea `archeology-doc.toml`, `.archeology-doc/` y la estructura
//! de salida (RF-001 a RF-006).

use anyhow::{bail, Context, Result};
use archeology_core::config::{AdapterConfig, AdapterMode, ArcheologyConfig};
use archeology_core::i18n::{self, Lang, LangSource};
use archeology_core::templates;
use archeology_core::Project;
use archeology_core::{t, tl};
use clap::Args;
use std::fs;
use std::path::Path;

#[derive(Args)]
pub struct InitArgs {
    /// Valores por defecto sin preguntar (para CI).
    #[arg(long)]
    yes: bool,
    /// Agente por defecto (claude, codex, manual, ...).
    #[arg(long)]
    agent: Option<String>,
    // El idioma de la documentación es el flag global `--lang` (en|es); sin
    // él, el idioma de la interfaz resuelto (ADOC_LANG, config, locale, en).
    /// Nombre del proyecto (por defecto: nombre del directorio).
    #[arg(long)]
    name: Option<String>,
    /// Adaptadores de contexto (claude,cursor,copilot). Por defecto, según el agente.
    #[arg(long, value_delimiter = ',')]
    adapters: Option<Vec<String>>,
    /// Directorio del proyecto (por defecto: actual).
    #[arg(long)]
    dir: Option<String>,
}

pub fn run(args: InitArgs) -> Result<()> {
    let dir = args
        .dir
        .as_deref()
        .map(Path::new)
        .map(|p| p.to_path_buf())
        .unwrap_or(std::env::current_dir()?);
    let dir = archeology_core::paths::canonicalize(&dir).unwrap_or(dir);
    let cfg_path = dir.join(archeology_core::config::CONFIG_FILE);
    if cfg_path.exists() {
        bail!(t!("cli.init.already_initialized"));
    }

    // Idioma de la documentación: `--lang` explícito (inválido = error, no
    // solo aviso) o, si no, el idioma de la interfaz resuelto.
    let resolution = i18n::ui_resolution();
    if let Some((_, value)) = resolution
        .invalid
        .iter()
        .find(|(source, _)| *source == LangSource::Flag)
    {
        bail!(t!("core.i18n.invalid_lang", value = value));
    }
    let default_lang = resolution.lang;

    let default_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());

    // Detección de agentes instalados (RF-003).
    let mut config = ArcheologyConfig::new(&default_name, default_lang.code());
    let installed = archeology_agents::adapter::detect_installed(&config.agent.adapters);
    let suggested = installed
        .iter()
        .find(|a| *a != "manual")
        .cloned()
        .unwrap_or_else(|| "manual".into());

    let (name, agent, language) = if args.yes {
        (
            args.name.clone().unwrap_or(default_name),
            args.agent.clone().unwrap_or(suggested),
            default_lang,
        )
    } else {
        eprintln!("{}", t!("cli.init.initializing", path = dir.display()));
        if !installed.is_empty() {
            eprintln!(
                "{}",
                t!("cli.init.detected_agents", agents = installed.join(", "))
            );
        }
        let name = super::ask(
            &t!("cli.init.ask_name"),
            args.name.as_deref().unwrap_or(&default_name),
        )?;
        let agent = super::ask(
            &t!("cli.init.ask_agent"),
            args.agent.as_deref().unwrap_or(&suggested),
        )?;
        let answer = super::ask(&t!("cli.init.ask_language"), default_lang.code())?;
        let language = Lang::parse(&answer)
            .ok_or_else(|| anyhow::anyhow!(t!("core.i18n.invalid_lang", value = answer)))?;
        (name, agent, language)
    };

    if !config.agent.adapters.contains_key(&agent) {
        eprintln!("{}", t!("cli.init.unknown_adapter", agent = agent));
        // Lo dejamos como manual para no romper inspect.
        config.agent.adapters.insert(
            agent.clone(),
            AdapterConfig {
                mode: AdapterMode::Export,
                ..AdapterConfig::manual()
            },
        );
    }

    config.project.name = name;
    // Siempre explícito en el archivo: sin el campo se asume `es`.
    config.project.language = language.code().to_string();
    // Adaptadores de contexto (RF-101): CLAUDE.md si se usa Claude Code.
    config.context.adapters = match &args.adapters {
        Some(list) => list.clone(),
        None if agent == "claude" => vec!["claude".to_string()],
        None => Vec::new(),
    };
    config.agent.default = agent;
    config.validate()?;

    // RF-005: no sobrescribir sin confirmación.
    if !args.yes && cfg_path.exists() && !super::confirm(&t!("cli.init.confirm_overwrite"), false)?
    {
        bail!(t!("cli.init.cancelled"));
    }

    config.save(&cfg_path)?;
    eprintln!("{}", t!("cli.init.created", path = cfg_path.display()));

    // Estructura de salida (§10).
    let project = Project::open(&dir)?;
    let docs_preexisting = project.docs_dir().exists();
    for d in [
        project.state_dir(),
        project.tasks_dir(),
        project.runs_dir(),
        project.knowledge_dir().join("modules"),
        project.knowledge_dir().join("features"),
        project.knowledge_dir().join("findings"),
        project.knowledge_dir().join("tests"),
        project.knowledge_dir().join("specs"),
        project.knowledge_dir().join("glossary"),
        project.knowledge_dir().join("decisions"),
        project.knowledge_dir().join("memory").join("sessions"),
        project.knowledge_dir().join("_templates"),
    ] {
        // Las carpetas del repositorio se crean sin atravesar symlinks.
        let created = if project.contains(&d) {
            archeology_core::fsutil::create_dir_in_root(&project.root, &d).map_err(Into::into)
        } else {
            fs::create_dir_all(&d).map_err(anyhow::Error::from)
        };
        created.with_context(|| t!("cli.init.creating", path = d.display()))?;
    }

    // Contenido generado en el idioma de la documentación del proyecto.
    let lang = project.doc_lang();

    // Plantillas de notas (RF-065).
    for name in templates::NOTE_TEMPLATES {
        if let Some(content) = templates::template(lang, name) {
            archeology_core::fsutil::write_in_root(
                &project.root,
                &project.knowledge_dir().join("_templates").join(name),
                content.as_bytes(),
            )?;
        }
    }

    // Memoria inicial (RF-102, RF-103): estable aquí, temporal en sessions/.
    let mem = project.memory_dir();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    for (slug, key) in [
        ("facts", "doc.memory.facts"),
        ("decisions", "doc.memory.decisions"),
        ("conventions", "doc.memory.conventions"),
        ("open-questions", "doc.memory.open_questions"),
    ] {
        let path = mem.join(format!("{slug}.md"));
        if fs::symlink_metadata(&path).is_err() {
            let title = tl!(lang, key);
            archeology_core::fsutil::write_in_root(
                &project.root,
                &path,
                templates::render(
                    lang,
                    "memory.md",
                    &[("slug", slug), ("title", &title), ("date", &today)],
                )?
                .as_bytes(),
            )?;
        }
    }

    // AGENTS.md, adaptadores por herramienta: regiones gestionadas (RF-100, RF-162).
    for f in archeology_core::context::refresh(&project, None)? {
        eprintln!("{}", t!("cli.init.updated", path = f));
    }

    // Sitio VitePress básico (RF-112).
    if config.site.enabled {
        scaffold_site(&project)?;
        if !docs_preexisting {
            fs::create_dir_all(project.state_dir())?;
            archeology_core::fsutil::atomic_write(
                &project
                    .state_dir()
                    .join(archeology_scan::scanner::GENERATED_DOCS_MARK),
                format!("{}\n", t!("cli.init.docs_mark")).as_bytes(),
            )?;
        }
        // Un `docs/wiki` o `docs/specs` del usuario no detiene `init`.
        if let Err(e) = archeology_core::site::prepare(&project) {
            eprintln!("{}", t!("cli.init.site_not_prepared", error = e));
        }
    }

    // llms.txt (RF-115).
    archeology_core::site::write_llms_txt(&project)?;

    // Registro global (RF-171).
    archeology_registry::Registry::update(|reg| {
        reg.upsert(&config.project.id, &config.project.name, &dir, "init")
    })?;
    eprintln!(
        "{}",
        t!(
            "cli.init.registered",
            path = archeology_registry::registry_path()?.display()
        )
    );

    println!("{}", t!("cli.init.done", path = dir.display()));
    println!(
        "{}",
        t!("cli.init.work_state", path = project.state_dir().display())
    );
    println!("{}", t!("cli.init.doc_language", lang = lang));
    println!("{}", t!("cli.init.next_step"));
    Ok(())
}

fn scaffold_site(project: &Project) -> Result<()> {
    let docs = project.docs_dir();
    for sub in [
        "tutorials",
        "guides",
        "reference",
        "explanation",
        "user-manual",
        ".vitepress",
    ] {
        archeology_core::fsutil::create_dir_in_root(&project.root, &docs.join(sub))?;
    }
    // La copia del vault, la caché y el build son salida derivada.
    let ignore = docs.join(".gitignore");
    if fs::symlink_metadata(&ignore).is_err() {
        archeology_core::fsutil::write_in_root(
            &project.root,
            &ignore,
            format!(
                "{}/\n.vitepress/cache/\n.vitepress/dist/\n",
                archeology_core::site::WIKI_DIR
            )
            .as_bytes(),
        )?;
    }
    let index = docs.join("index.md");
    if fs::symlink_metadata(&index).is_err() {
        archeology_core::fsutil::write_in_root(
            &project.root,
            &index,
            format!(
                "---\nlayout: home\ntitle: {}\n---\n\n# {}\n\n{}\n",
                project.config.project.name,
                project.config.project.name,
                tl!(project.doc_lang(), "doc.site.index_intro")
            )
            .as_bytes(),
        )?;
    }
    // `.mts` funciona tanto en paquetes CommonJS como ESM.
    let config_ts = docs.join(".vitepress").join("config.mts");
    if !config_ts.exists() && !docs.join(".vitepress").join("config.ts").exists() {
        archeology_core::fsutil::write_in_root(
            &project.root,
            &config_ts,
            templates::render(
                project.doc_lang(),
                "vitepress-config.ts",
                &[
                    ("project_name", &project.config.project.name),
                    ("language", &project.config.project.language),
                ],
            )?
            .as_bytes(),
        )?;
    }
    Ok(())
}
