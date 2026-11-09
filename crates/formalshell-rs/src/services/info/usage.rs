//! AI usage, Claude Code and Codex rate limits.
//! Claude: `~/.claude/.credentials.json` and Anthropic's OAuth usage
//! endpoint through curl, refreshing a stale pair by asking the `claude`
//! CLI for its auth status. Codex: one `codex app-server` JSON-RPC session
//! per poll. Each leg answers its own honest state; a provider switched off
//! in settings.json stays unknown.

use std::cell::RefCell;
use std::io::ErrorKind;
use std::rc::Rc;
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_info::usage::{self, RefreshState, UsageRow};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, FutureExt, StreamExt};
use serde_json::json;

use super::{idle, kicked};
use crate::runtime::Ctx;
use crate::services::config;
use crate::services::http;
use crate::services::proc;
use crate::services::wants::Source;
use crate::store;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Claude {
    #[default]
    Unknown,
    NoAuth,
    Stale,
    Loading,
    Error,
    Ok,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Codex {
    #[default]
    Unknown,
    Missing,
    Loading,
    Error,
    Ok,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    pub claude: Claude,
    pub claude_tier: String,
    pub claude_rows: Vec<UsageRow>,
    pub refresh: RefreshState,
    pub codex: Codex,
    pub codex_tier: String,
    pub codex_rows: Vec<UsageRow>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            claude_enabled: true,
            codex_enabled: true,
            claude: Claude::Unknown,
            claude_tier: String::new(),
            claude_rows: Vec::new(),
            refresh: RefreshState::Idle,
            codex: Codex::Unknown,
            codex_tier: String::new(),
            codex_rows: Vec::new(),
        }
    }
}

impl State {
    /// The cell shows once an enabled provider has answered at all: NO AUTH
    /// counts, only the pre-first-poll unknown does not.
    pub fn settled(&self) -> bool {
        (self.claude_enabled && self.claude != Claude::Unknown) || (self.codex_enabled && self.codex != Codex::Unknown)
    }

    fn rows(&self) -> impl Iterator<Item = &UsageRow> {
        let claude = (self.claude_enabled && self.claude == Claude::Ok).then_some(&self.claude_rows);
        let codex = (self.codex_enabled && self.codex == Codex::Ok).then_some(&self.codex_rows);
        claude.into_iter().chain(codex).flatten()
    }

    /// The highest window across providers, 0..1, or -1 with none.
    pub fn worst(&self) -> f64 {
        self.rows().map(|r| r.percent).fold(-1.0, f64::max)
    }

    /// The word a provider with no reading shows in the cell's place.
    pub fn status_label(&self) -> &'static str {
        if self.claude_enabled && !matches!(self.claude, Claude::Ok | Claude::Unknown) {
            return match self.claude {
                Claude::NoAuth => "No auth",
                Claude::Stale => "Stale",
                Claude::Loading => "Loading",
                _ => "Error",
            };
        }
        if self.codex_enabled && !matches!(self.codex, Codex::Ok | Codex::Unknown) {
            return match self.codex {
                Codex::Missing => "No Codex",
                Codex::Loading => "Loading",
                _ => "Error",
            };
        }
        ""
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Cfg {
    claude: bool,
    codex: bool,
    interval: Duration,
}

fn read() -> Cfg {
    let s = config::current();
    Cfg { claude: s.flag("usage.claude", true), codex: s.flag("usage.codex", true), interval: s.interval("usage.intervalMs", 900_000) }
}

/// The `claude` CLI rewrites its credentials file in place, which leaves it
/// missing for a moment: a read that finds nothing tries again this often.
const CREDENTIALS_READS: usize = 4;
const CREDENTIALS_RETRY: Duration = Duration::from_millis(300);

const REFRESH_COOLDOWN: Duration = Duration::from_secs(60);
const REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const CODEX_TIMEOUT: Duration = Duration::from_secs(20);

type Shared = Rc<RefCell<State>>;

struct Poll {
    ctx: Ctx,
    state: Shared,
    last_refresh: Option<Instant>,
}

impl Poll {
    fn update(&self, f: impl FnOnce(&mut State)) {
        let mut state = self.state.borrow_mut();
        f(&mut state);
        self.ctx.publish(store::Diff::Usage(state.clone()));
    }

    async fn credentials(&self) -> Option<usage::Credentials> {
        let path = format!("{}/.claude/.credentials.json", std::env::var("HOME").unwrap_or_default());
        for attempt in 0..CREDENTIALS_READS {
            let read = path.clone();
            match self.ctx.pool().run(move || std::fs::read_to_string(read)).await {
                Some(Ok(text)) => return usage::parse_credentials(&text).ok(),
                Some(Err(e)) if e.kind() == ErrorKind::NotFound && attempt + 1 < CREDENTIALS_READS => Timer::after(CREDENTIALS_RETRY).await,
                _ => return None,
            };
        }
        None
    }

    /// Asks the `claude` CLI to refresh its own pair. Held to a cooldown
    /// unless `force`, and answers whether it ran.
    async fn refresh(&mut self, force: bool) -> bool {
        if !force && self.last_refresh.is_some_and(|t| t.elapsed() < REFRESH_COOLDOWN) {
            return false;
        }
        self.last_refresh = Some(Instant::now());
        self.update(|s| s.refresh = RefreshState::Running);
        let done = proc::capture(&proc::argv(&["claude", "auth", "status", "--json"]), REFRESH_TIMEOUT).await;
        let ended = if done.code == proc::TIMED_OUT { RefreshState::Failed } else { usage::refresh_state_for_exit(done.code) };
        self.update(|s| s.refresh = ended);
        true
    }

