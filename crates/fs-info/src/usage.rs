//! Credentials and response parsing for the AI usage panel's two providers.
//! Shapes verified against Anthropic's OAuth usage endpoint and Codex's
//! app-server JSON-RPC replies. No XHR and no process access in here.

use std::fmt;
use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use regex::Regex;
use serde_json::Value;

use fs_js as js;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    MalformedJson,
    MissingFields,
}

impl ParseError {
    pub fn as_str(self) -> &'static str {
        match self {
            ParseError::MalformedJson => "malformed_json",
            ParseError::MissingFields => "missing_fields",
        }
    }
}

fn string_or_empty(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_owned()
}

#[derive(Clone, PartialEq)]
pub struct Credentials {
    pub access_token: String,
    /// The refresh token itself is never returned, only whether one is
    /// present, which is all the caller needs to tell "logged out" apart from
    /// "logged in, access token needs refreshing" (see `credentials_expired`).
    pub has_refresh_token: bool,
    /// 0 when the credentials carry no expiry info at all.
    pub expires_at_ms: f64,
    pub subscription_type: String,
    pub rate_limit_tier: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("access_token", &"<redacted>")
            .field("has_refresh_token", &self.has_refresh_token)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

fn normalize_expires_at_ms(v: Option<&Value>) -> f64 {
    let n = js::to_number(v);
    if n.is_finite() && n > 0.0 { n } else { 0.0 }
}

/// `~/.claude/.credentials.json`'s `.claudeAiOauth` object.
pub fn parse_credentials(body: &str) -> Result<Credentials, ParseError> {
    let data: Value = serde_json::from_str(body).map_err(|_| ParseError::MalformedJson)?;
    if !data.is_object() {
        return Err(ParseError::MalformedJson);
    }
    let oauth = data.get("claudeAiOauth").filter(|o| o.is_object());
    let oauth = oauth.ok_or(ParseError::MissingFields)?;

    let access_token = string_or_empty(oauth.get("accessToken"));
    if access_token.is_empty() {
        return Err(ParseError::MissingFields);
    }
    Ok(Credentials {
        access_token,
        has_refresh_token: !string_or_empty(oauth.get("refreshToken")).is_empty(),
        expires_at_ms: normalize_expires_at_ms(oauth.get("expiresAt")),
        subscription_type: string_or_empty(oauth.get("subscriptionType")),
        rate_limit_tier: string_or_empty(oauth.get("rateLimitTier")),
    })
}

/// 0 expiry means the credentials carry no expiry info at all, not the same as
/// "expired now".
///
/// Claude Code's accessToken lives ~12h while its refreshToken lives ~10d, and
/// only a Claude Code run refreshes the pair on disk. An expired accessToken
/// therefore means "logged in, token needs refreshing", NOT "logged out": the
/// caller renders those as different states. This is advisory only: the local
/// clock decides which label shows while a probe is in flight, the server's own
/// 401 decides the settled state.
pub fn credentials_expired(expires_at_ms: f64, now_ms: f64) -> bool {
    expires_at_ms > 0.0 && expires_at_ms <= now_ms
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageRow {
    pub label: String,
    /// 0..1.
    pub percent: f64,
    /// ISO-8601, or "" when the window carries no reset time.
    pub resets_at: String,
}

/// GET https://api.anthropic.com/api/oauth/usage's body: a flat object whose
/// keys are rate-limit windows, each shaped `{utilization, resets_at}`:
/// `five_hour`, `seven_day`, and per-model keys like
/// `seven_day_opus`/`seven_day_sonnet` that the endpoint adds without notice.
/// Every such key is rendered, so a future bucket shows up the day the API adds
/// it with zero code changes here.
pub fn parse_usage(body: &str) -> Result<Vec<UsageRow>, ParseError> {
    let payload: Value = serde_json::from_str(body).map_err(|_| ParseError::MalformedJson)?;
    let Some(object) = payload.as_object() else {
        return Err(ParseError::MalformedJson);
    };

    let keys = rate_window_keys(object);
    if keys.is_empty() {
        return Err(ParseError::MissingFields);
    }
    let percent_scale = uses_percent_scale(keys.iter().map(|k| &object[*k]));

    let rows: Vec<UsageRow> = keys
        .iter()
        .filter_map(|k| usage_row(&bucket_label(k), &object[*k], percent_scale))
        .collect();
    if rows.is_empty() {
        return Err(ParseError::MissingFields);
    }
    Ok(rows)
}

/// Every own key whose value looks like a rate window (an object carrying a
/// `utilization` field), ordered `five_hour`, `seven_day`, then alphabetical,
/// stable regardless of the source object's own key order.
fn rate_window_keys(payload: &serde_json::Map<String, Value>) -> Vec<&str> {
    let candidates: Vec<&str> = payload
        .iter()
        .filter(|(_, v)| v.as_object().is_some_and(|o| o.contains_key("utilization")))
        .map(|(k, _)| k.as_str())
        .collect();
    let head = ["five_hour", "seven_day"].into_iter().filter(|k| candidates.contains(k));
    let mut rest: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|k| *k != "five_hour" && *k != "seven_day")
        .collect();
    rest.sort_unstable();
    head.chain(rest).collect()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// five_hour is "5-hour", seven_day is "Weekly", seven_day_<x> is "Weekly <x>"
/// (covers seven_day_opus/seven_day_sonnet and any future per-model window),
/// anything else is sentence case with underscores turned to spaces.
fn bucket_label(key: &str) -> String {
    match key {
        "five_hour" => "5-hour".into(),
        "seven_day" => "Weekly".into(),
        _ => match key.strip_prefix("seven_day_") {
            Some(model) => capitalize(&format!("weekly {}", model.replace('_', " "))),
            None => capitalize(&key.replace('_', " ")),
        },
    }
}

/// The endpoint has been observed to report both percent-scaled (37.0) and
/// fraction-scaled (0.37) utilization; a payload containing any value >= 1 is
/// treated as percent-scaled across every bucket in it, so a session at 0%
/// never gets misread as a fraction while a weekly bucket in the same response
/// is percent-scaled.
fn uses_percent_scale<'a>(buckets: impl Iterator<Item = &'a Value>) -> bool {
    buckets
        .map(|b| js::to_number(b.get("utilization")))
        .any(|n| n.is_finite() && n >= 1.0)
}

fn usage_row(label: &str, bucket: &Value, percent_scale: bool) -> Option<UsageRow> {
    let utilization = bucket.get("utilization").filter(|u| !u.is_null())?;
    let n = js::to_number(Some(utilization));
    if !n.is_finite() || n < 0.0 {
        return None;
    }
    Some(UsageRow {
        label: label.to_owned(),
        percent: if percent_scale { (n / 100.0).min(1.0) } else { n.min(1.0) },
        resets_at: string_or_empty(bucket.get("resets_at")),
    })
}

/// A rateLimitTier of "max_20x" reads as "Max 20x"; otherwise fall back to a
/// capitalized subscriptionType.
pub fn tier_label(subscription_type: &str, rate_limit_tier: &str) -> String {
    static MAX_TIER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)max_([0-9]+x)").unwrap());
    if let Some(m) = MAX_TIER.captures(rate_limit_tier) {
        return format!("Max {}", &m[1]);
    }
    capitalize(subscription_type)
}

