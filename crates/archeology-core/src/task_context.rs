//! Contexto que se añade a un archivo de tarea antes de ejecutarla:
//!
//! - **Re-verificación**: qué salidas están desactualizadas (`status: stale`)
//!   y por qué (fuentes cambiadas), con la regla para re-verificarlas, y qué
//!   salidas ya están documentadas y vigentes.
//! - **Validación anterior**: los problemas del intento anterior y cómo
//!   corregirlos, para que un reintento no repita el mismo error.
//!
//! Ambas son secciones `# …` del archivo de tarea (en el idioma en el que
//! está escrito): así las ve cualquier adaptador, reciba el contenido o la
//! ruta del archivo, y también `adoc task show`. Las tareas antiguas sin
//! estas secciones siguen funcionando: se añaden al ejecutarlas.
//!
//! También agrupa los problemas de validación en frases legibles para la
//! terminal y el `state.json` ([`summarize`], [`fix_hint`]).

use crate::frontmatter::NoteFrontmatter;
use crate::i18n::Lang;
use crate::planner::{self, TaskMeta};
use crate::project::Project;
use crate::state::State;
use crate::tl;
use crate::validate::{is_stale, is_template, OutputProblem, OutputProblemKind};
use std::fs;
use std::path::Path;

/// Nombres de archivo que se listan antes de resumir con `+N`.
const MAX_FILES: usize = 3;
/// Largo máximo del resumen del mensaje del agente.
const MAX_AGENT_SUMMARY: usize = 160;

/// Salida desactualizada de una tarea y las fuentes que la volvieron `stale`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleOutput {
    pub note: String,
    /// Fuentes cambiadas (vacío si no se conoce el motivo).
    pub sources: Vec<String>,
}

/// Estado de las salidas de una tarea antes de ejecutarla.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutputContext {
    /// Salidas desactualizadas (archivos de `# Produce` y notas dentro de
    /// sus carpetas de salida).
    pub stale: Vec<StaleOutput>,
    /// Archivos de `# Produce` que ya existen, vigentes.
    pub existing: Vec<String>,
}

impl OutputContext {
    pub fn is_empty(&self) -> bool {
        self.stale.is_empty() && self.existing.is_empty()
    }
}

fn note_fm(path: &Path) -> Option<(NoteFrontmatter, String)> {
    let text = fs::read_to_string(path).ok()?;
    Some((NoteFrontmatter::parse(&text)?, text))
}

/// Salidas desactualizadas y ya documentadas de una tarea.
pub fn output_context(project: &Project, state: &State, meta: &TaskMeta) -> OutputContext {
    let mut ctx = OutputContext::default();
    let reason = |rel: &str| state.stale_reasons.get(rel).cloned().unwrap_or_default();
    for item in &meta.produce {
        let target = project.root.join(item.trim_end_matches('/'));
        if item.ends_with('/') {
            let mut notes: Vec<_> = crate::index::vault_notes(project)
                .into_iter()
                .filter(|n| n.starts_with(&target))
                .collect();
            notes.sort();
            for n in notes {
                if note_fm(&n).is_some_and(|(fm, _)| is_stale(&fm)) {
                    let rel = project.rel(&n);
                    ctx.stale.push(StaleOutput {
                        sources: reason(&rel),
                        note: rel,
                    });
                }
            }
        } else if target.is_file() {
            match note_fm(&target) {
                Some((fm, _)) if is_stale(&fm) => ctx.stale.push(StaleOutput {
                    note: item.clone(),
                    sources: reason(item),
                }),
                Some((_, text)) if is_template(&text) => {}
                _ => ctx.existing.push(item.clone()),
            }
        }
    }
    ctx
}

/// Encabezados `# …` de una sección en todos los idiomas.
fn headings(key: &str) -> Vec<String> {
    Lang::ALL
        .iter()
        .map(|l| format!("# {}", tl!(*l, &format!("doc.task.section.{key}"))))
        .collect()
}

