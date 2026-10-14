//! Clasificación de los fallos del agente por límites: cuota o límite de uso,
//! límite de peticiones (rate limit), sesión sin autenticar y contexto
//! excedido.
//!
//! Un fallo así no se arregla reintentando: con un límite de uso, cada tarea
//! siguiente fallaría igual (y gastaría lo poco que queda); sin sesión, nada
//! va a funcionar hasta iniciarla. La clasificación mira el código de salida
//! y las últimas líneas de la salida del agente contra una única tabla de
//! patrones ([`PATTERNS`]).

use archeology_core::t;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};

/// Tipo de límite que alcanzó el agente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// Demasiadas peticiones en poco tiempo (429, "rate limit"): temporal.
    RateLimited,
    /// Cuota, límite de uso, de gasto o sin créditos: temporal o de facturación.
    UsageLimit,
    /// Sin sesión iniciada o API key inválida.
    Auth,
    /// La tarea no cabe en el contexto del modelo (prompt demasiado largo).
    Context,
}

impl LimitKind {
    /// ¿Detiene toda la corrida? (las siguientes tareas fallarían igual).
    pub fn stops_run(self) -> bool {
        !matches!(self, LimitKind::Context)
    }

    /// Identificador estable (logs, claves de traducción).
    pub fn code(self) -> &'static str {
        match self {
            LimitKind::RateLimited => "rate_limit",
            LimitKind::UsageLimit => "usage_limit",
            LimitKind::Auth => "auth",
            LimitKind::Context => "context",
        }
    }

    /// Nombre legible en el idioma de la interfaz.
    pub fn label(self) -> String {
        t!(&format!("agents.limit.kind.{}", self.code()))
    }
}

/// Límite detectado en la salida de un agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLimit {
    pub kind: LimitKind,
    /// Línea del agente que lo indica (recortada).
    pub message: String,
    /// Cuándo se restablece, si el agente lo dice.
    pub reset_at: Option<DateTime<Utc>>,
}

impl AgentLimit {
    /// Motivo para el log y el estado: `límite de uso del agente: <mensaje>`.
    pub fn reason(&self) -> String {
        t!(
            "agents.limit.reason",
            kind = self.kind.label(),
            message = self.message
        )
    }

    /// Línea para el log de la tarea.
    pub fn log_line(&self) -> String {
        t!(
            "agents.limit.log_line",
            kind = self.kind.code(),
            message = self.message,
            reset = self
                .reset_at
                .map(|r| t!("agents.limit.reset_suffix", when = r.to_rfc3339()))
                .unwrap_or_default()
        )
    }
}

/// Un patrón de la tabla: texto (en minúsculas) que, si aparece en una de las
/// últimas líneas de la salida, clasifica el fallo.
#[derive(Debug, Clone, Copy)]
pub struct Pattern {
    pub kind: LimitKind,
    pub needle: &'static str,
    /// Inequívoco: cuenta aunque el agente haya terminado con código 0 (solo
    /// en las últimas líneas). Los débiles exigen un código de salida ≠ 0.
    pub strong: bool,
    /// CLIs que lo emiten (documentación; no restringe la búsqueda).
    pub source: &'static str,
}

const fn p(kind: LimitKind, needle: &'static str, strong: bool, source: &'static str) -> Pattern {
    Pattern {
        kind,
        needle,
        strong,
        source,
    }
}

use LimitKind::{Auth, Context, RateLimited, UsageLimit};

