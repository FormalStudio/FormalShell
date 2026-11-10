//! fspec as the shell's package manager (FormalSpec
//! `docs/shell-install.md`): Install mode's catalogue search, the install
//! and uninstall it starts, and which fspec package owns a desktop file.
//! The work itself is fspec's, on the operation stream Activity reads; this
//! only starts it and keeps what the launcher draws off. The sidebar's
//! Software card reads the spec checkout here too: the upstream commits the
//! machine has not applied yet, and the newest app change `fspec app undo`
//! would reverse while it is a day old at most, checked once it is first
//! drawn and every half hour after.
//!
//! On the UI thread it starts installs off what the store holds and acts
//! once one lands ([`land`]): the apps scanned again, and a pick for the
//! app an install brought.
//!
//! A search waits 250ms after the last query and a newer one drops the
//! child still answering an older one. `--offline` keeps crates.io and npm
//! out, so a keystroke never waits on the network.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::Timer;
use fs_activity::State as Run;
use fs_types::fspec::{AppVerb, Command};
use fs_menu::providers::{self as p, Info, Owner, Package};
use futures_lite::{FutureExt, future};
use serde_json::{Value, json};

use super::{http, menu, proc, state};
use crate::runtime::Ctx;
use crate::store::{self, Store};

const DEBOUNCE: Duration = Duration::from_millis(250);
const RECHECK: Duration = Duration::from_secs(30 * 60);
/// How long the card offers the last app change and its Undo.
const LAST_SHOWN: Duration = Duration::from_secs(24 * 60 * 60);
const SEARCH_LIMIT: u32 = 30;
/// What a search answers with no fspec to ask.
pub const NO_FSPEC: &str = "Installing apps needs fspec";
/// `check`'s answer on a machine whose fspec has no spec to read.
pub const NO_SPEC: &str = "No spec on this machine";

/// One install or uninstall the shell started, by package id.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    pub id: String,
    /// The catalog's name for it, which the app it brings is matched on.
    pub name: String,
    pub verb: AppVerb,
    /// The operation id once fspec printed one, `Ok(None)` while it starts,
    /// the reason it never started.
    pub op: Result<Option<String>, String>,
    /// The desktop ids before the install, so the one it brings is found.
    pub before: HashSet<String>,
    /// The app the install brought, once the scan after it found one.
    pub app: Option<String>,
}

/// The spec checkout as the Software card shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Software {
    /// The upstream's commits the machine has not applied and the
    /// upstream's name, why that cannot be told, `None` before the first
    /// check.
    pub behind: Option<Result<(u32, String), String>>,
    /// The newest app change on the upstream for this host, and the entry,
    /// while it is under [`LAST_SHOWN`] old.
    pub last: Option<(AppVerb, String)>,
    /// An Apply the card started that has not ended.
    pub applying: bool,
    /// The last Apply or Undo that did not go, and why.
    pub failed: Option<(Change, String)>,
}

/// The card's two buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Apply,
    Undo,
}

