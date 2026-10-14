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
    /// Reinicializa un proyecto ya inicializado (conserva su ID).
    #[arg(long)]
    force: bool,
    /// Asigna un ID nuevo (carpeta copiada o clon de un proyecto registrado).
    #[arg(long)]
    new_id: bool,
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
    let registry = archeology_registry::Registry::load()?;
    // Duplicados: carpeta ya inicializada o ya registrada (ID que se conserva con --force).
    let mut keep_id: Option<String> = None;
    if fs::symlink_metadata(&cfg_path).is_ok() {
        let existing = ArcheologyConfig::load(&cfg_path);
        if args.new_id {
            return regenerate_id(&dir, &cfg_path, existing?);
        }
        if !args.force {
            return Err(already_initialized(&dir, existing, &registry));
        }
        keep_id = existing.ok().map(|c| c.project.id);
    } else if let Some(entry) = registry.find_by_path(&dir) {
        if !args.force {
            bail!(t!(
                "projects.init.registered_at_path",
                name = entry.name,
                id = entry.id,
                path = dir.display()
            ));
        }
        keep_id = Some(entry.id.clone());
    }
    if args.new_id {
        keep_id = None;
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

    // Un nombre ya registrado en otra ruta haría ambiguo `--project <nombre>`.
    check_name_conflict(&registry, &name, &dir, keep_id.as_deref())?;
    if let Some(id) = keep_id {
        config.project.id = id;
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

/// Error de `init` en una carpeta que ya tiene `archeology-doc.toml`. Antes
/// mantiene el registro coherente: registra la carpeta si falta o la
/// re-vincula si se movió; si el mismo ID está registrado en otra carpeta
/// que sigue existiendo (copia o clon), solo avisa y sugiere `--new-id`.
fn already_initialized(
    dir: &Path,
    existing: archeology_core::Result<ArcheologyConfig>,
    registry: &archeology_registry::Registry,
) -> anyhow::Error {
    let cfg = match existing {
        Ok(cfg) => cfg,
        Err(e) => {
            return anyhow::anyhow!(t!(
                "projects.init.already_initialized_unreadable",
                path = dir.display(),
                error = e
            ))
        }
    };
    let (id, name) = (&cfg.project.id, &cfg.project.name);
    let dir_text = archeology_registry::canonical_string(dir);
    let by_id = registry.projects.iter().find(|p| &p.id == id);
    match by_id {
        Some(other) if other.path != dir_text && Path::new(&other.path).is_dir() => {
            eprintln!(
                "{}",
                t!(
                    "projects.init.same_id_elsewhere",
                    id = id,
                    path = other.path
                )
            );
        }
        Some(other) if other.path == dir_text => {}
        // Sin registrar, o la carpeta registrada ya no existe (se movió).
        _ => {
            let updated =
                archeology_registry::Registry::update(|reg| reg.upsert(id, name, dir, "init"));
            if updated.is_ok() {
                eprintln!("{}", t!("projects.init.registered_now"));
            }
        }
    }
    anyhow::anyhow!(t!(
        "projects.init.already_initialized",
        name = name,
        id = id,
        path = dir.display()
    ))
}

/// Otro proyecto registrado con el mismo nombre en otra ruta: error con
/// las salidas posibles (`--name`, o `projects delete` si ya no existe).
fn check_name_conflict(
    registry: &archeology_registry::Registry,
    name: &str,
    dir: &Path,
    own_id: Option<&str>,
) -> Result<()> {
    let dir_text = archeology_registry::canonical_string(dir);
    let Some(other) = registry
        .projects
        .iter()
        .find(|p| p.name == name && p.path != dir_text && Some(p.id.as_str()) != own_id)
    else {
        return Ok(());
    };
    let key = if Path::new(&other.path).is_dir() {
        "projects.init.name_conflict"
    } else {
        "projects.init.name_conflict_gone"
    };
    bail!(t!(key, name = name, path = other.path, id = other.id))
}

/// `init --new-id` en una carpeta ya inicializada: solo cambia `[project] id`
/// (el resto del archivo, comentarios incluidos, se conserva) y registra la
/// carpeta con el ID nuevo. El espacio de trabajo anterior sigue siendo del
/// proyecto original.
fn regenerate_id(dir: &Path, cfg_path: &Path, cfg: ArcheologyConfig) -> Result<()> {
    let text = archeology_core::fsutil::read_to_string_nofollow(cfg_path)?.unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    let new_id = uuid::Uuid::new_v4().to_string();
    doc["project"]["id"] = toml_edit::value(new_id.clone());
    archeology_core::fsutil::write_in_root(dir, cfg_path, doc.to_string().as_bytes())?;
    archeology_registry::Registry::update(|reg| {
        reg.upsert(&new_id, &cfg.project.name, dir, "init")
    })?;
    println!(
        "{}",
        t!(
            "projects.init.new_id",
            name = cfg.project.name,
            old = cfg.project.id,
            new = new_id
        )
    );
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
