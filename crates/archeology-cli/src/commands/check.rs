//! `check` — validaciones para CI (RF-130, RF-131).

use anyhow::Result;
use archeology_core::validate;
use archeology_core::{t, Project};
use clap::Args;
use serde::Serialize;

#[derive(Args)]
pub struct CheckArgs {
    /// Las advertencias también fallan.
    #[arg(long)]
    strict: bool,
    /// Verifica además que el sitio VitePress compila (RF-134).
    #[arg(long)]
    site: bool,
    /// Con --site, aprueba ejecutar la configuración de VitePress del proyecto
    /// (JavaScript del repositorio) sin preguntar.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Formato de salida.
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    format: String,
}

#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    errors: usize,
    warnings: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    issues: Vec<Issue<'a>>,
}

#[derive(Serialize)]
struct Issue<'a> {
    path: &'a str,
    severity: &'a str,
    message: &'a str,
}

pub fn run(project: &Project, args: CheckArgs) -> Result<i32> {
    let report = validate::run_checks(project, args.strict)?;

    if args.format == "json" {
        let mut issues = Vec::new();
        for i in &report.errors {
            issues.push(Issue {
                path: &i.path,
                severity: "error",
                message: &i.message,
            });
        }
        for i in &report.warnings {
            issues.push(Issue {
                path: &i.path,
                severity: "warning",
                message: &i.message,
            });
        }
        let out = Report {
            ok: report.is_ok() && (!args.strict || report.warnings.is_empty()),
            errors: report.errors.len(),
            warnings: report.warnings.len(),
            issues,
        };
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for i in &report.errors {
            eprintln!(
                "{}",
                t!("cli.check.error_line", path = i.path, message = i.message)
            );
        }
        for i in &report.warnings {
            eprintln!(
                "{}",
                t!("cli.check.warning_line", path = i.path, message = i.message)
            );
        }
        println!(
            "{}",
            t!(
                "cli.check.summary",
                errors = report.errors.len(),
                warnings = report.warnings.len()
            )
        );
    }

    let mut fail = !report.errors.is_empty() || (args.strict && !report.warnings.is_empty());
    if args.site {
        let built = super::site::sync(project)
            .and_then(|_| super::toolchain::run_vitepress(project, "build", args.yes));
        if let Err(e) = built {
            eprintln!("{}", t!("cli.check.site_build_failed", error = e));
            fail = true;
        }
    }
    Ok(if fail { 1 } else { 0 })
}
