//! `doctor` — verifica los requisitos del CLI y del proyecto.
//!
//! Requeridos (fallo → código de salida 1): directorio de configuración
//! escribible, registro legible y, dentro de un proyecto, configuración válida
//! y adaptador por defecto disponible. El resto son avisos: habilitan
//! funciones concretas (historial git, sitio, validación de OpenSpec).

use super::toolchain;
use anyhow::Result;
use arqueology_core::config::AdapterMode;
use arqueology_core::Project;
use clap::Args;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Args)]
pub struct DoctorArgs {
    /// Formato de salida.
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    format: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Level {
    Ok,
    Warn,
    Fail,
}

#[derive(Serialize)]
struct Check {
    name: String,
    level: Level,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

struct Report(Vec<Check>);

impl Report {
    fn add(&mut self, name: &str, level: Level, detail: impl Into<String>, hint: Option<&str>) {
        self.0.push(Check {
            name: name.into(),
            level,
            detail: detail.into(),
            hint: hint.map(String::from),
        });
    }
}

pub fn run(args: DoctorArgs, project_arg: Option<String>) -> Result<i32> {
    let pb = crate::ui::spinner("Verificando configuración y registro…");
    let mut r = Report(Vec::new());
    r.add(
        "adoc",
        Level::Ok,
        format!("versión {}", env!("CARGO_PKG_VERSION")),
        None,
    );

    check_config_dir(&mut r);
    check_registry(&mut r);

    // Proyecto actual o `--project`: opcional para doctor.
    let project = match &project_arg {
        Some(_) => match super::resolve_project(&project_arg) {
            Ok(p) => Some(p),
            Err(e) => {
                r.add("proyecto", Level::Fail, e.to_string(), None);
                None
            }
        },
        None => {
            match Project::discover(&std::env::current_dir()?) {
                Ok(p) => Some(p),
                Err(arqueology_core::Error::ProjectNotFound) => None,
                Err(e) => {
                    r.add(
                    "proyecto",
                    Level::Fail,
                    format!("arqueology-doc.toml inválido: {e}"),
                    Some("corrige el archivo; `adoc init` en un directorio vacío muestra un ejemplo"),
                );
                    None
                }
            }
        }
    };

    pb.set_message("Verificando herramientas: git, Node, VitePress, OpenSpec…");
    check_git(&mut r);
    let node_ok = check_node(&mut r);
    check_vitepress(&mut r, project.as_ref(), node_ok);
    check_openspec(&mut r, project.as_ref());
    pb.set_message("Verificando agentes…");
    check_agents(&mut r, project.as_ref());
    if let Some(p) = &project {
        check_project(&mut r, p);
    } else if project_arg.is_none() && !r.0.iter().any(|c| c.name == "proyecto") {
        r.add(
            "proyecto",
            Level::Ok,
            "sin proyecto en el directorio actual (se omiten sus comprobaciones)",
            None,
        );
    }

    pb.finish_and_clear();
    let failed = r.0.iter().any(|c| c.level == Level::Fail);
    if args.format == "json" {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": !failed,
                "checks": r.0,
            }))?
        );
    } else {
        for c in &r.0 {
            let mark = match c.level {
                Level::Ok => "✓",
                Level::Warn => "!",
                Level::Fail => "✗",
            };
            println!("{mark} {:<14} {}", c.name, c.detail);
            if let Some(h) = &c.hint {
                println!("  {:<14} → {h}", "");
            }
        }
        let warns = r.0.iter().filter(|c| c.level == Level::Warn).count();
        let fails = r.0.iter().filter(|c| c.level == Level::Fail).count();
        println!("\ndoctor: {fails} errores, {warns} avisos");
    }
    Ok(if failed { 1 } else { 0 })
}