/// Tabla única de patrones. El orden es la prioridad: gana el primer patrón
/// que aparezca (lo específico antes que lo genérico).
pub const PATTERNS: &[Pattern] = &[
    // --- Contexto: la tarea es demasiado grande para el modelo.
    p(Context, "prompt is too long", true, "claude, anthropic api"),
    p(
        Context,
        "context_length_exceeded",
        true,
        "codex, openai api, opencode, aider",
    ),
    p(
        Context,
        "context length exceeded",
        true,
        "openai api, litellm",
    ),
    p(Context, "maximum context length", true, "openai api, aider"),
    p(
        Context,
        "ran out of room in the model's context window",
        true,
        "codex",
    ),
    p(
        Context,
        "exceeds the context window",
        true,
        "codex, opencode",
    ),
    p(Context, "context window exceeded", true, "goose, opencode"),
    p(Context, "context limit reached", true, "claude"),
    p(
        Context,
        "exceeds the maximum number of tokens",
        true,
        "gemini, qwen",
    ),
    p(Context, "output token maximum", true, "claude"),
    p(Context, "input is too long", true, "anthropic api, bedrock"),
    p(Context, "request_too_large", false, "anthropic api"),
    p(Context, "input token count", false, "gemini"),
    p(Context, "token limit exceeded", false, "varios"),
    // --- Autenticación: sin sesión o con una API key inválida.
    p(Auth, "invalid api key", true, "claude"),
    p(Auth, "please run /login", true, "claude, copilot"),
    p(Auth, "not logged in", true, "claude, codex, cursor, devin"),
    p(Auth, "invalid x-api-key", true, "anthropic api"),
    p(
        Auth,
        "authentication_error",
        true,
        "anthropic api, opencode",
    ),
    p(
        Auth,
        "incorrect api key provided",
        true,
        "codex, openai api",
    ),
    p(Auth, "invalid_api_key", true, "openai api"),
    p(Auth, "api key not valid", true, "gemini"),
    p(Auth, "oauth token has expired", true, "claude"),
    p(Auth, "token has expired", false, "claude, copilot"),
    p(Auth, "please log in", false, "varios"),
    p(Auth, "please login", false, "varios"),
    p(Auth, "login required", false, "varios"),
    p(Auth, "not authenticated", false, "devin, copilot"),
    p(Auth, "authentication required", false, "varios"),
    p(Auth, "missing api key", false, "aider, goose"),
    p(Auth, "api key not found", false, "goose, opencode"),
    p(Auth, "gemini_api_key", false, "gemini"),
    p(Auth, "401 unauthorized", false, "varios"),
    // --- Límite de uso, cuota, gasto o créditos.
    p(
        UsageLimit,
        "usage limit reached",
        true,
        "claude (Claude AI usage limit reached|<epoch>)",
    ),
    p(UsageLimit, "hit your usage limit", true, "codex, cursor"),
    p(UsageLimit, "you've hit your", true, "claude, codex"),
    p(
        UsageLimit,
        "reached your usage limit",
        true,
        "cursor, copilot",
    ),
    p(UsageLimit, "api usage limits", true, "anthropic api"),
    p(
        UsageLimit,
        "exceeded your current quota",
        true,
        "codex, openai api",
    ),
    p(
        UsageLimit,
        "insufficient_quota",
        true,
        "codex, openai api, opencode",
    ),
    p(UsageLimit, "quota exceeded", true, "gemini, qwen, copilot"),
    p(UsageLimit, "resource_exhausted", true, "gemini"),
    p(UsageLimit, "exhausted your daily quota", true, "gemini"),
    p(UsageLimit, "spend limit", true, "cursor, anthropic console"),
    p(UsageLimit, "spending limit", true, "cursor, copilot"),
    p(UsageLimit, "monthly spend", true, "cursor"),
    p(
        UsageLimit,
        "credit balance is too low",
        true,
        "claude, anthropic api",
    ),
    p(
        UsageLimit,
        "insufficient credit",
        true,
        "amp, devin, openrouter",
    ),
    p(UsageLimit, "out of credits", true, "amp, devin"),
    p(UsageLimit, "premium request", false, "copilot"),
    p(UsageLimit, "usage limit", false, "varios"),
    p(UsageLimit, "daily quota", false, "gemini, qwen"),
    // --- Límite de peticiones (rate limit).
    p(
        RateLimited,
        "rate_limit_error",
        true,
        "claude, anthropic api, opencode",
    ),
    p(RateLimited, "rate_limit_exceeded", true, "openai api"),
    p(
        RateLimited,
        "ratelimiterror",
        true,
        "aider (litellm.RateLimitError)",
    ),
    p(RateLimited, "ratelimitexceeded", true, "goose"),
    p(RateLimited, "rate limit reached", true, "codex, openai api"),
    p(
        RateLimited,
        "rate limit exceeded",
        true,
        "gemini, goose, copilot",
    ),
    p(RateLimited, "too many requests", true, "429 (todos)"),
    p(RateLimited, "status 429", false, "codex"),
    p(RateLimited, "status: 429", false, "varios"),
    p(RateLimited, "status code 429", false, "varios"),
    p(RateLimited, "error 429", false, "varios"),
    p(RateLimited, "http 429", false, "varios"),
    p(RateLimited, "\"code\":429", false, "gemini (json)"),
    p(RateLimited, "\"code\": 429", false, "gemini (json)"),
    p(RateLimited, "rate limited", false, "copilot, amp"),
    p(RateLimited, "rate-limited", false, "varios"),
    p(RateLimited, "rate limit", false, "varios"),
    p(RateLimited, "throttl", false, "qwen, bedrock"),
    // --- Genéricos (al final: solo si nada más específico coincide).
    p(UsageLimit, "limit reached", false, "varios"),
    p(UsageLimit, "quota", false, "varios"),
    p(RateLimited, "try again later", false, "varios"),
];

