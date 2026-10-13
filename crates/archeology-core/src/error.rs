use crate::t;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Errores de archeology-core. Los mensajes se muestran en el idioma de la
/// interfaz (`crate::i18n::ui_lang`), por eso `Display` es manual.
#[derive(Debug)]
pub enum Error {
    ProjectNotFound,
    AlreadyInitialized(String),
    TaskNotFound(String),
    UnknownAdapter(String),
    /// Mensaje ya traducido.
    InvalidConfig(String),
    /// Mensaje ya traducido.
    Validation(String),
    Io(std::io::Error),
    TomlSer(toml::ser::Error),
    TomlDe(toml::de::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ProjectNotFound => f.write_str(&t!("core.error.project_not_found")),
            Error::AlreadyInitialized(p) => {
                f.write_str(&t!("core.error.already_initialized", path = p))
            }
            Error::TaskNotFound(id) => f.write_str(&t!("core.error.task_not_found", id = id)),
            Error::UnknownAdapter(name) => {
                f.write_str(&t!("core.error.unknown_adapter", name = name))
            }
            Error::InvalidConfig(m) | Error::Validation(m) => f.write_str(m),
            Error::Io(e) => e.fmt(f),
            Error::TomlSer(e) => e.fmt(f),
            Error::TomlDe(e) => e.fmt(f),
            Error::Json(e) => e.fmt(f),
            Error::Yaml(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Como `#[error(transparent)]`: la fuente es la del error envuelto.
        match self {
            Error::Io(e) => e.source(),
            Error::TomlSer(e) => e.source(),
            Error::TomlDe(e) => e.source(),
            Error::Json(e) => e.source(),
            Error::Yaml(e) => e.source(),
            _ => None,
        }
    }
}

macro_rules! from_error {
    ($variant:ident, $ty:ty) => {
        impl From<$ty> for Error {
            fn from(e: $ty) -> Self {
                Error::$variant(e)
            }
        }
    };
}

from_error!(Io, std::io::Error);
from_error!(TomlSer, toml::ser::Error);
from_error!(TomlDe, toml::de::Error);
from_error!(Json, serde_json::Error);
from_error!(Yaml, serde_yaml::Error);
