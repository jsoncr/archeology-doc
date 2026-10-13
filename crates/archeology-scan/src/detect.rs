//! Tablas de detección: lenguajes, manifiestos, frameworks, tests.

/// Extensión → lenguaje.
pub fn language_for(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "rs" => "Rust",
        "py" | "pyi" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TypeScript",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "scala" | "sc" => "Scala",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "hs" => "Haskell",
        "lua" => "Lua",
        "r" => "R",
        "jl" => "Julia",
        "dart" => "Dart",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "sh" | "bash" => "Shell",
        "ps1" | "psm1" => "PowerShell",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "md" | "markdown" => "Markdown",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "tf" | "tfvars" => "Terraform",
        // Plantillas de vistas: contienen lógica de presentación.
        "ejs" | "hbs" | "handlebars" | "pug" | "jade" | "njk" | "liquid" | "mustache" | "twig"
        | "erb" | "j2" | "jinja" | "jinja2" | "astro" => "Templates",
        "mdx" => "Markdown",
        _ => return None,
    })
}

/// Nombre de archivo → manifiesto/herramienta.
pub fn manifest_for(file_name: &str) -> Option<(&'static str, &'static str)> {
    Some(match file_name {
        "package.json" => ("npm", "package.json"),
        "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" => ("npm", file_name_leak(file_name)),
        "Cargo.toml" => ("cargo", "Cargo.toml"),
        "Cargo.lock" => ("cargo", "Cargo.lock"),
        "requirements.txt" | "Pipfile" | "pyproject.toml" | "setup.py" | "setup.cfg"
        | "poetry.lock" => ("python", file_name_leak(file_name)),
        "pom.xml" => ("maven", "pom.xml"),
        "build.gradle" | "build.gradle.kts" | "settings.gradle" => {
            ("gradle", file_name_leak(file_name))
        }
        "composer.json" => ("composer", "composer.json"),
        "go.mod" => ("go", "go.mod"),
        "go.sum" => ("go", "go.sum"),
        "Gemfile" => ("bundler", "Gemfile"),
        "mix.exs" => ("mix", "mix.exs"),
        "pubspec.yaml" => ("pub", "pubspec.yaml"),
        "Package.swift" => ("spm", "Package.swift"),
        "packages.config" => ("nuget", "packages.config"),
        "Dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => {
            ("docker", file_name_leak(file_name))
        }
        "Makefile" | "makefile" => ("make", "Makefile"),
        "CMakeLists.txt" => ("cmake", "CMakeLists.txt"),
        _ => return None,
    })
}

fn file_name_leak(name: &str) -> &'static str {
    // Los nombres de archivo detectados son un conjunto cerrado; los
    // devolvemos como &'static para simplificar el modelo.
    match name {
        "package-lock.json" => "package-lock.json",
        "yarn.lock" => "yarn.lock",
        "pnpm-lock.yaml" => "pnpm-lock.yaml",
        "requirements.txt" => "requirements.txt",
        "Pipfile" => "Pipfile",
        "pyproject.toml" => "pyproject.toml",
        "setup.py" => "setup.py",
        "setup.cfg" => "setup.cfg",
        "poetry.lock" => "poetry.lock",
        "build.gradle" => "build.gradle",
        "build.gradle.kts" => "build.gradle.kts",
        "settings.gradle" => "settings.gradle",
        "go.sum" => "go.sum",
        "docker-compose.yml" => "docker-compose.yml",
        "docker-compose.yaml" => "docker-compose.yaml",
        "makefile" => "makefile",
        _ => "unknown",
    }
}