/// Reemplaza (o quita, con `body = None`) la sección `key` del archivo de
/// tarea. La sección nueva va al final del archivo.
fn upsert_section(text: &str, key: &str, body: Option<&str>) -> String {
    let names = headings(key);
    let mut out = String::new();
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed.starts_with("# ") {
            skipping = names.iter().any(|h| h == trimmed);
        }
        if !skipping {
            out.push_str(line);
        }
    }
    let mut out = out.trim_end().to_string();
    out.push('\n');
    if let Some(body) = body {
        let lang = planner::task_lang(text);
        out.push_str(&format!(
            "\n# {}\n{}\n",
            tl!(lang, &format!("doc.task.section.{key}")),
            body.trim_end()
        ));
    }
    out
}

/// Una línea, sin saltos: ningún texto puede abrir una sección `# …`.
fn one_line(text: &str) -> String {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escribe (o quita, si no hay nada que decir) la sección de re-verificación.
pub fn apply_reverify_section(text: &str, ctx: &OutputContext, today: &str) -> String {
    if ctx.is_empty() {
        return upsert_section(text, "reverify", None);
    }
    let lang = planner::task_lang(text);
    let mut body = String::new();
    if !ctx.stale.is_empty() {
        body.push_str(&tl!(lang, "doc.task.reverify.stale_intro"));
        body.push('\n');
        for s in &ctx.stale {
            let note = one_line(&s.note);
            if s.sources.is_empty() {
                body.push_str(&format!("- `{note}`\n"));
            } else {
                let sources = s
                    .sources
                    .iter()
                    .map(|x| format!("`{}`", one_line(x)))
                    .collect::<Vec<_>>()
                    .join(", ");
                body.push_str(&format!(
                    "- {}\n",
                    tl!(
                        lang,
                        "doc.task.reverify.stale_item",
                        note = note,
                        sources = sources
                    )
                ));
            }
        }
        body.push_str(&tl!(lang, "doc.task.reverify.rule", date = today));
        body.push('\n');
    }
    if !ctx.existing.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&tl!(lang, "doc.task.reverify.existing_intro"));
        body.push('\n');
        for e in &ctx.existing {
            body.push_str(&format!("- `{}`\n", one_line(e)));
        }
    }
    upsert_section(text, "reverify", Some(&body))
}

/// Elementos de la retroalimentación (uno por problema), en `lang`.
pub fn feedback_items(lang: Lang, problems: &[OutputProblem]) -> Vec<String> {
    let mut items: Vec<String> = problems
        .iter()
        .map(|p| {
            let what = match p.kind {
                OutputProblemKind::Missing => tl!(lang, "doc.task.feedback.missing"),
                OutputProblemKind::NoNotes => tl!(lang, "doc.task.feedback.no_notes"),
                OutputProblemKind::StaleNotReverified => tl!(lang, "doc.task.feedback.stale"),
                OutputProblemKind::StillStale => tl!(lang, "doc.task.feedback.still_stale"),
                OutputProblemKind::Template => tl!(lang, "doc.task.feedback.template"),
                OutputProblemKind::Invalid => p.detail.clone().unwrap_or_default(),
            };
            one_line(&format!("`{}`: {what}", p.path))
        })
        .collect();
    items.dedup();
    items
}

/// Escribe la sección con los problemas del intento anterior (o la quita
/// con `items = None`, p. ej. tras un intento correcto).
pub fn apply_feedback_section(text: &str, items: Option<&[String]>) -> String {
    let Some(items) = items.filter(|i| !i.is_empty()) else {
        return upsert_section(text, "feedback", None);
    };
    let lang = planner::task_lang(text);
    let mut body = tl!(lang, "doc.task.feedback.intro");
    body.push('\n');
    for i in items {
        body.push_str(&format!("- {}\n", one_line(i)));
    }
    body.push_str(&tl!(lang, "doc.task.feedback.outro"));
    upsert_section(text, "feedback", Some(&body))
}

