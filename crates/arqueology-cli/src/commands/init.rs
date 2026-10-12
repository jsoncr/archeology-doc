//! `init` — crea `arqueology-doc.toml`, `.arqueology-doc/` y la estructura
//! de salida (RF-001 a RF-006).

use anyhow::{bail, Context, Result};
use arqueology_core::config::{AdapterConfig, AdapterMode, ArqueologyConfig};
use arqueology_core::templates;
use arqueology_core::Project;
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
    /// Idioma de la documentación generada.
    #[arg(long, default_value = "es")]
    lang: String,
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
    let dir = arqueology_core::paths::canonicalize(&dir).unwrap_or(dir);
    let cfg_path = dir.join(arqueology_core::config::CONFIG_FILE);
    if cfg_path.exists() {
        bail!("el proyecto ya tiene arqueology-doc.toml");
    }

    let default_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "proyecto".into());

    // Detección de agentes instalados (RF-003).
    let mut config = ArqueologyConfig::new(&default_name, &args.lang);
    let installed = arqueology_agents::adapter::detect_installed(&config.agent.adapters);
    let suggested = installed
        .iter()
        .find(|a| *a != "manual")
        .cloned()
        .unwrap_or_else(|| "manual".into());

    let (name, agent, language) = if args.yes {
        (
            args.name.clone().unwrap_or(default_name),
            args.agent.clone().unwrap_or(suggested),
            args.lang.clone(),
        )
    } else {
        eprintln!("Inicializando arqueology-doc en {}", dir.display());
        if !installed.is_empty() {
            eprintln!("Agentes detectados: {}", installed.join(", "));
        }
        let name = super::ask(
            "Nombre del proyecto",
            args.name.as_deref().unwrap_or(&default_name),
        )?;
        let agent = super::ask(
            "Agente por defecto",
            args.agent.as_deref().unwrap_or(&suggested),
        )?;
        let language = super::ask("Idioma de la documentación", &args.lang)?;
        (name, agent, language)
    };

    if !config.agent.adapters.contains_key(&agent) {
        eprintln!(
            "aviso: el adaptador '{agent}' no está predefinido; añade [agent.adapters.{agent}] a arqueology-doc.toml"
        );
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
    config.project.language = language;
    // Adaptadores de contexto (RF-101): CLAUDE.md si se usa Claude Code.
    config.context.adapters = match &args.adapters {
        Some(list) => list.clone(),
        None if agent == "claude" => vec!["claude".to_string()],
        None => Vec::new(),
    };
    config.agent.default = agent;
    config.validate()?;

    // RF-005: no sobrescribir sin confirmación.
    if !args.yes
        && cfg_path.exists()
        && !super::confirm("arqueology-doc.toml existe. ¿Sobrescribir?", false)?
    {
        bail!("cancelado");
    }

    config.save(&cfg_path)?;
    eprintln!("creado {}", cfg_path.display());

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
        fs::create_dir_all(&d).with_context(|| format!("creando {}", d.display()))?;
    }

    // Plantillas de notas (RF-065).
    for name in templates::NOTE_TEMPLATES {
        if let Some(content) = templates::template(name) {
            fs::write(
                project.knowledge_dir().join("_templates").join(name),
                content,
            )?;
        }
    }

    // Memoria inicial (RF-102, RF-103): estable aquí, temporal en sessions/.
    let mem = project.memory_dir();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    for (slug, title) in [
        ("facts", "Hechos confirmados"),
        ("decisions", "Decisiones"),
        ("conventions", "Convenciones"),
        ("open-questions", "Preguntas abiertas"),
    ] {
        let path = mem.join(format!("{slug}.md"));
        if !path.exists() {
            fs::write(
                &path,
                templates::render(
                    "memory.md",
                    &[("slug", slug), ("title", title), ("date", &today)],
                )?,
            )?;
        }
    }

    // AGENTS.md, adaptadores por herramienta: regiones gestionadas (RF-100, RF-162).
    for f in arqueology_core::context::refresh(&project, None)? {
        eprintln!("actualizado {f}");
    }

    // Sitio VitePress básico (RF-112).
    if config.site.enabled {
        scaffold_site(&project)?;
        if !docs_preexisting {
            fs::create_dir_all(project.state_dir())?;
            fs::write(
                project
                    .state_dir()
                    .join(arqueology_scan::scanner::GENERATED_DOCS_MARK),
                "docs/ creado por adoc init\n",
            )?;
        }
        // Un `docs/wiki` o `docs/specs` del usuario no detiene `init`.
        if let Err(e) = arqueology_core::site::prepare(&project) {
            eprintln!("aviso: sitio no preparado: {e}");
        }
    }

    // llms.txt (RF-115).
    arqueology_core::site::write_llms_txt(&project)?;

    // Registro global (RF-171).
    arqueology_registry::Registry::update(|reg| {
        reg.upsert(&config.project.id, &config.project.name, &dir, "init")
    })?;
    eprintln!(
        "registrado en {}",
        arqueology_registry::registry_path()?.display()
    );

    println!("arqueology-doc inicializado en {}", dir.display());
    println!(
        "Estado de trabajo (tareas, ejecuciones, inventario): {}",
        project.state_dir().display()
    );
    println!("Siguiente paso: `adoc scan` para generar el inventario.");
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
        fs::create_dir_all(docs.join(sub))?;
    }
    // La copia del vault, la caché y el build son salida derivada.
    let ignore = docs.join(".gitignore");
    if !ignore.exists() {
        fs::write(
            &ignore,
            format!(
                "{}/\n.vitepress/cache/\n.vitepress/dist/\n",
                arqueology_core::site::WIKI_DIR
            ),
        )?;
    }
    let index = docs.join("index.md");
    if !index.exists() {
        fs::write(
            &index,
            format!(
                "---\nlayout: home\ntitle: {}\n---\n\n# {}\n\nDocumentación generada con arqueology-doc.\n",
                project.config.project.name, project.config.project.name
            ),
        )?;
    }
    // `.mts` funciona tanto en paquetes CommonJS como ESM.
    let config_ts = docs.join(".vitepress").join("config.mts");
    if !config_ts.exists() && !docs.join(".vitepress").join("config.ts").exists() {
        fs::write(
            &config_ts,
            templates::render(
                "vitepress-config.ts",
                &[
                    ("project_name", &project.config.project.name),
                    ("language", &project.config.project.language),
                ],
            )?,
        )?;
    }
    Ok(())
}