impl Change {
    fn as_str(self) -> &'static str {
        match self {
            Change::Apply => "apply",
            Change::Undo => "undo",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The last query answered and its packages or why there are none.
    pub search: Option<(String, Result<Vec<Package>, String>)>,
    pub started: Vec<Started>,
    /// `fspec app owner` by desktop id.
    pub owners: HashMap<String, Owner>,
    /// The fspec operations whose success this shell already acted on (a
    /// rescan, a pick for the new app). Written on the UI thread.
    pub landed: HashSet<String>,
    pub software: Software,
    /// `fspec info` by package id, or why it did not answer.
    pub info: HashMap<String, Result<Info, String>>,
    /// A screenshot URL's cached file, `None` when it could not be fetched.
    pub shots: HashMap<String, Option<String>>,
}

pub enum Diff {
    Search(String, Result<Vec<Package>, String>),
    Started(Started),
    Owner(String, Owner),
    Checked(Result<(u32, String), String>, Option<(AppVerb, String)>),
    /// An Apply started or ended, and the change that did not go.
    Changing(bool, Option<(Change, String)>),
    Info(String, Result<Info, String>),
    Shot(String, Option<String>),
}

impl State {
    /// The IPC `status` reply; `fspec` is whether fspec is on PATH.
    pub fn status(&self, fspec: bool) -> Value {
        let search = self.search.as_ref().map(|(q, found)| match found {
            Ok(v) => json!({"query": q, "ids": v.iter().map(|f| f.id.as_str()).collect::<Vec<_>>()}),
            Err(why) => json!({"query": q, "error": why}),
        });
        let started: Vec<_> = self
            .started
            .iter()
            .map(|s| {
                let (op, error) = match &s.op {
                    Ok(op) => (op.clone(), None),
                    Err(why) => (None, Some(why.clone())),
                };
                json!({"id": s.id, "verb": s.verb.as_str(), "op": op, "error": error, "app": s.app})
            })
            .collect();
        let owners: serde_json::Map<String, Value> = self.owners.iter().map(|(k, o)| (k.clone(), json!({"id": o.id, "removable": o.removable}))).collect();
        let sw = &self.software;
        let (behind, upstream, error) = match &sw.behind {
            None => (None, None, None),
            Some(Ok((n, up))) => (Some(*n), Some(up.clone()), None),
            Some(Err(why)) => (None, None, Some(why.clone())),
        };
        let last = sw.last.as_ref().map(|(verb, name)| json!({"verb": verb.as_str(), "name": name}));
        let failed = sw.failed.as_ref().map(|(change, why)| json!({"change": change.as_str(), "why": why}));
        let software = json!({"checked": sw.behind.is_some(), "behind": behind, "upstream": upstream, "error": error, "last": last, "applying": sw.applying, "failed": failed});
        let info: serde_json::Map<String, Value> = self
            .info
            .iter()
            .map(|(id, i)| {
                let v = match i {
                    Ok(i) => json!({"name": i.name, "license": i.license, "developer": i.developer, "download": i.download, "installed": i.installed, "screenshot": i.screenshot}),
                    Err(why) => json!({"error": why}),
                };
                (id.clone(), v)
            })
            .collect();
        json!({"fspec": fspec, "search": search, "started": started, "owners": owners, "software": software, "info": info, "shots": self.shots})
    }

    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Search(q, found) => {
                let next = Some((q, found));
                if self.search == next {
                    return false;
                }
                self.search = next;
            }
            Diff::Started(s) => match self.started.iter_mut().find(|x| x.id == s.id) {
                Some(x) if x.op == s.op && x.verb == s.verb => return false,
                Some(x) => {
                    x.op = s.op;
                    x.verb = s.verb;
                    if !s.name.is_empty() {
                        x.name = s.name;
                    }
                    if s.verb == AppVerb::Install {
                        x.before = s.before;
                        x.app = None;
                    }
                }
                None => self.started.push(s),
            },
            Diff::Owner(id, owner) => {
                if self.owners.get(&id) == Some(&owner) {
                    return false;
                }
                self.owners.insert(id, owner);
            }
            Diff::Checked(behind, last) => {
                let (behind, last) = (Some(behind), last);
                if self.software.behind == behind && self.software.last == last {
                    return false;
                }
                self.software.behind = behind;
                self.software.last = last;
            }
            Diff::Info(id, info) => {
                if self.info.get(&id) == Some(&info) {
                    return false;
                }
                self.info.insert(id, info);
            }
            Diff::Shot(url, path) => {
                if self.shots.get(&url) == Some(&path) {
                    return false;
                }
                self.shots.insert(url, path);
            }
            Diff::Changing(applying, failed) => {
                if self.software.applying == applying && self.software.failed == failed {
                    return false;
                }
                self.software.applying = applying;
                self.software.failed = failed;
            }
        }
        true
    }

    /// The shell's own start of `id`, newest verb.
    pub fn started(&self, id: &str) -> Option<&Started> {
        self.started.iter().find(|s| s.id == id)
    }

    /// The search id an Uninstall of desktop entry `desktop` takes, once
    /// the owner query answered that fspec can take it out.
    pub fn removable(&self, desktop: &str) -> Option<&str> {
        self.owners.get(desktop).and_then(Owner::removable_id)
    }
}