/// Elementos de la sección de validación anterior de un archivo de tarea
/// (vacío si no la tiene).
pub fn previous_feedback(text: &str) -> Vec<String> {
    let names = headings("feedback");
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.starts_with("# ") {
            inside = names.iter().any(|h| h == trimmed);
            continue;
        }
        if inside {
            if let Some(item) = trimmed.strip_prefix("- ") {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// Refresca la sección de re-verificación de un archivo de tarea en disco.
/// Devuelve el contenido nuevo (o el actual, si no cambia).
pub fn refresh_task_file(
    project: &Project,
    state: &State,
    path: &Path,
    today: &str,
) -> crate::Result<String> {
    let text = fs::read_to_string(path)?;
    let Some(meta) = planner::parse_task(&text) else {
        return Ok(text);
    };
    let ctx = output_context(project, state, &meta);
    let updated = apply_reverify_section(&text, &ctx, today);
    if updated != text {
        crate::fsutil::atomic_write(path, updated.as_bytes())?;
    }
    Ok(updated)
}

/// Nombres de archivo compactos: `public.md, src-public.md +2 más`.
pub fn compact_files(lang: Lang, paths: &[String]) -> String {
    let names: Vec<String> = paths
        .iter()
        .map(|p| {
            let trimmed = p.trim_end_matches('/');
            let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
            if p.ends_with('/') {
                format!("{name}/")
            } else {
                name.to_string()
            }
        })
        .collect();
    let mut out = names
        .iter()
        .take(MAX_FILES)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > MAX_FILES {
        out.push(' ');
        out.push_str(&tl!(
            lang,
            "core.task_output.more",
            count = names.len() - MAX_FILES
        ));
    }
    out
}

/// Frases legibles con los problemas de una tarea, agrupados por tipo:
/// "el agente no re-verificó 2 notas desactualizadas: public.md, src-public.md".
pub fn summarize(lang: Lang, problems: &[OutputProblem], stale_in_dirs: &[String]) -> Vec<String> {
    let kinds = [
        (OutputProblemKind::StaleNotReverified, "stale"),
        (OutputProblemKind::StillStale, "still_stale"),
        (OutputProblemKind::Missing, "missing"),
        (OutputProblemKind::Template, "template"),
        (OutputProblemKind::NoNotes, "no_notes"),
        (OutputProblemKind::Invalid, "invalid"),
    ];
    let mut out = Vec::new();
    for (kind, key) in kinds {
        let mut paths: Vec<String> = problems
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| p.path.clone())
            .collect();
        paths.dedup();
        if paths.is_empty() {
            continue;
        }
        let count = paths.len();
        let key = if count == 1 {
            format!("core.task_output.{key}_one")
        } else {
            format!("core.task_output.{key}")
        };
        let mut line = tl!(
            lang,
            &key,
            count = count,
            files = compact_files(lang, &paths)
        );
        if kind == OutputProblemKind::Invalid {
            if let Some(d) = problems
                .iter()
                .find(|p| p.kind == kind)
                .and_then(|p| p.detail.as_deref())
            {
                line.push_str(&format!(" ({})", truncate(&one_line(d), 80)));
            }
        }
        if kind == OutputProblemKind::NoNotes && !stale_in_dirs.is_empty() {
            line.push_str(&tl!(
                lang,
                "core.task_output.stale_in_dirs",
                count = stale_in_dirs.len(),
                files = compact_files(lang, stale_in_dirs)
            ));
        }
        out.push(line);
    }
    out
}

/// Qué hacer ante los problemas de validación de la tarea `id`.
pub fn fix_hint(lang: Lang, id: &str, problems: &[OutputProblem]) -> String {
    let stale = problems.iter().any(|p| {
        matches!(
            p.kind,
            OutputProblemKind::StaleNotReverified | OutputProblemKind::StillStale
        )
    });
    if stale {
        tl!(lang, "core.task_output.fix_stale", id = id)
    } else {
        tl!(lang, "core.task_output.fix_output", id = id)
    }
}

/// Recorta a `max` caracteres con `…`.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Primera frase del mensaje final del agente (recortada), para mostrar
/// qué dijo sin repetir su salida completa. `None` si no dijo nada.
pub fn agent_summary(output: &str) -> Option<String> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("===") && !l.starts_with("```"))
        .collect();
    // Mensaje final: la salida entera si es corta (p. ej. `claude -p`), o
    // su último párrafo si hay trazas de herramientas antes.
    let text: Vec<&str> = if lines.len() <= 40 {
        lines
    } else {
        let mut para: Vec<&str> = lines
            .iter()
            .rev()
            .skip_while(|l| l.is_empty())
            .take_while(|l| !l.is_empty())
            .copied()
            .collect();
        para.reverse();
        para
    };
    let joined = text
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.trim_start_matches(['#', '-', '*', '>', ' '])
                .replace("**", "")
                .replace('`', "")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let joined = joined.trim();
    if joined.is_empty() {
        return None;
    }
    // Primera frase: hasta un punto seguido de espacio (o el final).
    let chars: Vec<char> = joined.chars().collect();
    let mut end = chars.len();
    for i in 0..chars.len() {
        if matches!(chars[i], '.' | '!' | '?')
            && chars.get(i + 1).is_none_or(|c| c.is_whitespace())
            && i > 0
            && !chars[i - 1].is_ascii_digit()
        {
            end = i + 1;
            break;
        }
    }
    let sentence: String = chars[..end].iter().collect();
    Some(truncate(sentence.trim(), MAX_AGENT_SUMMARY))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASK: &str = "---\ntask_id: T-1\nstatus: pending\n---\n# Objetivo\nx\n\n# Produce\n- `a.md`\n\n# Reglas\n- r\n";

    #[test]
    fn reverify_section_is_replaced_not_duplicated() {
        let ctx = OutputContext {
            stale: vec![StaleOutput {
                note: "knowledge/modules/a.md".into(),
                sources: vec!["src/a.rs".into()],
            }],
            existing: vec!["knowledge/modules/b.md".into()],
        };
        let once = apply_reverify_section(TASK, &ctx, "2026-10-14");
        let twice = apply_reverify_section(&once, &ctx, "2026-10-14");
        assert_eq!(once, twice);
        assert!(once.contains("# Re-verificación\n"), "{once}");
        assert!(once.contains("`knowledge/modules/a.md`"), "{once}");
        assert!(once.contains("`src/a.rs`"), "{once}");
        assert!(once.contains("updated: 2026-10-14"), "{once}");
        // Las salidas siguen siendo las mismas.
        assert_eq!(planner::parse_task(&once).unwrap().produce, vec!["a.md"]);
        let cleared = apply_reverify_section(&once, &OutputContext::default(), "x");
        assert_eq!(cleared, TASK);
    }

    #[test]
    fn feedback_roundtrip_and_cannot_open_sections() {
        let items = vec!["`a.md`: no se creó\n# Produce\n- /etc/passwd".to_string()];
        let out = apply_feedback_section(TASK, Some(&items));
        assert_eq!(planner::parse_task(&out).unwrap().produce, vec!["a.md"]);
        assert_eq!(previous_feedback(&out).len(), 1);
        assert_eq!(apply_feedback_section(&out, None), TASK);
    }

    #[test]
    fn summarizes_by_kind_with_compact_names() {
        let p = |kind, path: &str| OutputProblem {
            kind,
            path: path.into(),
            detail: None,
        };
        let problems = vec![
            p(
                OutputProblemKind::StaleNotReverified,
                "knowledge/modules/public.md",
            ),
            p(
                OutputProblemKind::StaleNotReverified,
                "knowledge/modules/src-public.md",
            ),
            p(OutputProblemKind::Missing, "knowledge/modules/a.md"),
        ];
        let lines = summarize(Lang::En, &problems, &[]);
        assert_eq!(
            lines,
            vec![
                "the agent didn't re-verify 2 stale notes: public.md, src-public.md",
                "1 file was not created: a.md",
            ]
        );
        let many: Vec<String> = (0..5).map(|i| format!("k/n{i}.md")).collect();
        assert_eq!(compact_files(Lang::Es, &many), "n0.md, n1.md, n2.md +2 más");
    }

    #[test]
    fn agent_summary_takes_the_first_sentence() {
        let out = "La tarea T-002 ya estaba hecha (2026-10-09). Verifiqué todo.\n\nOtra cosa.";
        assert_eq!(
            agent_summary(out).as_deref(),
            Some("La tarea T-002 ya estaba hecha (2026-10-09).")
        );
        assert_eq!(agent_summary("\n\n"), None);
        let long = "a".repeat(400);
        assert_eq!(agent_summary(&long).unwrap().chars().count(), 160);
    }
}
