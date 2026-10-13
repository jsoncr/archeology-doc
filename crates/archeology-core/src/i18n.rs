//! Internacionalización: idiomas soportados (`en`, `es`), catálogos de
//! mensajes y resolución del idioma de la interfaz.
//!
//! Hay dos idiomas distintos:
//!
//! - **Idioma de la interfaz** (mensajes, errores, avisos): se resuelve una
//!   vez al arrancar con [`init_ui_lang`] y se usa con [`t!`](crate::t).
//!   Precedencia: `--lang` > `ADOC_LANG` > `language` en
//!   `~/.archeology-doc/config.toml` > locale del sistema > `en`.
//! - **Idioma de la documentación del proyecto** (plantillas, AGENTS.md,
//!   MOCs, llms.txt, sitio, prompts de tareas): `[project] language` de
//!   `archeology-doc.toml` ([`crate::Project::doc_lang`]). Se usa con
//!   [`tl!`](crate::tl), nunca con el idioma de la interfaz.
//!
//! Los textos viven en `crates/archeology-core/locales/<idioma>/*.toml`
//! (tablas anidadas → claves con puntos: `[cli.task]` + `answer_saved = "..."`
//! es `cli.task.answer_saved`). Los marcadores son `{nombre}`. Una clave que
//! falta en un idioma usa el texto en inglés y, si tampoco existe, la clave.

use crate::error::{Error, Result};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Variable de entorno con el idioma de la interfaz.
pub const LANG_ENV: &str = "ADOC_LANG";
/// Archivo de preferencias del usuario dentro del directorio de configuración.
pub const USER_CONFIG_FILE: &str = "config.toml";
/// Clave del idioma en el archivo de preferencias.
pub const USER_LANGUAGE_KEY: &str = "language";

/// Idioma soportado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    En,
    Es,
}

impl Lang {
    /// Todos los idiomas soportados.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Es];
    /// Idioma de respaldo (claves que faltan, valores inválidos).
    pub const FALLBACK: Lang = Lang::En;

    /// Código ISO 639-1 (`en`, `es`): es el valor de las configuraciones.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
        }
    }

    /// Valor exacto de configuración: `en` o `es` (sin distinguir mayúsculas).
    pub fn parse(value: &str) -> Option<Lang> {
        match value.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "es" => Some(Lang::Es),
            _ => None,
        }
    }

    /// Locale del sistema (`es_CR.UTF-8`, `en-US`, `es`): `es*` → es, `en*` → en.
    pub fn from_locale(value: &str) -> Option<Lang> {
        let v = value.trim().to_ascii_lowercase();
        if v.starts_with("es") {
            Some(Lang::Es)
        } else if v.starts_with("en") {
            Some(Lang::En)
        } else {
            None
        }
    }

    fn index(self) -> usize {
        match self {
            Lang::En => 0,
            Lang::Es => 1,
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::str::FromStr for Lang {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Lang::parse(s)
            .ok_or_else(|| Error::InvalidConfig(crate::t!("core.i18n.invalid_lang", value = s)))
    }
}

// ---------------------------------------------------------------------------
// Catálogos
// ---------------------------------------------------------------------------

mod embedded {
    // Generado por build.rs: `EN` y `ES`, (archivo, contenido) por idioma.
    include!(concat!(env!("OUT_DIR"), "/locales.rs"));
}

type Catalog = HashMap<String, String>;

/// Archivos embebidos de un idioma: (nombre de archivo, contenido TOML).
pub fn embedded_files(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::En => embedded::EN,
        Lang::Es => embedded::ES,
    }
}

/// Parsea los archivos de un idioma en un mapa clave → texto. Error si un
/// archivo no es TOML válido, un valor no es texto o una clave se repite.
pub fn parse_catalog(files: &[(&str, &str)]) -> std::result::Result<Catalog, String> {
    let mut out = Catalog::new();
    for (name, text) in files {
        let table: toml::Table = text.parse().map_err(|e| format!("{name}: {e}"))?;
        flatten(name, "", &table, &mut out)?;
    }
    Ok(out)
}