pub enum Cmd {
    Search(String),
    /// A package id, its name, and the desktop ids there before it.
    Install(String, String, HashSet<String>),
    Uninstall(String),
    /// Who owns this desktop id, asked once until [`Cmd::Forget`].
    Owner(String),
    /// Ownership moved (an install, an uninstall, an undo landed).
    Forget,
    /// The Software card is up: check now unless it already has been, and
    /// every half hour after.
    Watch,
    /// Check the spec checkout now.
    Check,
    Change(Change),
    /// One package's details for the Install pane, and its screenshot,
    /// asked once each until one of them fails.
    Info(String),
    /// `fspec info` or the screenshot's fetch failed: ask again next time.
    InfoFailed(String),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn send(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

type Searching = Pin<Box<dyn Future<Output = (String, proc::Done)> + Send>>;

enum Wake {
    Cmd(Cmd),
    Due,
    Searched(String, proc::Done),
    Gone,
}

fn searched(code: i32, stdout: &str, stderr: &str) -> Result<Vec<Package>, String> {
    match code {
        0 => Ok(p::parse_search(stdout)),
        proc::MISSING => Err(NO_FSPEC.into()),
        proc::TIMED_OUT => Err("fspec search timed out".into()),
        _ => Err(p::failure(stderr, code)),
    }
}

fn spawn_app(ctx: &Ctx, verb: AppVerb, id: String, name: String, before: HashSet<String>) {
    let c = ctx.clone();
    ctx.spawn(async move {
        let done = proc::capture_err(&Command::App(verb, id.clone()).argv(), Duration::from_secs(300)).await;
        let op = match (done.code, p::parse_op(&done.stdout)) {
            (0, Some(op)) => Ok(Some(op)),
            (0, None) => Err("fspec printed no operation".into()),
            (proc::MISSING, _) => Err(NO_FSPEC.into()),
            (code, _) => Err(p::failure(&done.stderr, code)),
        };
        if let Err(why) = &op {
            eprintln!("packages: fspec app {} {id}: {why}", verb.as_str());
        }
        c.publish(store::Diff::Packages(Diff::Started(Started { id, name, verb, op, before, app: None })));
    });
}

/// `fspec info` for one package, then its screenshot into the cache: a
/// file already there is not fetched again.
fn spawn_info(ctx: &Ctx, id: String) {
    let c = ctx.clone();
    ctx.spawn(async move {
        let done = proc::capture_err(&Command::Info(id.clone()).argv(), Duration::from_secs(20)).await;
        let info = match done.code {
            0 => p::parse_info(&done.stdout).ok_or_else(|| "fspec printed no details".to_owned()),
            proc::MISSING => Err(NO_FSPEC.into()),
            proc::TIMED_OUT => Err("fspec info timed out".into()),
            code => Err(p::failure(&done.stderr, code)),
        };
        let url = info.as_ref().ok().and_then(|i| i.screenshot.clone()).filter(|u| u.starts_with("https://") || u.starts_with("http://"));
        if info.is_err() {
            send(Cmd::InfoFailed(id.clone()));
        }
        c.publish(store::Diff::Packages(Diff::Info(id.clone(), info)));
        let Some(url) = url else { return };
        let path = shot_path(&url);
        let have = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
        let got = have || fetch_shot(&url, &path).await;
        if !got {
            send(Cmd::InfoFailed(id));
        }
        c.publish(store::Diff::Packages(Diff::Shot(url, got.then(|| path.to_string_lossy().into_owned()))));
    });
}

/// Where a screenshot URL is cached, named by a hash of the whole URL:
/// Flathub reuses one last segment (`image-1_752x423@1.png`) across apps.
fn shot_path(url: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let base = crate::paths::cache();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut h);
    // `image::open` reads the format off the extension.
    let ext = url.rsplit('/').next().and_then(|last| last.rsplit_once('.')).map(|(_, e)| e).filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("png");
    base.join("formalshell/install-shots").join(format!("{:016x}.{ext}", h.finish()))
}

/// curl into a part file, renamed over `path` once whole.
async fn fetch_shot(url: &str, path: &Path) -> bool {
    let Some(dir) = path.parent() else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let part = path.with_extension("part");
    let req = http::Request::get(url, 20).follow(50).connect_timeout(4).max_filesize(8_388_608).output(&part);
    if http::get(&req).await.is_none() {
        let _ = std::fs::remove_file(&part);
        return false;
    }
    std::fs::rename(&part, path).is_ok()
}

/// The spec's directory, off `FSPEC_SPEC` as fspec reads it (a directory
/// or the spec.star in one).
fn spec_dir() -> Option<PathBuf> {
    let spec = PathBuf::from(std::env::var_os("FSPEC_SPEC").filter(|v| !v.is_empty())?);
    if spec.is_dir() { Some(spec) } else { spec.parent().map(Path::to_path_buf) }
}

/// The name fspec knows this machine by in its commits.
fn host() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|h| h.trim().to_owned()).unwrap_or_default()
}