/// Líneas finales que se revisan cuando el agente falló (código ≠ 0).
const TAIL_LINES: usize = 40;
/// Líneas finales que se revisan cuando terminó con código 0: el mensaje
/// final del agente puede hablar de "rate limit" al documentar código.
const TAIL_LINES_OK: usize = 3;
/// Largo máximo del mensaje del agente que se guarda.
const MAX_MESSAGE: usize = 300;

/// Clasifica la salida de un agente. `exit_ok`: terminó con código 0 (la
/// validación falló): solo cuentan los patrones inequívocos en las últimas
/// líneas.
pub fn classify(output: &str, exit_ok: bool) -> Option<AgentLimit> {
    classify_at(output, exit_ok, Utc::now())
}

/// [`classify`] con la hora actual explícita (para calcular el reinicio).
pub fn classify_at(output: &str, exit_ok: bool, now: DateTime<Utc>) -> Option<AgentLimit> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let take = if exit_ok { TAIL_LINES_OK } else { TAIL_LINES };
    let tail = &lines[lines.len().saturating_sub(take)..];
    // Apóstrofo tipográfico (`You’ve hit your limit`) como el ASCII.
    let lower: Vec<String> = tail
        .iter()
        .map(|l| l.to_lowercase().replace('\u{2019}', "'"))
        .collect();
    let (pattern, line) = PATTERNS
        .iter()
        .filter(|p| p.strong || !exit_ok)
        .find_map(|p| {
            lower
                .iter()
                .rposition(|l| l.contains(p.needle))
                .map(|i| (p, tail[i]))
        })?;
    let text = tail.join("\n");
    Some(AgentLimit {
        kind: pattern.kind,
        message: line.chars().take(MAX_MESSAGE).collect(),
        reset_at: if pattern.kind == Context || pattern.kind == Auth {
            None
        } else {
            parse_reset(&text, now)
        },
    })
}