fn flatten(
    file: &str,
    prefix: &str,
    table: &toml::Table,
    out: &mut Catalog,
) -> std::result::Result<(), String> {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::String(s) => {
                if out.insert(key.clone(), s.clone()).is_some() {
                    return Err(format!("{file}: clave repetida `{key}`"));
                }
            }
            toml::Value::Table(t) => flatten(file, &key, t, out)?,
            _ => return Err(format!("{file}: `{key}` debe ser texto")),
        }
    }
    Ok(())
}

fn catalogs() -> &'static [Catalog; 2] {
    static CATALOGS: OnceLock<[Catalog; 2]> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        // Los catálogos se validan en los tests: aquí un error es un bug.
        Lang::ALL.map(|l| {
            parse_catalog(embedded_files(l))
                .unwrap_or_else(|e| panic!("catálogo de traducción {l} inválido: {e}"))
        })
    })
}

/// Texto de `key` en `lang` con los marcadores `{nombre}` sustituidos.
/// Respaldo: inglés y, si tampoco existe, la propia clave.
pub fn translate(lang: Lang, key: &str, args: &[(&str, String)]) -> String {
    let cats = catalogs();
    let template = cats[lang.index()]
        .get(key)
        .or_else(|| cats[Lang::FALLBACK.index()].get(key))
        .map(String::as_str)
        .unwrap_or(key);
    interpolate(template, args)
}

/// ¿Existe `key` en el catálogo de `lang`?
pub fn has_key(lang: Lang, key: &str) -> bool {
    catalogs()[lang.index()].contains_key(key)
}

/// Sustituye `{nombre}` por su valor en una sola pasada: un valor que
/// contiene llaves no se vuelve a interpretar. Los marcadores sin valor
/// (o llaves que no son marcadores) se dejan tal cual.
pub fn interpolate(template: &str, args: &[(&str, String)]) -> String {
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_len = placeholder_len(after);
        if name_len > 0 && after[name_len..].starts_with('}') {
            let name = &after[..name_len];
            if let Some((_, v)) = args.iter().find(|(n, _)| *n == name) {
                out.push_str(v);
                rest = &after[name_len + 1..];
                continue;
            }
        }
        out.push('{');
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Longitud de un identificador de marcador al inicio de `s` (0 si no hay).
fn placeholder_len(s: &str) -> usize {
    let mut len = 0;
    for (i, c) in s.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        };
        if !ok {
            break;
        }
        len = i + c.len_utf8();
    }
    len
}

/// Marcadores `{nombre}` de un texto (para comprobar la paridad de idiomas).
pub fn placeholders(template: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let n = placeholder_len(after);
        if n > 0 && after[n..].starts_with('}') {
            out.insert(after[..n].to_string());
        }
        rest = after;
    }
    out
}

/// Traduce con el idioma de la interfaz. Uso: `t!("core.error.task_not_found", id = id)`.
///
/// Los valores se convierten con `to_string()` (rutas: `path.display()`).
#[macro_export]
macro_rules! t {
    ($key:expr) => {
        $crate::i18n::translate($crate::i18n::ui_lang(), $key, &[])
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::translate(
            $crate::i18n::ui_lang(),
            $key,
            &[$((stringify!($name), ($value).to_string())),+],
        )
    };
}

/// Traduce con un idioma explícito (contenido generado: idioma del proyecto).
/// Uso: `tl!(project.doc_lang(), "doc.agents.commands")`.
#[macro_export]
macro_rules! tl {
    ($lang:expr, $key:expr) => {
        $crate::i18n::translate($lang, $key, &[])
    };
    ($lang:expr, $key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::translate(
            $lang,
            $key,
            &[$((stringify!($name), ($value).to_string())),+],
        )
    };
}