async fn git(dir: &Path, args: &[&str], timeout: Duration) -> proc::Done {
    let dir = dir.to_string_lossy();
    let mut argv = vec!["git", "-C", &dir];
    argv.extend_from_slice(args);
    proc::capture_err(&proc::argv(&argv), timeout).await
}

/// `feat(apps): install <name> on <host>`, fspec's commit for one change.
fn app_change(subject: &str, host: &str) -> Option<(AppVerb, String)> {
    let rest = subject.trim().strip_prefix("feat(apps): ")?;
    let (verb, rest) = rest.split_once(' ')?;
    let verb = AppVerb::parse(verb)?;
    let name = rest.strip_suffix(&format!(" on {host}"))?;
    Some((verb, name.to_owned()))
}

/// The spec commits the machine's scopes were last applied from, off
/// `fspec facts --json`'s `applied`. An apply over ssh from another
/// machine's checkout never moves this one's HEAD, so HEAD says nothing of
/// what runs here.
async fn applied() -> Vec<String> {
    let done = proc::capture(&Command::Facts.argv(), Duration::from_secs(10)).await;
    if done.code != 0 {
        return Vec::new();
    }
    let Ok(facts) = serde_json::from_str::<Value>(&done.stdout) else { return Vec::new() };
    ["system", "user"].iter().filter_map(|scope| facts["applied"][scope].as_str().map(str::to_owned)).collect()
}

/// The upstream's commits not in `rev`, fetching `rev` off `remote` when
/// the checkout lacks it.
async fn behind_of(dir: &Path, remote: &str, rev: &str) -> Option<u32> {
    let commit = format!("{rev}^{{commit}}");
    if git(dir, &["cat-file", "-e", &commit], Duration::from_secs(10)).await.code != 0 {
        let _ = git(dir, &["fetch", "--quiet", remote, rev], Duration::from_secs(60)).await;
    }
    let count = git(dir, &["rev-list", "--count", &format!("{rev}..@{{u}}")], Duration::from_secs(10)).await;
    count.stdout.trim().parse().ok().filter(|_| count.code == 0)
}