/// Hora de reinicio que menciona el agente:
/// - `…limit reached|1760466000` (Claude Code: epoch en segundos);
/// - `try again in 4 days 3 hours`, `Please try again in 20s`, `retry after 30`;
/// - `resets at 2025-10-15T18:00:00Z`, `regain access on 2025-11-01 at 00:00 UTC`;
/// - `resets 3pm`, `try again at 3:04 PM`, `resets at 15:30` (hora local, la
///   próxima vez que llegue).
pub fn parse_reset(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let lower = text.to_lowercase();
    if let Some(t) = epoch_after_pipe(&lower) {
        return Some(t);
    }
    const RELATIVE: &[&str] = &[
        "try again in",
        "retry in",
        "retry after",
        "retry-after:",
        "resets in",
        "reset in",
        "available in",
    ];
    const ABSOLUTE: &[&str] = &[
        "resets at",
        "reset at",
        "resets on",
        "reset on",
        "regain access on",
        "try again at",
        "try again after",
        "available again at",
        "retry after",
        "resets",
    ];
    for kw in RELATIVE {
        for rest in after_keyword(&lower, kw) {
            if let Some(d) = parse_duration(rest) {
                return now.checked_add_signed(d);
            }
        }
    }
    for kw in ABSOLUTE {
        for rest in after_keyword(&lower, kw) {
            if let Some(t) = parse_absolute(rest, now) {
                return Some(t);
            }
        }
    }
    None
}

/// Texto que sigue a cada aparición de `kw`.
fn after_keyword<'a>(text: &'a str, kw: &str) -> Vec<&'a str> {
    text.match_indices(kw)
        .map(|(i, _)| &text[i + kw.len()..])
        .collect()
}

/// `|1760466000` (o milisegundos) tras un mensaje de límite.
fn epoch_after_pipe(text: &str) -> Option<DateTime<Utc>> {
    text.match_indices('|').find_map(|(i, _)| {
        let digits: String = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let n: i64 = digits.parse().ok()?;
        let secs = match digits.len() {
            10 => n,
            13 => n / 1000,
            _ => return None,
        };
        // Plausible: desde 2020.
        (secs > 1_577_836_800)
            .then(|| Utc.timestamp_opt(secs, 0).single())
            .flatten()
    })
}

/// Duración en texto: `4 days 3 hours 2 minutes`, `1h2m3s`, `20s`, `1.5s`,
/// `30` (segundos). `None` si no empieza con una duración.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut total = 0f64;
    let mut parts = 0;
    let skip = |i: &mut usize| {
        loop {
            while *i < chars.len() && matches!(chars[*i], ' ' | ',' | ':' | '~' | '\t') {
                *i += 1;
            }
            // `and` entre partes: "1 hour and 5 minutes".
            if parts_word(&chars, *i, "and") {
                *i += 3;
                continue;
            }
            break;
        }
    };
    loop {
        skip(&mut i);
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
            i += 1;
        }
        if start == i {
            break;
        }
        let Ok(n) = chars[start..i].iter().collect::<String>().parse::<f64>() else {
            break;
        };
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let ustart = i;
        while i < chars.len() && chars[i].is_ascii_alphabetic() {
            i += 1;
        }
        let unit: String = chars[ustart..i].iter().collect();
        // `2025-10-15…` o `15:30` no son una duración sin unidad.
        if unit.is_empty() && matches!(chars.get(i), Some('-' | ':' | '/')) {
            break;
        }
        let mult = match unit.as_str() {
            "" if parts == 0 => 1.0,
            "ms" | "msec" | "msecs" => 0.001,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
            "d" | "day" | "days" => 86400.0,
            _ => break,
        };
        total += n * mult;
        parts += 1;
        if unit.is_empty() {
            break;
        }
    }
    (parts > 0 && total.is_finite() && total < 400.0 * 86400.0)
        .then(|| Duration::milliseconds((total * 1000.0).round() as i64))
}

/// ¿Empieza en `i` la palabra `w` (seguida de un no-alfabético)?
fn parts_word(chars: &[char], i: usize, w: &str) -> bool {
    let wc: Vec<char> = w.chars().collect();
    chars.len() >= i + wc.len()
        && chars[i..i + wc.len()] == wc[..]
        && chars
            .get(i + wc.len())
            .is_none_or(|c| !c.is_ascii_alphabetic())
}

