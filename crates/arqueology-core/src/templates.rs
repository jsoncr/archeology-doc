//! Plantillas embebidas (minijinja). Las fuentes canónicas viven en
//! `templates/` en la raíz del repositorio y se embeben en compilación.

use anyhow::Result;
use minijinja::{context, Environment};

macro_rules! tpl {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../templates/", $name)))
    };
}

/// Todas las plantillas disponibles, nombre → contenido.
pub const TEMPLATES: &[(&str, &str)] = &[
    tpl!("feature.md"),
    tpl!("finding.md"),
    tpl!("module.md"),
    tpl!("overview.md"),
    tpl!("test-report.md"),
    tpl!("spec-report.md"),
    tpl!("glossary.md"),
    tpl!("decision.md"),
    tpl!("memory.md"),
    tpl!("session.md"),
    tpl!("task.md"),
    tpl!("agents.md"),
    tpl!("vitepress-config.ts"),
];

/// Archivos que `init` escribe en `knowledge/_templates/` (solo notas).
pub const NOTE_TEMPLATES: &[&str] = &[
    "feature.md",
    "finding.md",
    "module.md",
    "overview.md",
    "test-report.md",
    "spec-report.md",
    "glossary.md",
    "decision.md",
    "memory.md",
    "session.md",
];

pub fn template(name: &str) -> Option<&'static str> {
    TEMPLATES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Renderiza una plantilla con un contexto simple de strings.
pub fn render(name: &str, vars: &[(&str, &str)]) -> Result<String> {
    let src = template(name).ok_or_else(|| anyhow::anyhow!("plantilla desconocida: {name}"))?;
    render_str(src, vars)
}

/// Renderiza una plantilla arbitraria con contexto simple.
pub fn render_str(src: &str, vars: &[(&str, &str)]) -> Result<String> {
    let mut env = Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Lenient);
    let tmpl = env.template_from_str(src)?;
    let mut map = std::collections::BTreeMap::new();
    for (k, v) in vars {
        map.insert(*k, *v);
    }
    Ok(tmpl.render(context! { ..map })?)
}