/// Frameworks inferidos a partir de manifiestos y archivos.
pub fn framework_hints(path: &str, content_peek: Option<&str>) -> Vec<&'static str> {
    let mut out = Vec::new();
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if name == "package.json" {
        if let Some(c) = content_peek {
            for (needle, fw) in [
                ("\"react\"", "React"),
                ("\"vue\"", "Vue"),
                ("\"next\"", "Next.js"),
                ("\"express\"", "Express"),
                ("\"@angular/core\"", "Angular"),
                ("\"svelte\"", "Svelte"),
                ("\"vitepress\"", "VitePress"),
                ("\"vite\"", "Vite"),
                ("\"jest\"", "Jest"),
                ("\"vitest\"", "Vitest"),
                ("\"mocha\"", "Mocha"),
                ("\"fastify\"", "Fastify"),
                ("\"nestjs\"", "NestJS"),
                ("\"electron\"", "Electron"),
            ] {
                if c.contains(needle) {
                    out.push(fw);
                }
            }
        }
    }
    if name == "Cargo.toml" {
        if let Some(c) = content_peek {
            for (needle, fw) in [
                ("tokio", "Tokio"),
                ("axum", "Axum"),
                ("actix", "Actix"),
                ("clap", "clap"),
                ("serde", "serde"),
                ("wasm", "wasm"),
                ("tauri", "Tauri"),
            ] {
                if c.contains(needle) {
                    out.push(fw);
                }
            }
        }
    }
    if name == "requirements.txt" || name == "pyproject.toml" {
        if let Some(c) = content_peek {
            for (needle, fw) in [
                ("django", "Django"),
                ("flask", "Flask"),
                ("fastapi", "FastAPI"),
                ("pytest", "pytest"),
                ("numpy", "NumPy"),
                ("pandas", "pandas"),
                ("sqlalchemy", "SQLAlchemy"),
            ] {
                if c.to_lowercase().contains(needle) {
                    out.push(fw);
                }
            }
        }
    }
    out
}

/// ¿Es un archivo de test? (RF-014)
pub fn is_test_file(path: &str) -> bool {
    // Prefijo "/" para que `tests/` en la raíz también coincida.
    let lower = format!("/{}", path.to_lowercase());
    let name = lower.rsplit('/').next().unwrap_or("");
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.contains("/spec/")
        || name.starts_with("test_")
        || name.ends_with("_test.go")
        || name.ends_with("_test.rs")
        || name.ends_with("_test.py")
        || name.ends_with(".test.js")
        || name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.js")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.tsx")
        || name.ends_with("_spec.rb")
        || name.ends_with("test.php")
        || name.ends_with("test.java")
        || name.ends_with("tests.java")
}

/// ¿Es un posible punto de entrada? (RF-013)
pub fn is_entrypoint(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    matches!(
        name,
        "main.rs"
            | "main.py"
            | "main.go"
            | "main.java"
            | "Main.java"
            | "main.c"
            | "main.cpp"
            | "app.py"
            | "app.js"
            | "server.js"
            | "server.ts"
            | "index.js"
            | "index.ts"
            | "cli.py"
            | "cli.rs"
            | "cli.ts"
            | "__main__.py"
            | "worker.py"
            | "worker.js"
            | "Procfile"
            | "manage.py"
    ) || path.ends_with("/bin/")
        || name.starts_with("main.")
}

/// ¿Es un archivo de CI/CD? (RF-015)
pub fn is_ci_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with(".github/workflows/")
        || lower.starts_with(".gitlab-ci")
        || lower.starts_with(".circleci/")
        || lower.starts_with(".azure-pipelines")
        || lower.starts_with("azure-pipelines")
        || lower.contains("jenkinsfile")
        || lower.starts_with(".drone")
        || lower.starts_with("bitbucket-pipelines")
        || lower == ".travis.yml"
}

/// ¿Es documentación existente? (RF-006)
pub fn is_doc_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("").to_lowercase();
    let lower = path.to_lowercase();
    name.starts_with("readme")
        || name.starts_with("changelog")
        || name.starts_with("contributing")
        || name.starts_with("architecture")
        || name.starts_with("agents.md")
        || name.starts_with("claude.md")
        || lower.starts_with("docs/")
        || lower.starts_with("doc/")
        || lower.starts_with("wiki/")
}

/// Frameworks de testing detectados por convención.
pub fn test_framework_for(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with("_test.go") {
        return Some("go test");
    }
    if lower.ends_with(".test.js") || lower.ends_with(".spec.js") || lower.contains("__tests__") {
        return Some("jest/vitest");
    }
    if lower.ends_with(".test.ts") || lower.ends_with(".test.tsx") || lower.ends_with(".spec.ts") {
        return Some("jest/vitest");
    }
    if lower.starts_with("tests/") && lower.ends_with(".py")
        || lower.contains("/test_") && lower.ends_with(".py")
        || lower.rsplit('/').next().unwrap_or("").starts_with("test_") && lower.ends_with(".py")
    {
        return Some("pytest");
    }
    if lower.ends_with("_test.rs") || lower.contains("/tests/") && lower.ends_with(".rs") {
        return Some("cargo test");
    }
    if lower.ends_with("test.java") || lower.ends_with("tests.java") {
        return Some("junit");
    }
    if lower.ends_with(".feature") {
        return Some("gherkin");
    }
    None
}