fn check_config_dir(r: &mut Report) {
    match arqueology_registry::config_dir() {
        Ok(dir) => {
            let probe = dir.join(format!(".doctor-{}", std::process::id()));
            let writable = fs::create_dir_all(&dir)
                .and_then(|_| fs::write(&probe, b""))
                .and_then(|_| fs::remove_file(&probe));
            match writable {
                Ok(()) => r.add("configuración", Level::Ok, dir.display().to_string(), None),
                Err(e) => r.add(
                    "configuración",
                    Level::Fail,
                    format!("{} no es escribible: {e}", dir.display()),
                    Some("revisa permisos o define ARQUEOLOGY_DOC_CONFIG_DIR"),
                ),
            }
        }
        Err(e) => r.add(
            "configuración",
            Level::Fail,
            e.to_string(),
            Some("define ARQUEOLOGY_DOC_CONFIG_DIR"),
        ),
    }
}

fn check_registry(r: &mut Report) {
    match arqueology_registry::Registry::load() {
        Ok(reg) => {
            let missing = reg
                .projects
                .iter()
                .filter(|p| !PathBuf::from(&p.path).is_dir())
                .count();
            if missing > 0 {
                r.add(
                    "registro",
                    Level::Warn,
                    format!(
                        "{} proyectos, {missing} con ruta inexistente",
                        reg.projects.len()
                    ),
                    Some("adoc projects prune"),
                );
            } else {
                r.add(
                    "registro",
                    Level::Ok,
                    format!("{} proyectos registrados", reg.projects.len()),
                    None,
                );
            }
        }
        Err(e) => r.add(
            "registro",
            Level::Fail,
            format!("no se puede leer: {e:#}"),
            Some("corrige o elimina registry.toml y reconstrúyelo con `adoc projects discover <dir>`"),
        ),
    }
}

fn check_git(r: &mut Report) {
    match toolchain::tool_version("git") {
        Some(v) => r.add("git", Level::Ok, v, None),
        None => r.add(
            "git",
            Level::Warn,
            "no encontrado: sin historial, hotspots ni `sync --since`",
            Some("instala git"),
        ),
    }
}

fn check_node(r: &mut Report) -> bool {
    match toolchain::node_version() {
        Some(v) if toolchain::major(&v).unwrap_or(0) >= toolchain::MIN_NODE_MAJOR => {
            let npm = toolchain::npm_version()
                .map(|n| format!(" · npm {n}"))
                .unwrap_or_else(|| " · npm no encontrado".into());
            r.add("node", Level::Ok, format!("{v}{npm}"), None);
            true
        }
        Some(v) => {
            r.add(
                "node",
                Level::Warn,
                format!(
                    "{v}: VitePress requiere Node {}+",
                    toolchain::MIN_NODE_MAJOR
                ),
                Some("actualiza Node"),
            );
            false
        }
        None => {
            r.add(
                "node",
                Level::Warn,
                "no encontrado: el sitio VitePress no estará disponible",
                Some("instala Node 18+"),
            );
            false
        }
    }
}

fn check_vitepress(r: &mut Report, project: Option<&Project>, node_ok: bool) {
    match toolchain::find_vitepress(project) {
        Some(vp) => {
            let detail = format!(
                "{} ({}) — {}",
                vp.version.as_deref().unwrap_or("?"),
                vp.source.as_str(),
                vp.package_dir.display()
            );
            let global_with_import = vp.source == toolchain::VitepressSource::Global
                && project
                    .map(toolchain::config_imports_vitepress)
                    .unwrap_or(false);
            if global_with_import {
                r.add(
                    "vitepress",
                    Level::Warn,
                    detail,
                    Some(
                        "docs/.vitepress/config importa 'vitepress' y fallará con la instalación \
                         global: quita el import (ver plantilla) o usa `adoc site setup`",
                    ),
                );
            } else {
                r.add("vitepress", Level::Ok, detail, None);
            }
        }
        None => r.add(
            "vitepress",
            Level::Warn,
            "no encontrado: `site dev|build` y `check --site` no funcionarán",
            Some(if node_ok {
                "adoc site setup (compartido) · npm i -g vitepress · npm i -D vitepress"
            } else {
                "instala Node 18+ y luego `adoc site setup`"
            }),
        ),
    }
}