fn parse_instant_ms(iso: &str) -> Option<i64> {
    if let Ok(t) = DateTime::parse_from_rfc3339(iso) {
        return Some(t.timestamp_millis());
    }
    // A date-only string is UTC midnight, a date-time with no offset is local.
    if let Ok(d) = NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().timestamp_millis());
    }
    let naive = NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    Some(Local.from_local_datetime(&naive).earliest()?.timestamp_millis())
}

/// "Resets 2H 14M" / "Resets 1D 3H" / "Resets now" / "" for no timestamp.
pub fn format_reset(now_ms: f64, resets_at_iso: &str) -> String {
    if resets_at_iso.is_empty() {
        return String::new();
    }
    let Some(reset) = parse_instant_ms(resets_at_iso) else {
        return String::new();
    };
    let diff_ms = reset as f64 - now_ms;
    if diff_ms <= 0.0 {
        return "Resets now".into();
    }

    let total_mins = (diff_ms / 60000.0).floor() as u64;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours > 24 {
        format!("Resets {}D {}H", hours / 24, hours % 24)
    } else if hours > 0 {
        format!("Resets {hours}H {mins}M")
    } else {
        format!("Resets {mins}M")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshState {
    Idle,
    Running,
    Ok,
    NoCli,
    Failed,
}

/// How the panel's refresh helper ended, from its exit code. 127 is the
/// `command -v claude` guard's own code (no CLI installed at all); any other
/// non-zero is the CLI itself answering that it cannot help: logged out, dead
/// refresh token, no network.
pub fn refresh_state_for_exit(exit_code: i32) -> RefreshState {
    match exit_code {
        0 => RefreshState::Ok,
        127 => RefreshState::NoCli,
        _ => RefreshState::Failed,
    }
}

/// The half of the panel's STALE line that names what is left to do. `Ok`
/// lands on the same text as `Idle` on purpose: the helper ran clean and the
/// token is still stale, so the refresh was not the missing piece and a real
/// `claude` run is still the answer.
pub fn refresh_hint(state: RefreshState) -> &'static str {
    match state {
        RefreshState::Running => "Refreshing",
        RefreshState::NoCli => "No Claude CLI",
        RefreshState::Failed => "Run claude auth login",
        RefreshState::Idle | RefreshState::Ok => "Run claude to refresh",
    }
}

/// The `result` of one line of the app-server's stdout, already framed as
/// newline-delimited JSON (no Content-Length headers).
fn rpc_result(body: &str) -> Result<Value, ParseError> {
    let msg: Value = serde_json::from_str(body).map_err(|_| ParseError::MalformedJson)?;
    if !msg.is_object() {
        return Err(ParseError::MalformedJson);
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

/// Reply to `account/read`: `{id, result: {account}}`. The plan type.
pub fn parse_codex_account(body: &str) -> Result<String, ParseError> {
    let result = rpc_result(body)?;
    let account = result.get("account").filter(|a| a.is_object());
    let account = account.ok_or(ParseError::MissingFields)?;
    Ok(match account.get("planType").and_then(Value::as_str) {
        Some(plan) => plan.to_owned(),
        None => string_or_empty(account.get("type")),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct CodexLimits {
    pub plan_type: String,
    pub rows: Vec<UsageRow>,
}

/// Reply to `account/rateLimits/read`: `{id, result: {rateLimits: {planType,
/// primary, secondary}}}`, each window `{usedPercent, windowDurationMins,
/// resetsAt}` (resetsAt is unix seconds).
pub fn parse_codex_rate_limits(body: &str) -> Result<CodexLimits, ParseError> {
    let result = rpc_result(body)?;
    let limits = result.get("rateLimits").filter(|l| l.is_object());
    let limits = limits.ok_or(ParseError::MissingFields)?;

    let rows: Vec<UsageRow> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| codex_window_row(limits.get(*k)))
        .collect();
    if rows.is_empty() {
        return Err(ParseError::MissingFields);
    }
    Ok(CodexLimits { plan_type: string_or_empty(limits.get("planType")), rows })
}

fn codex_window_row(window: Option<&Value>) -> Option<UsageRow> {
    let window = window.filter(|w| w.is_object())?;
    let used = js::to_number(window.get("usedPercent"));
    if !used.is_finite() || used < 0.0 {
        return None;
    }

    let mins = js::to_number(window.get("windowDurationMins"));
    let label = if mins.is_finite() && mins > 0.0 {
        if mins == 10080.0 {
            "Weekly".to_owned()
        } else if mins % 60.0 == 0.0 {
            format!("{}h window", js::num_str(mins / 60.0))
        } else {
            format!("{}m window", js::num_str(mins))
        }
    } else {
        "Window".to_owned()
    };

    let reset = js::to_number(window.get("resetsAt"));
    let resets_at = if reset.is_finite() && reset > 0.0 {
        DateTime::from_timestamp_millis((reset * 1000.0) as i64)
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };

    Some(UsageRow { label, percent: (used / 100.0).min(1.0), resets_at })
}