/// The machine's standing against the checkout's upstream, fetched first; a
/// fetch that fails leaves the last one's answer. Counted from the scope
/// applied most recently, since an app change applies its own scope alone;
/// from HEAD where no scope records a commit.
async fn check() -> (Result<(u32, String), String>, Option<(AppVerb, String)>) {
    let Some(dir) = spec_dir() else { return (Err(NO_SPEC.into()), None) };
    let _ = git(&dir, &["fetch", "--quiet"], Duration::from_secs(60)).await;
    let up = git(&dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], Duration::from_secs(10)).await;
    if up.code != 0 {
        return (Err("The spec's branch has no upstream".into()), None);
    }
    let upstream = up.stdout.trim().to_owned();
    let remote = upstream.split_once('/').map_or(upstream.as_str(), |(r, _)| r);
    let mut counts = Vec::new();
    for rev in applied().await {
        counts.extend(behind_of(&dir, remote, &rev).await);
    }
    let behind = match counts.into_iter().min() {
        Some(n) => Ok((n, upstream.clone())),
        None => {
            let count = git(&dir, &["rev-list", "--count", "HEAD..@{u}"], Duration::from_secs(10)).await;
            match count.stdout.trim().parse::<u32>() {
                Ok(n) if count.code == 0 => Ok((n, upstream.clone())),
                _ => Err(p::failure(&count.stderr, count.code)),
            }
        }
    };
    let host = host();
    let grep = format!("--grep=^feat\\(apps\\): (install|uninstall) .+ on {}$", host.replace('.', "\\."));
    let log = git(&dir, &["log", "-1", "--format=%ct %s", "-E", &grep, "@{u}", "--", "apps.star"], Duration::from_secs(10)).await;
    let now = (crate::runtime::now_ms() / 1000) as u64;
    let last = log.stdout.lines().find_map(|l| recent_change(l, &host, now));
    (behind, last)
}

/// One `%ct %s` line: the app change, while it is under [`LAST_SHOWN`] old.
fn recent_change(line: &str, host: &str, now: u64) -> Option<(AppVerb, String)> {
    let (at, subject) = line.split_once(' ')?;
    let at = at.parse::<u64>().ok()?;
    if now.saturating_sub(at) >= LAST_SHOWN.as_secs() {
        return None;
    }
    app_change(subject, host)
}

fn spawn_check(ctx: &Ctx) {
    let c = ctx.clone();
    ctx.spawn(async move {
        let (behind, last) = check().await;
        c.publish(store::Diff::Packages(Diff::Checked(behind, last)));
    });
}

/// Apply: the upstream fast-forwarded into the checkout, then that commit
/// applied. Undo: the newest app change reversed, detached, on the stream.
fn spawn_change(ctx: &Ctx, change: Change) {
    let c = ctx.clone();
    let publish = move |applying, failed| c.publish(store::Diff::Packages(Diff::Changing(applying, failed)));
    let c = ctx.clone();
    ctx.spawn(async move {
        let failed = match change {
            Change::Apply => {
                publish(true, None);
                match spec_dir() {
                    None => Some(NO_SPEC.to_owned()),
                    Some(dir) => {
                        let merged = git(&dir, &["merge", "--ff-only", "--quiet", "@{u}"], Duration::from_secs(60)).await;
                        if merged.code != 0 {
                            Some(p::failure(&merged.stderr, merged.code))
                        } else {
                            let done = proc::capture_err(&Command::ApplyHead.argv(), Duration::from_secs(2 * 60 * 60)).await;
                            match done.code {
                                0 => None,
                                proc::MISSING => Some(NO_FSPEC.into()),
                                code => Some(p::failure(&done.stderr, code)),
                            }
                        }
                    }
                }
            }
            Change::Undo => {
                let done = proc::capture_err(&Command::Undo.argv(), Duration::from_secs(60)).await;
                match (done.code, p::parse_op(&done.stdout)) {
                    (0, Some(_)) => None,
                    (0, None) => Some("fspec printed no operation".into()),
                    (proc::MISSING, _) => Some(NO_FSPEC.into()),
                    (code, _) => Some(p::failure(&done.stderr, code)),
                }
            }
        };
        if let Some(why) = &failed {
            eprintln!("packages: {change:?}: {why}");
        }
        publish(false, failed.map(|why| (change, why)));
        let (behind, last) = check().await;
        c.publish(store::Diff::Packages(Diff::Checked(behind, last)));
    });
}