// ---------------------------------------------------------------------------
// Idioma de la interfaz
// ---------------------------------------------------------------------------

/// Origen del idioma de la interfaz resuelto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangSource {
    /// Flag global `--lang`.
    Flag,
    /// Variable `ADOC_LANG`.
    Env,
    /// `language` en `~/.archeology-doc/config.toml`.
    UserConfig,
    /// Locale del sistema (`LC_ALL`, `LC_MESSAGES`, `LANG` o el del SO).
    SystemLocale,
    /// Sin ninguna de las anteriores: `en`.
    Default,
}

impl LangSource {
    /// Identificador estable (salida JSON).
    pub fn as_str(self) -> &'static str {
        match self {
            LangSource::Flag => "flag",
            LangSource::Env => "env",
            LangSource::UserConfig => "config",
            LangSource::SystemLocale => "locale",
            LangSource::Default => "default",
        }
    }

    /// Descripción en el idioma de la interfaz.
    pub fn describe(self) -> String {
        match self {
            LangSource::Flag => crate::t!("core.i18n.source.flag"),
            LangSource::Env => crate::t!("core.i18n.source.env", var = LANG_ENV),
            LangSource::UserConfig => crate::t!(
                "core.i18n.source.config",
                path = user_config_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| USER_CONFIG_FILE.to_string())
            ),
            LangSource::SystemLocale => crate::t!("core.i18n.source.locale"),
            LangSource::Default => crate::t!("core.i18n.source.default"),
        }
    }
}

/// Valores que intervienen en la resolución (inyectables en los tests).
#[derive(Debug, Clone, Default)]
pub struct LangInputs {
    /// `--lang`.
    pub flag: Option<String>,
    /// `ADOC_LANG`.
    pub env: Option<String>,
    /// `language` del archivo de preferencias.
    pub user_config: Option<String>,
    /// Primera de `LC_ALL`, `LC_MESSAGES`, `LANG` no vacía.
    pub locale_env: Option<String>,
    /// Locale del sistema operativo (solo si no hay variables de locale).
    pub system_locale: Option<String>,
}

impl LangInputs {
    /// Lee el entorno real: variables, preferencias del usuario y locale.
    pub fn from_env(flag: Option<&str>) -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let locale_env = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| var(k));
        // Con variables de locale definidas mandan ellas (también `C`):
        // el locale del SO solo se consulta si no hay ninguna.
        let system_locale = if locale_env.is_none() {
            sys_locale::get_locale()
        } else {
            None
        };
        Self {
            flag: flag.map(String::from),
            env: var(LANG_ENV),
            user_config: read_user_language().ok().flatten(),
            locale_env,
            system_locale,
        }
    }
}

/// Resultado de resolver el idioma de la interfaz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub lang: Lang,
    pub source: LangSource,
    /// Valores inválidos ignorados: (origen, valor).
    pub invalid: Vec<(LangSource, String)>,
}

/// Aplica la precedencia `--lang` > `ADOC_LANG` > preferencias > locale > `en`.
/// Un valor inválido se anota en `invalid` y se pasa a la siguiente fuente.
pub fn resolve(inputs: &LangInputs) -> Resolution {
    let mut invalid = Vec::new();
    for (source, value) in [
        (LangSource::Flag, &inputs.flag),
        (LangSource::Env, &inputs.env),
        (LangSource::UserConfig, &inputs.user_config),
    ] {
        if let Some(v) = value {
            match Lang::parse(v) {
                Some(lang) => {
                    return Resolution {
                        lang,
                        source,
                        invalid,
                    }
                }
                None => invalid.push((source, v.clone())),
            }
        }
    }
    let locale = inputs
        .locale_env
        .as_deref()
        .or(inputs.system_locale.as_deref());
    if let Some(lang) = locale.and_then(Lang::from_locale) {
        return Resolution {
            lang,
            source: LangSource::SystemLocale,
            invalid,
        };
    }
    Resolution {
        lang: Lang::FALLBACK,
        source: LangSource::Default,
        invalid,
    }
}