fn check_openspec(r: &mut Report, project: Option<&Project>) {
    let uses_openspec = project
        .map(|p| p.config.openspec_enabled(&p.root))
        .unwrap_or(false);
    match toolchain::tool_version("openspec") {
        Some(v) => r.add("openspec", Level::Ok, v, None),
        None if uses_openspec => r.add(
            "openspec",
            Level::Warn,
            "CLI no encontrado: se usará la validación estructural propia",
            Some("npm i -g @fission-ai/openspec"),
        ),
        None => r.add(
            "openspec",
            Level::Ok,
            "CLI no instalado (opcional; el proyecto no usa OpenSpec)",
            None,
        ),
    }
}

fn check_agents(r: &mut Report, project: Option<&Project>) {
    let config = project.map(|p| p.config.clone()).unwrap_or_default();
    let default = &config.agent.default;
    for (name, adapter) in &config.agent.adapters {
        let st = arqueology_agents::check_adapter(name, adapter);
        let is_default = name == default;
        let label = if is_default {
            format!("agente:{name}*")
        } else {
            format!("agente:{name}")
        };
        let level = match (st.available, is_default && project.is_some()) {
            (true, _) => Level::Ok,
            (false, true) => Level::Fail,
            (false, false) => Level::Warn,
        };
        let hint = (!st.available).then_some(if !adapter.trusted {
            "revisa el adaptador y apruébalo con `adoc agent trust <nombre>`"
        } else if is_default {
            "instala el agente o cambia agent.default (p. ej. a \"manual\")"
        } else {
            "opcional: solo si vas a usar este adaptador"
        });
        let mode = match adapter.mode {
            AdapterMode::Headless => "headless",
            AdapterMode::Export => "export",
        };
        r.add(&label, level, format!("{mode} · {}", st.detail), hint);
        for w in arqueology_agents::adapter_warnings(name, adapter) {
            r.add(
                &label,
                if is_default && project.is_some() {
                    Level::Fail
                } else {
                    Level::Warn
                },
                w,
                None,
            );
        }
    }
    if !config.agent.adapters.contains_key(default) {
        r.add(
            "agente",
            Level::Fail,
            format!("agent.default = \"{default}\" no está en [agent.adapters]"),
            Some("define el adaptador o cambia agent.default"),
        );
    }
}

fn check_project(r: &mut Report, p: &Project) {
    r.add(
        "proyecto",
        Level::Ok,
        format!("{} ({})", p.config.project.name, p.root.display()),
        None,
    );
    r.add(
        "espacio",
        Level::Ok,
        p.state_dir().display().to_string(),
        None,
    );
    if let Err(e) = arqueology_core::state::State::load(&p.state_file()) {
        r.add(
            "estado",
            Level::Fail,
            format!("state.json ilegible: {e}"),
            Some("elimina state.json del espacio de trabajo; `adoc scan` lo regenera"),
        );
    }
    if !p.inventory_file().is_file() {
        r.add(
            "inventario",
            Level::Warn,
            "sin inventario",
            Some("adoc scan"),
        );
    }
    if p.config.site.enabled {
        let vp = p.docs_dir().join(".vitepress");
        if !vp.join("config.mts").is_file() && !vp.join("config.ts").is_file() {
            r.add(
                "sitio",
                Level::Warn,
                "falta docs/.vitepress/config.mts",
                Some("restáuralo desde la plantilla o desactiva [site] enabled"),
            );
        }
    }
    // El registro debe apuntar a la ruta actual (RF-175).
    if let Ok(reg) = arqueology_registry::Registry::load() {
        match reg.projects.iter().find(|e| e.id == p.config.project.id) {
            None => r.add(
                "registro",
                Level::Warn,
                "el proyecto no está registrado",
                Some("adoc projects add ."),
            ),
            Some(e)
                if std::path::Path::new(&e.path)
                    != arqueology_core::paths::canonicalize(&p.root).unwrap_or(p.root.clone()) =>
            {
                r.add(
                    "registro",
                    Level::Warn,
                    format!("registrado en otra ruta: {}", e.path),
                    Some("cualquier comando en este directorio lo re-vincula"),
                )
            }
            _ => {}
        }
    }
}