/// What Install mode's rows for `q` need asked, on the UI thread: fspec's
/// search once the query is two characters long, then the pictures of the
/// packages it found that the index has not read yet.
pub fn search_for(store: &Store, q: &str) {
    if !store.activity.fspec.installed {
        return;
    }
    let q = q.trim();
    if q.chars().count() < 2 {
        return;
    }
    send(Cmd::Search(q.to_owned()));
    let Some((answered, Ok(found))) = &store.packages.search else { return };
    if answered != q {
        return;
    }
    let icons: Vec<String> = found.iter().filter_map(|f| f.icon.clone()).filter(|i| i.starts_with('/') && !store.menu.row_icons.contains_key(i)).collect();
    if !icons.is_empty() {
        menu::ask(menu::Ask::Icons(icons));
    }
}

/// Starts an install of `id`, named off the search that listed it, the
/// launcher's Enter and Activity's Retry alike.
pub fn install(store: &Store, id: &str) {
    let name = match &store.packages.search {
        Some((_, Ok(found))) => found.iter().find(|f| f.id == id).map(|f| f.name.clone()),
        _ => None,
    };
    let name = name.or_else(|| store.packages.started(id).map(|s| s.name.clone())).unwrap_or_default();
    send(Cmd::Install(id.to_owned(), name, desktop_ids(store)));
}

/// The desktop ids the apps scan knows now, before an install.
fn desktop_ids(store: &Store) -> HashSet<String> {
    store.menu.exec.apps.keys().cloned().collect()
}

/// fspec operations that change what is installed.
fn changes_apps(kind: &str) -> bool {
    matches!(kind, "install" | "uninstall" | "undo")
}

/// What the shell does once an fspec install, uninstall or undo succeeds,
/// whoever started it: the apps scanned again and every owner answer
/// dropped. An install the launcher started also records a pick for the
/// app it brought, once the scan has it, so the app heads `Recent` once.
/// Run on the UI thread for every Activity and Menu change; true when the
/// launcher's rows may read differently.
pub fn land(store: &mut Store) -> bool {
    let done: Vec<String> = store
        .activity
        .activity
        .live()
        .iter()
        .chain(store.activity.activity.history())
        .filter(|i| i.origin == fs_activity::Origin::Fspec && changes_apps(&i.kind) && matches!(i.state, Run::Succeeded | Run::Done))
        .map(|i| i.id.clone())
        .filter(|id| !store.packages.landed.contains(id))
        .collect();
    let mut moved = false;
    if !done.is_empty() {
        store.packages.landed.extend(done);
        store.packages.owners.clear();
        send(Cmd::Forget);
        send(Cmd::Check);
        menu::ask(menu::Ask::Rescan);
        moved = true;
    }
    let now = desktop_ids(store);
    let labels: HashMap<String, String> =
        now.iter().filter_map(|id| store.menu.nodes().get(&format!("apps.{id}")).map(|n| (id.clone(), n.label.clone()))).collect();
    let mut claimed: HashSet<String> = store.packages.started.iter().filter_map(|s| s.app.clone()).collect();
    let mut picks = Vec::new();
    for s in store.packages.started.iter_mut().filter(|s| s.verb == AppVerb::Install && s.app.is_none()) {
        let Ok(Some(op)) = &s.op else { continue };
        if !store.packages.landed.contains(&fs_activity::fspec::item_id(op)) {
            continue;
        }
        let mut fresh: Vec<&String> = now
            .iter()
            .filter(|id| !s.before.contains(*id) && !claimed.contains(*id))
            .filter(|id| brought_by(id, labels.get(*id).map_or("", String::as_str), &s.id, &s.name))
            .collect();
        fresh.sort();
        if let Some(app) = fresh.first() {
            claimed.insert((*app).clone());
            s.app = Some((*app).clone());
            picks.push(fs_menu::actions::SubjectId::App((*app).clone()).to_string());
            moved = true;
        }
    }
    if !picks.is_empty() {
        let mut held = store.state.data.picks.clone();
        let at = crate::runtime::now_ms() as f64;
        for id in &picks {
            held = fs_menu::frecency::record(&held, id, at, None);
        }
        state::set(vec![state::Field::Picks(held)]);
    }
    moved
}