/// Fecha u hora absoluta al inicio de `s`.
fn parse_absolute(s: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let tokens: Vec<String> = s
        .split_whitespace()
        .take(5)
        .map(|t| {
            t.trim_matches(|c: char| matches!(c, '.' | ',' | '(' | ')' | ';' | '"' | '\''))
                .to_string()
        })
        .filter(|t| !t.is_empty() && t != "at" && t != "on")
        .collect();
    let first = tokens.first()?;
    // RFC 3339 (`2025-10-15T18:00:00Z`).
    if let Ok(t) = DateTime::parse_from_rfc3339(&first.to_uppercase()) {
        return Some(t.with_timezone(&Utc));
    }
    // `2025-11-01 [at] 00:00 [utc]`.
    if let Ok(date) = NaiveDate::parse_from_str(first, "%Y-%m-%d") {
        let (time, used) =
            parse_clock(&tokens[1..]).unwrap_or((NaiveTime::from_hms_opt(0, 0, 0)?, 0));
        let naive = date.and_time(time);
        let utc = tokens
            .get(1 + used)
            .is_some_and(|t| t == "utc" || t == "gmt" || t == "z");
        return if utc {
            Some(Utc.from_utc_datetime(&naive))
        } else {
            Local
                .from_local_datetime(&naive)
                .earliest()
                .map(|t| t.with_timezone(&Utc))
        };
    }
    // Solo hora (`3pm`, `3:04 pm`, `15:30`): la próxima vez que llegue, en hora local.
    let (time, _) = parse_clock(&tokens)?;
    let local_now = now.with_timezone(&Local);
    let mut candidate = Local
        .from_local_datetime(&local_now.date_naive().and_time(time))
        .earliest()?;
    if candidate <= local_now {
        candidate = Local
            .from_local_datetime(&(local_now.date_naive() + Duration::days(1)).and_time(time))
            .earliest()?;
    }
    Some(candidate.with_timezone(&Utc))
}

/// Hora del día: `3pm`, `3 pm`, `3:04pm`, `3:04 p.m.`, `15:30`. Devuelve la
/// hora y cuántos tokens usó. Sin am/pm exige `HH:MM`.
fn parse_clock(tokens: &[String]) -> Option<(NaiveTime, usize)> {
    let first = tokens.first()?;
    let (body, mut suffix) = split_ampm(first);
    let mut used = 1;
    if suffix.is_none() {
        if let Some(next) = tokens.get(1) {
            let n = next.replace('.', "");
            if n == "am" || n == "pm" {
                suffix = Some(n == "pm");
                used = 2;
            }
        }
    }
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None if suffix.is_some() => (body.parse::<u32>().ok()?, 0),
        None => return None,
    };
    let h = match suffix {
        Some(pm) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            (h % 12) + if pm { 12 } else { 0 }
        }
        None => h,
    };
    Some((NaiveTime::from_hms_opt(h, m, 0)?, used))
}

/// `3pm` → (`3`, Some(true)); `3:04am` → (`3:04`, Some(false)); `15:30` → (`15:30`, None).
fn split_ampm(token: &str) -> (&str, Option<bool>) {
    let t = token.trim_end_matches('.');
    for (suffix, pm) in [("p.m", true), ("a.m", false), ("pm", true), ("am", false)] {
        if let Some(body) = t.strip_suffix(suffix) {
            if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit() || c == ':') {
                return (body, Some(pm));
            }
        }
    }
    (t, None)
}

/// Cuánto falta, compacto: `3d 4h`, `2h 5m`, `45m`, `<1m`.
pub fn human_duration(secs: i64) -> String {
    let secs = secs.max(0);
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        "<1m".to_string()
    }
}

