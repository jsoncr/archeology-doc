//! Acceso a las specs de OpenSpec del proyecto para `status`, `review` y
//! `check` (el comando `adoc spec` está desactivado temporalmente).

use anyhow::Result;
use archeology_core::Project;
use archeology_openspec::{ComplianceSummary, SpecProject};
use std::path::PathBuf;

/// Carga las specs del proyecto (None si no hay OpenSpec).
pub fn load_spec(project: &Project) -> Result<Option<SpecProject>> {
    let rel = PathBuf::from(&project.config.openspec.path);
    archeology_openspec::load(&project.root, &rel)
}

/// Cumplimiento cruzando specs con los `spec-report` del vault.
pub fn compliance(project: &Project, spec: &SpecProject) -> ComplianceSummary {
    let reports = archeology_openspec::load_reports(&project.knowledge_dir().join("specs"));
    archeology_openspec::compliance(spec, &reports, project.config.openspec.stale_change_days)
}
