use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("no se encontró un proyecto arqueology-doc (falta arqueology-doc.toml). Ejecuta `adoc init` primero")]
    ProjectNotFound,

    #[error("el proyecto ya está inicializado: {0}")]
    AlreadyInitialized(String),

    #[error("la tarea {0} no existe")]
    TaskNotFound(String),

    #[error("el adaptador {0} no está definido en la configuración")]
    UnknownAdapter(String),

    #[error("{0}")]
    InvalidConfig(String),

    #[error("{0}")]
    Validation(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),

    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
}
