//! arqueology-core: modelos, configuración, estado, plantillas y validación.
//!
//! Este crate contiene la lógica compartida por el resto de componentes:
//! - [`config`]: carga y guardado de `arqueology-doc.toml`.
//! - [`context`]: AGENTS.md, adaptadores por herramienta y skills.
//! - [`paths`]: directorio de configuración y espacios de trabajo globales.
//! - [`project`]: descubrimiento del proyecto inspeccionado y sus rutas.
//! - [`state`]: estado incremental (`.arqueology-doc/state.json`).
//! - [`model`]: estructuras de datos del inventario y tareas.
//! - [`frontmatter`]: parsing y validación del frontmatter de las notas.
//! - [`templates`]: plantillas embebidas de notas, tareas y sitio.
//! - [`graph`]: grafo del conocimiento (notas, relaciones y evidencias).
//! - [`index`]: índice fuente → notas y marcado de notas `stale`.
//! - [`managed`]: regiones gestionadas en archivos compartidos (p. ej. `AGENTS.md`).
//! - [`site`]: preparación del sitio VitePress y de `llms.txt`.
//! - [`planner`]: división del proyecto en tareas acotadas.
//! - [`validate`]: validaciones del comando `check`.
//! - [`trust`]: aprobación de los adaptadores definidos en el repositorio.

pub mod config;
pub mod context;
pub mod error;
pub mod frontmatter;
pub mod graph;
pub mod index;
pub mod managed;
pub mod model;
pub mod paths;
pub mod planner;
pub mod project;
pub mod site;
pub mod state;
pub mod templates;
pub mod trust;
pub mod validate;

pub use config::ArqueologyConfig;
pub use error::{Error, Result};
pub use project::Project;