/// 0 = sin resolver; si no, `Lang::index() + 1`.
static UI_LANG: AtomicU8 = AtomicU8::new(0);
static RESOLUTION: OnceLock<Resolution> = OnceLock::new();

thread_local! {
    /// Evita la recursión: leer las preferencias puede producir mensajes
    /// (`t!`) antes de que el idioma esté resuelto; esos usan `en`.
    static RESOLVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Resuelve el idioma de la interfaz (una vez, al arrancar), lo fija como
/// global y avisa por stderr de los valores inválidos ignorados.
pub fn init_ui_lang(flag: Option<&str>) -> Resolution {
    RESOLVING.with(|r| r.set(true));
    let inputs = LangInputs::from_env(flag);
    RESOLVING.with(|r| r.set(false));
    let res = resolve(&inputs);
    set_ui_lang(res.lang);
    if RESOLUTION.set(res.clone()).is_ok() {
        warn_invalid(&res);
    }
    res
}

fn warn_invalid(res: &Resolution) {
    for (source, value) in &res.invalid {
        eprintln!(
            "{}",
            crate::t!(
                "core.i18n.invalid_ignored",
                value = value,
                source = source.describe()
            )
        );
    }
}

/// Resolución usada al arrancar (o la calculada al primer uso sin `--lang`).
pub fn ui_resolution() -> Resolution {
    if let Some(r) = RESOLUTION.get() {
        return r.clone();
    }
    init_ui_lang(None)
}

/// Fija el idioma de la interfaz (también útil en tests).
pub fn set_ui_lang(lang: Lang) {
    UI_LANG.store(lang.index() as u8 + 1, Ordering::Relaxed);
}

/// Idioma de la interfaz. Si nadie llamó a [`init_ui_lang`], se resuelve
/// sin `--lang` en el primer uso.
pub fn ui_lang() -> Lang {
    match UI_LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        2 => Lang::Es,
        _ if RESOLVING.with(|r| r.get()) => Lang::FALLBACK,
        _ => ui_resolution().lang,
    }
}

// ---------------------------------------------------------------------------
// Preferencias del usuario (`~/.archeology-doc/config.toml`)
// ---------------------------------------------------------------------------

/// Ruta del archivo de preferencias (respeta `ARCHEOLOGY_DOC_CONFIG_DIR`).
pub fn user_config_path() -> Result<PathBuf> {
    Ok(crate::paths::config_dir()?.join(USER_CONFIG_FILE))
}

/// `language` tal como está en las preferencias (sin validar).
pub fn read_user_language() -> Result<Option<String>> {
    let path = user_config_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let table: toml::Table = text.parse()?;
    Ok(table
        .get(USER_LANGUAGE_KEY)
        .and_then(|v| v.as_str())
        .map(String::from))
}

