//! `config` — preferencias del usuario en `~/.archeology-doc/config.toml`
//! (o `$ARCHEOLOGY_DOC_CONFIG_DIR/config.toml`). Por ahora: el idioma de la
//! interfaz (`language`).

use anyhow::Result;
use archeology_core::i18n::{self, Lang};
use archeology_core::t;
use clap::{Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Fija una preferencia (p. ej. `adoc config set language en`).
    Set {
        key: ConfigKey,
        /// Valor (language: en|es).
        value: String,
    },
    /// Muestra el valor efectivo y de dónde sale.
    Get {
        key: ConfigKey,
        /// Salida JSON: {"key", "value", "source"}.
        #[arg(long)]
        json: bool,
    },
    /// Borra una preferencia (se vuelve a usar el valor por defecto).
    Unset { key: ConfigKey },
}

/// Claves de configuración soportadas.
#[derive(Clone, Copy, ValueEnum)]
pub enum ConfigKey {
    /// Idioma de la interfaz (en|es).
    Language,
}

pub fn run(cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Set {
            key: ConfigKey::Language,
            value,
        } => {
            let lang = Lang::parse(&value)
                .ok_or_else(|| anyhow::anyhow!(t!("core.i18n.invalid_lang", value = value)))?;
            let path = i18n::write_user_language(Some(lang))?;
            println!(
                "{}",
                t!(
                    "cli.config.language_set",
                    lang = lang,
                    path = path.display()
                )
            );
        }
        ConfigCmd::Get {
            key: ConfigKey::Language,
            json,
        } => {
            let res = i18n::ui_resolution();
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "key": "language",
                        "value": res.lang.code(),
                        "source": res.source.as_str(),
                    })
                );
            } else {
                println!(
                    "{}",
                    t!(
                        "cli.config.language_get",
                        lang = res.lang,
                        source = res.source.describe()
                    )
                );
            }
        }
        ConfigCmd::Unset {
            key: ConfigKey::Language,
        } => {
            let path = i18n::write_user_language(None)?;
            println!("{}", t!("cli.config.language_unset", path = path.display()));
        }
    }
    Ok(())
}
