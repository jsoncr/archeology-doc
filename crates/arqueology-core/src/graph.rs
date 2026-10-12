//! Grafo del conocimiento: notas del vault y sus relaciones (como el grafo de
//! Obsidian), más los archivos de código citados como evidencia.
//!
//! Aristas:
//! - `link`: wikilinks `[[destino]]` del cuerpo y del frontmatter.
//! - `related`: campo `related: [id, ...]` (hallazgos relacionados).
//! - `module`: campo `modules: [nombre]` → nota `modules/<nombre>.md`.
//! - `tests`: un `test-report` con `feature: <id>` → la funcionalidad.
//! - `evidence`: `sources: [ruta:línea]` → nodo de código (`kind: code`).

use crate::frontmatter::NoteFrontmatter;
use crate::index::{source_file, source_lines, vault_notes};
use crate::project::Project;
use crate::validate::extract_wikilinks;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Serialize)]
pub struct Node {
    /// Ruta relativa al vault sin `.md` (`modules/billing`) o `code:<ruta>`.
    pub id: String,
    pub title: String,
    /// Tipo de nota (`feature`, `finding`…) o `code`.
    pub kind: String,
    pub status: Option<String>,
    /// Severidad (solo hallazgos).
    pub severity: Option<String>,
    /// Estado del hallazgo (open, resolved…).
    pub state: Option<String>,
    pub stale: bool,
    /// Archivo a abrir al hacer clic.
    pub file: PathBuf,
    /// Primera línea citada (nodos de código).
    pub line: Option<usize>,
}

#[derive(Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: String,
}