/// Fija (`Some`) o borra (`None`) `language` en las preferencias,
/// conservando el resto del archivo. Escritura atómica.
pub fn write_user_language(lang: Option<Lang>) -> Result<PathBuf> {
    let path = user_config_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        Error::InvalidConfig(format!("{}: {e}", path.display()))
    })?;
    match lang {
        Some(l) => doc[USER_LANGUAGE_KEY] = toml_edit::value(l.code()),
        None => {
            doc.as_table_mut().remove(USER_LANGUAGE_KEY);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::fsutil::atomic_write(&path, doc.to_string().as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_have_the_same_keys_and_placeholders() {
        let en = parse_catalog(embedded_files(Lang::En)).unwrap();
        let es = parse_catalog(embedded_files(Lang::Es)).unwrap();
        assert!(!en.is_empty());
        let en_keys: BTreeSet<_> = en.keys().collect();
        let es_keys: BTreeSet<_> = es.keys().collect();
        let only_en: Vec<_> = en_keys.difference(&es_keys).collect();
        let only_es: Vec<_> = es_keys.difference(&en_keys).collect();
        assert!(
            only_en.is_empty() && only_es.is_empty(),
            "solo en en: {only_en:?}; solo en es: {only_es:?}"
        );
        for (k, v) in &en {
            assert_eq!(
                placeholders(v),
                placeholders(&es[k]),
                "marcadores distintos en `{k}`"
            );
        }
        // Mismos archivos en ambos idiomas.
        let names = |l| {
            embedded_files(l)
                .iter()
                .map(|(n, _)| *n)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(Lang::En), names(Lang::Es));
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let files = [
            ("a.toml", "[x]\ny = \"1\"\n"),
            ("b.toml", "[x]\ny = \"2\"\n"),
        ];
        assert!(parse_catalog(&files).is_err());
    }

    #[test]
    fn interpolation_is_single_pass_and_keeps_unknown_braces() {
        let args = [("a", "{b}".to_string()), ("b", "x".to_string())];
        assert_eq!(
            interpolate("{a}-{b}-{c}-{{ id }}", &args),
            "{b}-x-{c}-{{ id }}"
        );
        assert_eq!(
            placeholders("{a} {b_1} {{ x }} {1}"),
            ["a", "b_1"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn missing_keys_fall_back_to_the_key() {
        assert_eq!(translate(Lang::Es, "no.such.key", &[]), "no.such.key");
        assert_eq!(
            tl!(Lang::Es, "core.i18n.invalid_lang", value = "fr"),
            translate(
                Lang::Es,
                "core.i18n.invalid_lang",
                &[("value", "fr".into())]
            )
        );
    }

    #[test]
    fn parses_codes_and_locales() {
        assert_eq!(Lang::parse(" ES "), Some(Lang::Es));
        assert_eq!(Lang::parse("es-CR"), None);
        assert_eq!(Lang::from_locale("es_CR.UTF-8"), Some(Lang::Es));
        assert_eq!(Lang::from_locale("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_locale("C"), None);
    }

    fn inputs(
        flag: Option<&str>,
        env: Option<&str>,
        cfg: Option<&str>,
        locale: Option<&str>,
        system: Option<&str>,
    ) -> LangInputs {
        LangInputs {
            flag: flag.map(String::from),
            env: env.map(String::from),
            user_config: cfg.map(String::from),
            locale_env: locale.map(String::from),
            system_locale: system.map(String::from),
        }
    }

    #[test]
    fn resolution_follows_the_precedence() {
        let r = resolve(&inputs(
            Some("es"),
            Some("en"),
            Some("en"),
            Some("en_US"),
            None,
        ));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::Flag));
        let r = resolve(&inputs(None, Some("es"), Some("en"), Some("en_US"), None));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::Env));
        let r = resolve(&inputs(None, None, Some("es"), Some("en_US"), None));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::UserConfig));
        let r = resolve(&inputs(None, None, None, Some("es_CR.UTF-8"), Some("en")));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::SystemLocale));
        let r = resolve(&inputs(None, None, None, None, Some("es-ES")));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::SystemLocale));
        let r = resolve(&inputs(None, None, None, Some("C"), None));
        assert_eq!((r.lang, r.source), (Lang::En, LangSource::Default));
        let r = resolve(&LangInputs::default());
        assert_eq!((r.lang, r.source), (Lang::En, LangSource::Default));
    }

    #[test]
    fn invalid_values_are_reported_and_skipped() {
        let r = resolve(&inputs(Some("fr"), Some("xx"), Some("es"), None, None));
        assert_eq!((r.lang, r.source), (Lang::Es, LangSource::UserConfig));
        assert_eq!(
            r.invalid,
            vec![
                (LangSource::Flag, "fr".to_string()),
                (LangSource::Env, "xx".to_string())
            ]
        );
    }
}