/// Whether desktop entry `desktop` (shown as `label`) is the app package
/// `id` (`<source>:<ref>`, catalog name `name`) brought: a Flathub app's
/// entry is named after its ref, anything else matches by name.
fn brought_by(desktop: &str, label: &str, id: &str, name: &str) -> bool {
    let reference = id.split_once(':').map_or(id, |(_, r)| r);
    let tail = |s: &str| s.rsplit(['.', '/']).next().unwrap_or(s).to_owned();
    desktop.eq_ignore_ascii_case(reference)
        || tail(desktop).eq_ignore_ascii_case(&tail(reference))
        || (!name.is_empty() && (label.eq_ignore_ascii_case(name) || desktop.eq_ignore_ascii_case(name)))
}

/// The app an Activity install row's Open launches, once it landed.
pub fn landed_app<'a>(store: &'a Store, source: &str) -> Option<&'a str> {
    let id = fs_activity::FspecSource::arg_of(source, "install")?;
    store.packages.started(id).filter(|s| s.verb == AppVerb::Install).and_then(|s| s.app.as_deref())
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    // The query last asked for, answered or not: the launcher asks again
    // on every resolve.
    let mut last: Option<String> = None;
    let mut pending: Option<String> = None;
    let mut due: Option<Instant> = None;
    let mut searching: Option<Searching> = None;
    let mut asked: HashSet<String> = HashSet::new();
    let mut asked_info: HashSet<String> = HashSet::new();
    let mut recheck: Option<Instant> = None;
    loop {
        let wake = {
            let timer = async {
                match due {
                    Some(t) => {
                        Timer::at(t).await;
                        Wake::Due
                    }
                    None => future::pending().await,
                }
            };
            let answer = async {
                match searching.as_mut() {
                    Some(f) => {
                        let (q, done) = f.await;
                        Wake::Searched(q, done)
                    }
                    None => future::pending().await,
                }
            };
            let watch = async {
                match recheck {
                    Some(t) => {
                        Timer::at(t).await;
                        Wake::Cmd(Cmd::Check)
                    }
                    None => future::pending().await,
                }
            };
            async { CHANNEL.1.recv().await.map_or(Wake::Gone, Wake::Cmd) }.or(timer).or(answer).or(watch).await
        };
        match wake {
            Wake::Gone => return,
            Wake::Cmd(Cmd::Search(q)) => {
                if last.as_deref() == Some(q.as_str()) {
                    continue;
                }
                searching = None;
                last = Some(q.clone());
                pending = Some(q);
                due = Some(Instant::now() + DEBOUNCE);
            }
            Wake::Due => {
                due = None;
                let Some(q) = pending.take() else { continue };
                let argv = Command::Search { query: q.clone(), limit: SEARCH_LIMIT }.argv();
                searching = Some(Box::pin(async move {
                    let done = proc::capture_err(&argv, Duration::from_secs(30)).await;
                    (q, done)
                }));
            }
            Wake::Searched(q, done) => {
                searching = None;
                ctx.publish(store::Diff::Packages(Diff::Search(q, searched(done.code, &done.stdout, &done.stderr))));
            }
            Wake::Cmd(Cmd::Install(id, name, before)) => {
                let started = Started { id: id.clone(), name: name.clone(), verb: AppVerb::Install, op: Ok(None), before: before.clone(), app: None };
                ctx.publish(store::Diff::Packages(Diff::Started(started)));
                spawn_app(&ctx, AppVerb::Install, id, name, before);
            }
            Wake::Cmd(Cmd::Uninstall(id)) => {
                let started = Started { id: id.clone(), name: String::new(), verb: AppVerb::Uninstall, op: Ok(None), before: HashSet::new(), app: None };
                ctx.publish(store::Diff::Packages(Diff::Started(started)));
                spawn_app(&ctx, AppVerb::Uninstall, id, String::new(), HashSet::new());
            }
            Wake::Cmd(Cmd::Owner(desktop)) => {
                if !asked.insert(desktop.clone()) {
                    continue;
                }
                let c = ctx.clone();
                ctx.spawn(async move {
                    let done = proc::capture(&Command::Owner(desktop.clone()).argv(), Duration::from_secs(20)).await;
                    let owner = if done.code == 0 { Owner::parse(&done.stdout) } else { Owner::default() };
                    c.publish(store::Diff::Packages(Diff::Owner(desktop, owner)));
                });
            }
            Wake::Cmd(Cmd::Forget) => asked.clear(),
            Wake::Cmd(Cmd::Watch) if recheck.is_some() => {}
            Wake::Cmd(Cmd::Watch | Cmd::Check) => {
                recheck = Some(Instant::now() + RECHECK);
                spawn_check(&ctx);
            }
            Wake::Cmd(Cmd::Change(change)) => spawn_change(&ctx, change),
            Wake::Cmd(Cmd::Info(id)) => {
                if !asked_info.insert(id.clone()) {
                    continue;
                }
                spawn_info(&ctx, id);
            }
            Wake::Cmd(Cmd::InfoFailed(id)) => {
                asked_info.remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_reads_each_way_it_ends() {
        assert_eq!(searched(proc::MISSING, "", ""), Err(NO_FSPEC.to_owned()));
        assert_eq!(searched(1, "", "fspec: no catalog\n"), Err("no catalog".to_owned()));
        assert_eq!(searched(0, "", ""), Ok(Vec::new()));
    }

    #[test]
    fn reads_fspecs_app_commits_for_this_host() {
        assert_eq!(app_change("feat(apps): install org.mozilla.firefox on g815", "g815"), Some((AppVerb::Install, "org.mozilla.firefox".into())));
        assert_eq!(app_change("feat(apps): uninstall btop on e1504g", "e1504g"), Some((AppVerb::Uninstall, "btop".into())));
        assert_eq!(app_change("feat(apps): install btop on e1504g", "g815"), None);
        assert_eq!(app_change("feat(apps): pin btop on g815", "g815"), None);
    }

    #[test]
    fn an_app_change_shows_for_a_day() {
        let line = "1000000 feat(apps): uninstall figlet on g815";
        assert_eq!(recent_change(line, "g815", 1000000 + 3600), Some((AppVerb::Uninstall, "figlet".into())));
        assert_eq!(recent_change(line, "g815", 1000000 + 24 * 3600), None);
        assert_eq!(recent_change("feat(apps): uninstall figlet on g815", "g815", 1000000), None);
    }

    #[test]
    fn an_install_claims_only_its_own_app() {
        assert!(brought_by("org.mozilla.firefox", "Firefox", "flatpak:org.mozilla.firefox", "Firefox"));
        assert!(brought_by("firefox", "Firefox", "zypper:firefox", "firefox"));
        assert!(brought_by("org.gnome.Btop", "btop", "zypper:btop", "btop"));
        assert!(!brought_by("org.gnome.Calculator", "Calculator", "zypper:btop", "btop"));
        assert!(!brought_by("org.example.Other", "Other", "aqua:ripgrep", "ripgrep"));
    }

    #[test]
    fn an_owner_answer_gates_the_uninstall() {
        let mut s = State::default();
        s.apply(Diff::Owner("firefox".into(), Owner { id: Some("flatpak:org.mozilla.firefox".into()), removable: true }));
        s.apply(Diff::Owner("gimp".into(), Owner { id: Some("zypper:gimp".into()), removable: false }));
        assert_eq!(s.removable("firefox"), Some("flatpak:org.mozilla.firefox"));
        assert_eq!(s.removable("gimp"), None);
        assert_eq!(s.removable("other"), None);
    }
}