/// Construye el grafo completo del vault del proyecto.
pub fn build(project: &Project) -> Graph {
    let knowledge = project.knowledge_dir();
    let stale: BTreeSet<String> = crate::index::stale_notes(project).into_iter().collect();

    struct Parsed {
        id: String,
        fm: Option<NoteFrontmatter>,
        text: String,
        file: PathBuf,
    }
    let parsed: Vec<Parsed> = vault_notes(project)
        .into_iter()
        .filter_map(|file| {
            let text = fs::read_to_string(&file).ok()?;
            let id = file
                .strip_prefix(&knowledge)
                .ok()?
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            Some(Parsed {
                id,
                fm: NoteFrontmatter::parse(&text),
                text,
                file,
            })
        })
        .collect();

    // Resolución de destinos: ruta relativa, nombre de archivo e `id` del frontmatter.
    let mut by_path: BTreeMap<String, String> = BTreeMap::new();
    let mut by_stem: BTreeMap<String, String> = BTreeMap::new();
    let mut by_note_id: BTreeMap<String, String> = BTreeMap::new();
    for p in &parsed {
        by_path.insert(p.id.clone(), p.id.clone());
        let stem = p.id.rsplit('/').next().unwrap_or(&p.id).to_string();
        by_stem.entry(stem).or_insert_with(|| p.id.clone());
        if let Some(note_id) = p.fm.as_ref().and_then(|f| f.id.clone()) {
            by_note_id.insert(note_id, p.id.clone());
        }
    }
    let resolve = |target: &str| -> Option<String> {
        let t = target
            .split(['|', '#'])
            .next()?
            .trim()
            .trim_end_matches(".md");
        by_path
            .get(t)
            .or_else(|| by_stem.get(t))
            .or_else(|| by_note_id.get(t))
            .cloned()
    };

    let mut nodes = Vec::new();
    let mut edges: BTreeSet<Edge> = BTreeSet::new();
    let mut code: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let add = |source: &str, target: String, kind: &str, edges: &mut BTreeSet<Edge>| {
        if source != target {
            edges.insert(Edge {
                source: source.to_string(),
                target,
                kind: kind.to_string(),
            });
        }
    };

    for p in &parsed {
        let fm = p.fm.as_ref();
        let get = |k: &str| {
            fm.and_then(|f| f.get(k))
                .and_then(|v| v.as_str())
                .map(String::from)
        };
        let list = |k: &str| -> Vec<String> {
            fm.and_then(|f| f.get(k))
                .and_then(|v| v.as_sequence())
                .map(|s| {
                    s.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let kind = fm
            .and_then(|f| f.note_type.clone())
            .unwrap_or_else(|| "unknown".into());
        let rel = project.rel(&p.file);
        nodes.push(Node {
            id: p.id.clone(),
            title: fm
                .and_then(|f| f.title.clone())
                .unwrap_or_else(|| p.id.rsplit('/').next().unwrap_or(&p.id).to_string()),
            kind: kind.clone(),
            status: fm.and_then(|f| f.status.clone()),
            severity: get("severity"),
            state: get("state"),
            stale: stale.contains(&rel),
            file: p.file.clone(),
            line: None,
        });

        for target in extract_wikilinks(&p.text) {
            if let Some(t) = resolve(&target) {
                add(&p.id, t, "link", &mut edges);
            }
        }
        for r in list("related") {
            if let Some(t) = resolve(&r) {
                add(&p.id, t, "related", &mut edges);
            }
        }
        for m in list("modules") {
            if let Some(t) = resolve(&format!("modules/{m}")) {
                add(&p.id, t, "module", &mut edges);
            }
        }
        if kind == "test-report" {
            if let Some(t) = get("feature").and_then(|f| resolve(&f)) {
                add(&p.id, t, "tests", &mut edges);
            }
        }
        if let Some(f) = fm {
            for src in &f.sources {
                let file = source_file(src).to_string();
                let line = source_lines(src).map(|(a, _)| a);
                code.entry(file.clone()).or_insert(line);
                add(&p.id, format!("code:{file}"), "evidence", &mut edges);
            }
        }
    }

    for (file, line) in code {
        nodes.push(Node {
            id: format!("code:{file}"),
            title: file.clone(),
            kind: "code".into(),
            status: None,
            severity: None,
            state: None,
            stale: false,
            file: project.root.join(&file),
            line,
        });
    }

    Graph {
        nodes,
        edges: edges.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_links_relations_and_evidence() {
        let tmp = std::env::temp_dir().join(format!("arq-graph-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let know = tmp.join("knowledge");
        fs::create_dir_all(know.join("modules")).unwrap();
        fs::create_dir_all(know.join("features")).unwrap();
        fs::create_dir_all(know.join("findings")).unwrap();
        fs::create_dir_all(know.join("tests")).unwrap();
        fs::write(
            tmp.join("arqueology-doc.toml"),
            toml::to_string(&crate::config::ArqueologyConfig::new("g", "es")).unwrap(),
        )
        .unwrap();
        fs::write(
            know.join("modules/billing.md"),
            "---\nid: mod-billing\ntype: module\ntitle: Billing\n---\n",
        )
        .unwrap();
        fs::write(
            know.join("features/discount.md"),
            "---\nid: feat-discount\ntype: feature\ntitle: Descuento\nmodules: [billing]\nsources: [app/billing.py:7]\n---\nVer [[billing|módulo]].\n",
        )
        .unwrap();
        fs::write(
            know.join("findings/limit.md"),
            "---\nid: find-limit\ntype: finding\ntitle: Tope\nkind: contradiction\nseverity: high\nstate: open\nrelated: [feat-discount]\nsources: [app/billing.py:7]\n---\n",
        )
        .unwrap();
        fs::write(
            know.join("tests/discount.md"),
            "---\nid: test-discount\ntype: test-report\nfeature: feat-discount\n---\n",
        )
        .unwrap();
        std::env::set_var("ARQUEOLOGY_DOC_CONFIG_DIR", tmp.join("cfg"));
        let project = Project::open(&tmp).unwrap();
        let g = build(&project);
        let has = |s: &str, t: &str, k: &str| {
            g.edges
                .iter()
                .any(|e| e.source == s && e.target == t && e.kind == k)
        };
        assert!(has("features/discount", "modules/billing", "link"));
        assert!(has("features/discount", "modules/billing", "module"));
        assert!(has("findings/limit", "features/discount", "related"));
        assert!(has("tests/discount", "features/discount", "tests"));
        assert!(has("findings/limit", "code:app/billing.py", "evidence"));
        let code = g.nodes.iter().find(|n| n.kind == "code").unwrap();
        assert_eq!(code.line, Some(7));
        let finding = g.nodes.iter().find(|n| n.kind == "finding").unwrap();
        assert_eq!(finding.severity.as_deref(), Some("high"));
        fs::remove_dir_all(&tmp).ok();
    }
}