    async fn usage(&self, token: &str) -> (i32, String) {
        let auth = format!("Authorization: Bearer {token}");
        let req = http::Request::get("https://api.anthropic.com/api/oauth/usage", 20)
            .header(auth)
            .header("anthropic-beta: oauth-2025-04-20")
            .header("Accept: application/json");
        match http::send(&req).await {
            Ok(reply) => (i32::from(reply.status), reply.text()),
            Err(_) => (0, String::new()),
        }
    }

    async fn claude(&mut self, force_refresh: bool) {
        // One read, and at most one more after a refresh: a refresh that
        // changed nothing must not loop.
        for pass in 0..2 {
            let Some(creds) = self.credentials().await else {
                return self.update(|s| {
                    s.claude = Claude::NoAuth;
                    s.claude_rows.clear();
                });
            };
            let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
            let expired = usage::credentials_expired(creds.expires_at_ms, now_ms);
            let tier = usage::tier_label(&creds.subscription_type, &creds.rate_limit_tier);
            self.update(|s| {
                s.claude = if expired { Claude::Stale } else { Claude::Loading };
                s.claude_tier = tier;
            });
            if pass == 0 && (expired || force_refresh) && self.refresh(force_refresh).await {
                continue;
            }
            let (status, body) = self.usage(&creds.access_token).await;
            if status == 401 || status == 403 {
                self.update(|s| {
                    s.claude = if creds.has_refresh_token { Claude::Stale } else { Claude::NoAuth };
                    s.claude_rows.clear();
                });
                if pass == 0 && creds.has_refresh_token && self.refresh(false).await {
                    continue;
                }
                return;
            }
            if !(200..300).contains(&status) {
                return self.update(|s| s.claude = Claude::Error);
            }
            return match usage::parse_usage(&body) {
                Ok(rows) => self.update(|s| {
                    s.claude_rows = rows;
                    s.claude = Claude::Ok;
                }),
                Err(_) => self.update(|s| s.claude = Claude::Error),
            };
        }
    }

    async fn codex(&self) {
        self.update(|s| {
            s.codex = Codex::Loading;
            s.codex_tier.clear();
            s.codex_rows.clear();
        });
        let ended = self.codex_session().await;
        self.update(|s| s.codex = ended);
    }

    async fn codex_session(&self) -> Codex {
        let child = crate::services::proc::command("codex")
            .args(["-s", "read-only", "-a", "untrusted", "app-server"])
            .stdin(async_process::Stdio::piped())
            .stdout(async_process::Stdio::piped())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) if e.kind() == ErrorKind::NotFound => return Codex::Missing,
            Err(_) => return Codex::Error,
        };
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return Codex::Error };
        let mut lines = BufReader::new(stdout).lines();
        let send = |value: serde_json::Value| format!("{value}\n");
        let hello = send(json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "formalshell", "version": "1"}}}));
        if stdin.write_all(hello.as_bytes()).await.is_err() {
            return Codex::Error;
        }
        let mut plan = String::new();
        let session = async {
            while let Some(Ok(line)) = lines.next().await {
                let Some(id) = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|m| m.get("id")?.as_i64()) else { continue };
                let reply = match id {
                    1 => send(json!({"method": "initialized", "params": {}})) + &send(json!({"id": 2, "method": "account/read", "params": {}})),
                    2 => {
                        plan = usage::parse_codex_account(&line).unwrap_or_default();
                        send(json!({"id": 3, "method": "account/rateLimits/read", "params": {}}))
                    }
                    3 => {
                        return match usage::parse_codex_rate_limits(&line) {
                            Ok(limits) => {
                                let tier = if limits.plan_type.is_empty() { plan.clone() } else { limits.plan_type };
                                self.update(|s| {
                                    s.codex_tier = tier;
                                    s.codex_rows = limits.rows;
                                });
                                Codex::Ok
                            }
                            Err(_) => Codex::Error,
                        };
                    }
                    _ => continue,
                };
                if stdin.write_all(reply.as_bytes()).await.is_err() {
                    return Codex::Error;
                }
            }
            Codex::Error
        };
        session
            .or(async {
                Timer::after(CODEX_TIMEOUT).await;
                Codex::Error
            })
            .await
    }
}

pub async fn run(ctx: Ctx) {
    let rx = config::subscribe();
    let kick = kicked(Source::Usage);
    while kick.try_recv().is_ok() {}
    let mut poll = Poll { ctx: ctx.clone(), state: Rc::new(RefCell::new(State::default())), last_refresh: None };
    let mut force = false;
    loop {
        let cfg = read();
        poll.update(|s| {
            s.claude_enabled = cfg.claude;
            s.codex_enabled = cfg.codex;
            if !cfg.claude {
                s.claude = Claude::Unknown;
            }
            if !cfg.codex {
                s.codex = Codex::Unknown;
            }
        });
        if cfg.claude {
            poll.claude(force).await;
        }
        if cfg.codex {
            poll.codex().await;
        }
        let kicked = async {
            let _ = kick.recv().await;
            true
        };
        let waited = async {
            idle(cfg.interval, &rx, &cfg, read).await;
            false
        };
        force = kicked.or(waited).await;
    }
}