/// Cuánto falta para el reinicio: `en 2h 5m` o `ya debería estar disponible`.
pub fn relative_reset(reset_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (reset_at - now).num_seconds();
    if secs > 0 {
        t!("agents.limit.relative_in", duration = human_duration(secs))
    } else {
        t!("agents.limit.relative_past")
    }
}

/// Reinicio legible: `en 2h 5m (2025-10-14 18:20, hora local)`.
pub fn describe_reset(reset_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let relative = relative_reset(reset_at, now);
    t!(
        "agents.limit.reset_when",
        relative = relative,
        local = reset_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    )
}

/// Cómo iniciar sesión en el CLI del agente (por nombre del ejecutable).
pub fn login_hint(program: &str) -> String {
    const KNOWN: &[&str] = &[
        "claude", "codex", "gemini", "qwen", "devin", "opencode", "agent", "copilot", "amp",
        "goose", "aider",
    ];
    let key = if KNOWN.contains(&program) {
        program
    } else {
        "generic"
    };
    t!(&format!("agents.limit.login.{key}"))
}

/// Cómo dividir una tarea que no cabe en el contexto del agente.
pub fn context_hint(max_files: usize, max_bytes: u64) -> String {
    t!(
        "agents.limit.context_hint",
        files = max_files,
        bytes = max_bytes
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 10, 14, 12, 0, 0).unwrap()
    }

    fn kind(out: &str) -> Option<LimitKind> {
        classify_at(out, false, now()).map(|l| l.kind)
    }

    /// Mensajes típicos de cada CLI → tipo de límite.
    #[test]
    fn classifies_known_agent_messages() {
        let cases: &[(&str, LimitKind)] = &[
            ("Claude AI usage limit reached|1760466000", UsageLimit),
            ("5-hour limit reached ∙ resets 3pm", UsageLimit),
            ("You've hit your limit · resets 3pm (America/Costa_Rica)", UsageLimit),
            ("You\u{2019}ve hit your limit \u{b7} resets 3pm", UsageLimit),
            ("Credit balance is too low", UsageLimit),
            ("API Error: 429 {\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}", RateLimited),
            ("Invalid API key · Please run /login", Auth),
            ("OAuth token has expired. Please obtain a new token or refresh your existing token.", Auth),
            ("API Error: 400 prompt is too long: 215000 tokens > 200000 maximum", Context),
            ("■ You've hit your usage limit. Upgrade to Pro or try again in 4 days 3 hours 2 minutes.", UsageLimit),
            ("ERROR: You exceeded your current quota, please check your plan and billing details.", UsageLimit),
            ("stream error: exceeded retry limit, last status: 429 Too Many Requests", RateLimited),
            ("Rate limit reached for gpt-5 in organization org-x on tokens per min. Please try again in 20s.", RateLimited),
            ("Codex ran out of room in the model's context window. Start a new conversation.", Context),
            ("error: Incorrect API key provided: sk-xxx", Auth),
            ("[API Error: got status: 429 Too Many Requests. {\"error\":{\"code\":429,\"status\":\"RESOURCE_EXHAUSTED\"}}]", UsageLimit),
            ("Quota exceeded for quota metric 'Gemini 2.5 Pro Requests' and limit per day", UsageLimit),
            ("API key not valid. Please pass a valid API key.", Auth),
            ("The input token count (1200000) exceeds the maximum number of tokens allowed (1048576).", Context),
            ("Error: Not logged in. Run `devin auth login`.", Auth),
            ("You've hit your usage limit for Pro. Switch to Auto or upgrade.", UsageLimit),
            ("Error: You have reached your monthly spend limit", UsageLimit),
            ("Error: Quota exceeded. You have no premium requests remaining.", UsageLimit),
            ("AI_APICallError: insufficient_quota", UsageLimit),
            ("ProviderAuthError: authentication_error", Auth),
            ("litellm.RateLimitError: AnthropicException - rate_limit_error", RateLimited),
            ("Error: Insufficient credit balance. Add credits at ampcode.com", UsageLimit),
            ("Error: Rate limit exceeded, please try again later", RateLimited),
            ("Throttling: Requests rate limit exceeded", RateLimited),
            ("Error: Free allocated quota exceeded.", UsageLimit),
        ];
        for (out, expected) in cases {
            assert_eq!(kind(out), Some(*expected), "{out}");
        }
        assert_eq!(kind("compilé y todo bien\nlisto"), None);
        assert_eq!(kind("error: archivo no encontrado"), None);
    }

    /// Con código 0 solo cuentan los patrones inequívocos al final.
    #[test]
    fn exit_ok_needs_strong_pattern_at_the_end() {
        let doc = "El módulo maneja el rate limit de la API.\nNotas escritas.";
        assert_eq!(classify_at(doc, true, now()), None);
        let mid = "Claude AI usage limit reached|1760466000\na\nb\nc\nd";
        assert_eq!(classify_at(mid, true, now()), None);
        let end = "trabajando...\nClaude AI usage limit reached|1760466000";
        assert_eq!(
            classify_at(end, true, now()).map(|l| l.kind),
            Some(UsageLimit)
        );
    }

    #[test]
    fn every_pattern_is_lowercase_and_classifies_itself() {
        for p in PATTERNS {
            assert_eq!(p.needle, p.needle.to_lowercase(), "{}", p.needle);
            assert!(
                classify_at(p.needle, false, now()).is_some(),
                "{}",
                p.needle
            );
        }
    }

    #[test]
    fn parses_reset_times() {
        let n = now();
        let l = classify_at("Claude AI usage limit reached|1760466000", false, n).unwrap();
        assert_eq!(l.reset_at, Utc.timestamp_opt(1_760_466_000, 0).single());
        assert_eq!(
            parse_reset("try again in 4 days 3 hours 2 minutes.", n),
            Some(n + Duration::seconds(4 * 86400 + 3 * 3600 + 120))
        );
        assert_eq!(
            parse_reset("Please try again in 1m30.5s.", n),
            Some(n + Duration::milliseconds(90_500))
        );
        assert_eq!(
            parse_reset("retry-after: 30", n),
            Some(n + Duration::seconds(30))
        );
        assert_eq!(
            parse_reset("limit resets at 2025-10-15T18:00:00Z", n),
            Some(Utc.with_ymd_and_hms(2025, 10, 15, 18, 0, 0).unwrap())
        );
        assert_eq!(
            parse_reset("You will regain access on 2025-11-01 at 00:00 UTC.", n),
            Some(Utc.with_ymd_and_hms(2025, 11, 1, 0, 0, 0).unwrap())
        );
        // Solo hora: la próxima vez que llegue en hora local.
        let t = parse_reset("5-hour limit reached ∙ resets 3pm", n).unwrap();
        let local = t.with_timezone(&Local);
        assert_eq!(local.format("%H:%M").to_string(), "15:00");
        assert!(t > n && t <= n + Duration::days(1));
        let t = parse_reset("try again at 3:04 PM", n).unwrap();
        assert_eq!(t.with_timezone(&Local).format("%H:%M").to_string(), "15:04");
        assert_eq!(parse_reset("try again later", n), None);
        assert_eq!(parse_reset("rate limit, resets soon", n), None);
    }

    #[test]
    fn durations_and_descriptions() {
        assert_eq!(parse_duration("2h 5m"), Some(Duration::seconds(7500)));
        assert_eq!(
            parse_duration("1 hour and 5 minutes"),
            Some(Duration::seconds(3900))
        );
        assert_eq!(parse_duration("soon"), None);
        assert_eq!(human_duration(3 * 86400 + 4 * 3600), "3d 4h");
        assert_eq!(human_duration(7500), "2h 5m");
        assert_eq!(human_duration(30), "<1m");
    }
}
