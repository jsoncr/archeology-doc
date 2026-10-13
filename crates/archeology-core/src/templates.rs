//! Plantillas embebidas (minijinja). Las fuentes canónicas viven en
//! `templates/<idioma>/` en la raíz del repositorio (`templates/en/`,
//! `templates/es/`) y se embeben en compilación. Se eligen según el idioma
//! de la documentación del proyecto ([`crate::Project::doc_lang`]).

use crate::i18n::Lang;
use anyhow::Result;
use minijinja::{context, Environment};

macro_rules! tpl {
    ($lang:literal, $name:literal) => {
        (
            $name,
            include_str!(concat!("../../../templates/", $lang, "/", $name)),
        )
    };
}

/// Lista de plantillas de un idioma, nombre → contenido.
macro_rules! templates_for {
    ($lang:literal) => {
        &[
            tpl!($lang, "feature.md"),
            tpl!($lang, "finding.md"),
            tpl!($lang, "module.md"),
            tpl!($lang, "overview.md"),
            tpl!($lang, "test-report.md"),
            tpl!($lang, "spec-report.md"),
            tpl!($lang, "glossary.md"),
            tpl!($lang, "decision.md"),
            tpl!($lang, "memory.md"),
            tpl!($lang, "session.md"),
            tpl!($lang, "task.md"),
            tpl!($lang, "agents.md"),
            tpl!($lang, "vitepress-config.ts"),
        ]
    };
}

/// Plantillas en inglés, nombre → contenido.
pub const TEMPLATES_EN: &[(&str, &str)] = templates_for!("en");
/// Plantillas en español, nombre → contenido.
pub const TEMPLATES_ES: &[(&str, &str)] = templates_for!("es");

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

/// Todas las plantillas de un idioma.
pub fn templates(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::En => TEMPLATES_EN,
        Lang::Es => TEMPLATES_ES,
    }
}

/// Contenido de la plantilla `name` en el idioma `lang`.
pub fn template(lang: Lang, name: &str) -> Option<&'static str> {
    templates(lang)
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

/// Renderiza una plantilla del idioma `lang` con un contexto simple de strings.
pub fn render(lang: Lang, name: &str, vars: &[(&str, &str)]) -> Result<String> {
    let src = template(lang, name)
        .ok_or_else(|| anyhow::anyhow!(crate::t!("core.templates.unknown", name = name)))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_exists_in_both_languages_with_the_same_frontmatter_keys() {
        assert_eq!(TEMPLATES_EN.len(), TEMPLATES_ES.len());
        for ((en_name, en), (es_name, es)) in TEMPLATES_EN.iter().zip(TEMPLATES_ES) {
            assert_eq!(en_name, es_name);
            // Las claves del frontmatter y las variables no se traducen.
            let keys = |s: &str| -> Vec<String> {
                let Some(rest) = s.strip_prefix("---\n") else {
                    return Vec::new();
                };
                let end = rest.find("\n---").unwrap_or(0);
                rest[..end]
                    .lines()
                    .filter_map(|l| l.split_once(':').map(|(k, _)| k.trim().to_string()))
                    .filter(|k| !k.is_empty() && !k.starts_with('{'))
                    .collect()
            };
            assert_eq!(keys(en), keys(es), "frontmatter distinto en {en_name}");
            let vars = |s: &str| -> std::collections::BTreeSet<String> {
                s.split("{{")
                    .skip(1)
                    .filter_map(|p| p.split("}}").next())
                    .map(|v| v.split('|').next().unwrap_or("").trim().to_string())
                    .collect()
            };
            assert_eq!(vars(en), vars(es), "variables distintas en {en_name}");
        }
    }

    #[test]
    fn renders_in_the_requested_language() {
        let en = render(
            Lang::En,
            "session.md",
            &[("date", "2026-01-01"), ("topic", "x")],
        )
        .unwrap();
        let es = render(
            Lang::Es,
            "session.md",
            &[("date", "2026-01-01"), ("topic", "x")],
        )
        .unwrap();
        assert!(en.contains("# Session 2026-01-01: x"), "{en}");
        assert!(es.contains("# Sesión 2026-01-01: x"), "{es}");
    }
}
