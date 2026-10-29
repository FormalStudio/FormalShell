//! `formalshell install|update|uninstall`: the system side of a tarball
//! install (install.sh). Everything written here is something the NixOS and
//! home-manager modules write on a Nix system, so on NixOS these only point
//! at the modules.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const REPO: &str = "FormalStudio/FormalShell";
/// First line of every Hyprland file the installer owns.
const MARKER: &str = "-- Written by `formalshell install`; `formalshell update` rewrites it.";
const PAM_PATH: &str = "/etc/pam.d/formalshell-lock";
const GREETD_CONFIG: &str = "/etc/greetd/config.toml";
const GREETD_BACKUP: &str = "/etc/greetd/config.toml.pre-formalshell";
const GREETD_SESSION: &str = "/etc/greetd/formalshell-session.sh";
const GREETD_TMPFILES: &str = "/etc/tmpfiles.d/formalshell-greeter.conf";
const UNITS: [&str; 3] = ["formalshell.service", "formalshell-watchdog.service", "formalshell-watchdog.timer"];

const USAGE: &str = "usage: formalshell install [--yes] [--no-system]
       formalshell update [--yes] [--no-system] [--from <tarball>]
       formalshell uninstall [--yes]

  --yes        answer yes to every sudo prompt (the PAM file, greetd)
  --no-system  write only files under your home, never anything in /etc
  --from       update from a local tarball instead of the latest release
";

struct Opts {
    yes: bool,
    no_system: bool,
    from: Option<PathBuf>,
}

struct Layout {
    prefix: PathBuf,
    config: PathBuf,
    home: PathBuf,
}

impl Layout {
    fn share(&self) -> PathBuf {
        self.prefix.join("share/formalshell")
    }
    fn bin(&self, name: &str) -> PathBuf {
        self.prefix.join("bin").join(name)
    }
    fn unit_dir(&self) -> PathBuf {
        self.config.join("systemd/user")
    }
    fn hypr(&self, name: &str) -> PathBuf {
        self.config.join("hypr").join(name)
    }
    /// mango reads `~/.config/mango` whatever XDG_CONFIG_HOME says.
    fn mango(&self, name: &str) -> PathBuf {
        self.home.join(".config/mango").join(name)
    }
}

pub fn main(cmd: &str, args: &[String]) -> i32 {
    let mut opts = Opts { yes: false, no_system: false, from: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" | "-y" => opts.yes = true,
            "--no-system" => opts.no_system = true,
            "--from" if cmd == "update" => match it.next() {
                Some(p) => opts.from = Some(PathBuf::from(p)),
                None => return usage(),
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            _ => return usage(),
        }
    }
    if Path::new("/etc/NIXOS").exists() {
        println!("On NixOS the formalshell NixOS and home-manager modules own this; nothing to do.");
        return 0;
    }
    let layout = match layout() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("formalshell {cmd}: {e}");
            return 1;
        }
    };
    let result = match cmd {
        "install" => install(&layout, &opts),
        "update" => update(&layout, &opts),
        _ => uninstall(&layout, &opts),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("formalshell {cmd}: {e}");
            1
        }
    }
}

fn usage() -> i32 {
    eprint!("{USAGE}");
    2
}

/// The prefix is where install.sh unpacked the tarball: this binary sits at
/// `<prefix>/lib/formalshell/formalshell-rs` next to the tarball's manifest.
fn layout() -> Result<Layout, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this binary: {e}"))?;
    let prefix = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or("cannot work out the install prefix")?
        .to_path_buf();
    if !prefix.join("share/formalshell/MANIFEST").is_file() {
        return Err(format!(
            "{} is not a tarball install (no share/formalshell/MANIFEST under {}); install with install.sh",
            exe.display(),
            prefix.display()
        ));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".config"));
    Ok(Layout { prefix, config, home })
}

fn install(l: &Layout, opts: &Opts) -> Result<(), String> {
    let units = l.unit_dir();
    write_owned(&units.join("formalshell.service"), &unit_service(l))?;
    write_owned(&units.join("formalshell-watchdog.service"), &unit_watchdog(l))?;
    write_owned(&units.join("formalshell-watchdog.timer"), UNIT_TIMER)?;
    if systemctl(&["daemon-reload"]) && systemctl(&["enable", "formalshell.service", "formalshell-watchdog.timer"]) {
        println!("enabled formalshell.service and formalshell-watchdog.timer");
    } else {
        println!("note: no systemd user manager answered; the units are written and Hyprland starts them at login");
    }

    let example = fs::read_to_string(l.share().join("examples/hyprland/formalshell.lua")).map_err(|e| format!("reading the Hyprland example: {e}"))?;
    write_owned(&l.hypr("formalshell.lua"), &hypr_include(&example, &l.bin("formalshell-ipc")))?;
    hypr_entry(l)?;
    mango_include(l)?;

    if opts.no_system {
        return Ok(());
    }
    match pam_file() {
        Some(pam) => system_file(PAM_PATH, &pam, "the formalshell-lock PAM file the lock screen authenticates against", opts)?,
        None => println!("note: no /etc/pam.d/common-auth or system-auth to include; the lock screen will say \"PAM error\""),
    }
    greetd(l, opts)?;
    Ok(())
}

fn update(l: &Layout, opts: &Opts) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("formalshell-update-{}", std::process::id()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let tarball = match &opts.from {
            Some(p) => p.clone(),
            None => {
                let arch = std::env::consts::ARCH;
                let url = format!("https://github.com/{REPO}/releases/latest/download/formalshell-{arch}-linux.tar.gz");
                let dest = tmp.join("formalshell.tar.gz");
                println!("downloading {url}");
                run(Command::new("curl").args(["-fL", "--progress-bar", "-o"]).arg(&dest).arg(&url), false)?;
                dest
            }
        };
        let old = fs::read_to_string(l.share().join("MANIFEST")).unwrap_or_default();
        let unpacked = tmp.join("unpacked");
        fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
        run(Command::new("tar").arg("-xzf").arg(&tarball).arg("-C").arg(&unpacked), false)?;
        let new = fs::read_to_string(unpacked.join("formalshell/share/formalshell/MANIFEST")).map_err(|_| "the tarball carries no share/formalshell/MANIFEST")?;
        let version = fs::read_to_string(unpacked.join("formalshell/share/formalshell/VERSION")).unwrap_or_default();
        let sudo = !writable(&l.prefix);
        // tar replaces each file by unlinking it, so the running shell keeps
        // its old binary until the restart below.
        run(Command::new("tar").arg("-xzf").arg(&tarball).arg("-C").arg(&l.prefix).arg("--strip-components=1"), sudo)?;
        let keep: std::collections::HashSet<&str> = new.lines().collect();
        let stale: Vec<PathBuf> = old.lines().filter(|f| !f.is_empty() && !keep.contains(f)).map(|f| l.prefix.join(f)).collect();
        remove_files(&stale, sudo)?;
        println!("unpacked FormalShell {} into {}", version.trim(), l.prefix.display());
        Ok(())
    })();
    let _ = fs::remove_dir_all(&tmp);
    result?;
    // The new binary rewrites the installer's files from its own templates.
    let mut cmd = Command::new(l.bin("formalshell"));
    cmd.arg("install");
    if opts.yes {
        cmd.arg("--yes");
    }
    if opts.no_system {
        cmd.arg("--no-system");
    }
    run(&mut cmd, false)?;
    systemctl(&["try-restart", "formalshell.service"]);
    Ok(())
}

fn uninstall(l: &Layout, opts: &Opts) -> Result<(), String> {
    systemctl(&["disable", "--now", "formalshell-watchdog.timer", "formalshell.service"]);
    for unit in UNITS {
        remove_owned(&l.unit_dir().join(unit))?;
    }
    systemctl(&["daemon-reload"]);

    remove_owned(&l.hypr("formalshell.lua"))?;
    remove_owned(&l.mango("formalshell.conf"))?;
    let entry = l.hypr("hyprland.lua");
    if is_owned(&entry) {
        fs::remove_file(&entry).map_err(|e| format!("{}: {e}", entry.display()))?;
        let user = l.hypr("hypr-user.lua");
        if user.exists() {
            fs::rename(&user, &entry).map_err(|e| format!("{}: {e}", user.display()))?;
            println!("moved {} back to {}", user.display(), entry.display());
        }
    }

    if Path::new(PAM_PATH).exists() && ask(&format!("Remove {PAM_PATH}?"), opts) {
        remove_files(&[PathBuf::from(PAM_PATH)], !am_root())?;
    }
    if Path::new(GREETD_SESSION).exists() && ask("Remove the FormalShell greetd config and restore the previous one?", opts) {
        let sudo = !am_root();
        if Path::new(GREETD_BACKUP).exists() {
            run(Command::new("mv").args(["-f", GREETD_BACKUP, GREETD_CONFIG]), sudo)?;
        }
        remove_files(&[PathBuf::from(GREETD_SESSION), PathBuf::from(GREETD_TMPFILES)], sudo)?;
    }

    let manifest = fs::read_to_string(l.share().join("MANIFEST")).map_err(|e| e.to_string())?;
    let files: Vec<PathBuf> = manifest.lines().filter(|f| !f.is_empty()).map(|f| l.prefix.join(f)).collect();
    let sudo = !writable(&l.prefix);
    remove_files(&files, sudo)?;
    for dir in ["lib/formalshell/bin", "lib/formalshell", "share/formalshell"] {
        let dir = l.prefix.join(dir);
        if dir.exists() {
            run(Command::new("rm").arg("-rf").arg(&dir), sudo)?;
        }
    }
    println!("FormalShell is uninstalled; ~/.config/formalshell and its state are left in place.");
    Ok(())
}

fn unit_service(l: &Layout) -> String {
    format!(
        "{}\n[Unit]\nDescription=FormalShell\nPartOf=graphical-session.target\nAfter=graphical-session.target\n\n[Service]\nExecStart={}\nRestart=on-failure\n\n[Install]\nWantedBy=graphical-session.target\n",
        unit_marker(),
        l.bin("formalshell").display()
    )
}

fn unit_watchdog(l: &Layout) -> String {
    format!(
        "{}\n[Unit]\nDescription=FormalShell liveness probe\nPartOf=graphical-session.target\nAfter=formalshell.service\n\n[Service]\nType=oneshot\nExecStart={}\n",
        unit_marker(),
        l.bin("formalshell-watchdog").display()
    )
}

const UNIT_TIMER: &str = "# Written by `formalshell install`; `formalshell update` rewrites it.
[Unit]
Description=FormalShell liveness probe
PartOf=graphical-session.target

[Timer]
OnActiveSec=30s
OnUnitActiveSec=30s
AccuracySec=5s
Unit=formalshell-watchdog.service

[Install]
WantedBy=graphical-session.target
";

fn unit_marker() -> &'static str {
    UNIT_TIMER.lines().next().unwrap_or_default()
}

/// The binds call `formalshell-ipc` by name, and Hyprland's PATH need not
/// carry `~/.local/bin`.
fn hypr_include(example: &str, ipc: &Path) -> String {
    format!("{MARKER}\n{}", example.replace("\"formalshell-ipc call \"", &format!("\"{} call \"", ipc.display())))
}

/// hyprland.lua loads the shell's include, then the user's own file. A
/// hyprland.lua the installer did not write becomes that user file, once.
fn hypr_entry(l: &Layout) -> Result<(), String> {
    let entry = l.hypr("hyprland.lua");
    let user = l.hypr("hypr-user.lua");
    if entry.exists() && !is_owned(&entry) {
        if user.exists() {
            println!(
                "note: {} is your own and {} already exists, so neither is touched; add\n  dofile(\"{}\")\nto your hyprland.lua",
                entry.display(),
                user.display(),
                l.hypr("formalshell.lua").display()
            );
            return Ok(());
        }
        fs::rename(&entry, &user).map_err(|e| format!("{}: {e}", entry.display()))?;
        println!("moved your {} to {}, which updates never touch", entry.display(), user.display());
    }
    if !user.exists() {
        write_new(&user, "-- Your own Hyprland settings (monitors, input, binds). FormalShell never rewrites this file.\n")?;
    }
    let systemctl = "systemctl --user import-environment WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP XDG_SESSION_TYPE && systemctl --user start formalshell.service formalshell-watchdog.timer";
    let body = format!(
        "{MARKER}\n-- Put your own settings in hypr-user.lua.\nlocal dir = (os.getenv(\"XDG_CONFIG_HOME\") or (os.getenv(\"HOME\") .. \"/.config\")) .. \"/hypr/\"\ndofile(dir .. \"formalshell.lua\")\ndofile(dir .. \"hypr-user.lua\")\n\n-- A plain Hyprland login never reaches graphical-session.target.\nhl.on(\"hyprland.start\", function() hl.exec_cmd([==[{systemctl}]==]) end)\n"
    );
    write_owned(&entry, &body)
}

/// The mango include beside the user's own config.conf, which is never
/// written: it only has to source the include.
fn mango_include(l: &Layout) -> Result<(), String> {
    let example = fs::read_to_string(l.share().join("examples/mango/formalshell.conf")).map_err(|e| format!("reading the mango example: {e}"))?;
    let ipc = format!("{} call ", l.bin("formalshell-ipc").display());
    write_owned(&l.mango("formalshell.conf"), &format!("{}\n{}", unit_marker(), example.replace("formalshell-ipc call ", &ipc)))?;
    let config = l.mango("config.conf");
    let sourced = fs::read_to_string(&config).is_ok_and(|t| t.lines().any(|line| line.trim_start().starts_with("source") && line.contains("formalshell.conf")));
    if !sourced {
        println!("note: for mango, add\n  source=./formalshell.conf\nto {}", config.display());
    }
    Ok(())
}

fn is_owned(path: &Path) -> bool {
    fs::File::open(path)
        .ok()
        .and_then(|f| BufReader::new(f).lines().next())
        .and_then(Result::ok)
        .is_some_and(|line| line == MARKER || line == unit_marker())
}

fn write_owned(path: &Path, body: &str) -> Result<(), String> {
    if fs::read_to_string(path).is_ok_and(|old| old == body) {
        return Ok(());
    }
    if path.exists() && !is_owned(path) {
        return Err(format!("{} exists and was not written by the installer; move it aside and rerun", path.display()));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn write_new(path: &Path, body: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn remove_owned(path: &Path) -> Result<(), String> {
    if is_owned(path) {
        fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("removed {}", path.display());
    }
    Ok(())
}

/// `common-auth` on Debian, Ubuntu and openSUSE, `system-auth` on Arch and
/// Fedora: the same stack the distro's own login uses. `include` rather than
/// `@include`, which is a Debian patch that openSUSE's PAM rejects.
fn pam_file() -> Option<String> {
    pam_for(Path::new("/etc/pam.d/common-auth").exists(), Path::new("/etc/pam.d/system-auth").exists())
}

fn pam_for(common: bool, system: bool) -> Option<String> {
    if common {
        Some("#%PAM-1.0\nauth       include      common-auth\naccount    include      common-account\n".into())
    } else if system {
        Some("#%PAM-1.0\nauth       include      system-auth\naccount    include      system-auth\n".into())
    } else {
        None
    }
}

/// The greeter runs under sway, as nixosModules.formalshell-greeter does,
/// and only where greetd and sway are both installed.
fn greetd(l: &Layout, opts: &Opts) -> Result<(), String> {
    if !Path::new("/etc/greetd").is_dir() {
        return Ok(());
    }
    if !on_path("sway") {
        println!("note: greetd is installed but sway is not; the FormalShell greeter runs under sway, so greetd is left as it is");
        return Ok(());
    }
    let current = fs::read_to_string(GREETD_CONFIG).unwrap_or_default();
    let user = greetd_user(&current);
    let session = greeter_session(l);
    let tmpfiles = format!("d /run/formalshell-greeter 0700 {user} {user} -\nd /var/lib/formalshell-greeter 0700 {user} {user} -\n");
    let config = format!("[terminal]\nvt = 1\n\n[default_session]\ncommand = \"{GREETD_SESSION}\"\nuser = \"{user}\"\n");
    let current_session = fs::read_to_string(GREETD_SESSION).unwrap_or_default();
    if current == config && current_session == session && fs::read_to_string(GREETD_TMPFILES).is_ok_and(|t| t == tmpfiles) {
        return Ok(());
    }
    if !ask("Make the FormalShell greeter greetd's login screen (rewrites /etc/greetd/config.toml, keeping a backup)?", opts) {
        return Ok(());
    }
    let sudo = !am_root();
    if !current.is_empty() && !current.contains(GREETD_SESSION) && !Path::new(GREETD_BACKUP).exists() {
        run(Command::new("cp").args(["-a", GREETD_CONFIG, GREETD_BACKUP]), sudo)?;
    }
    put(GREETD_SESSION, &session, "755", sudo)?;
    put(GREETD_TMPFILES, &tmpfiles, "644", sudo)?;
    run(Command::new("systemd-tmpfiles").args(["--create", GREETD_TMPFILES]), sudo)?;
    put(GREETD_CONFIG, &config, "644", sudo)?;
    println!("greetd now starts the FormalShell greeter; `systemctl enable greetd` makes it the login screen");
    Ok(())
}

fn greetd_user(config: &str) -> String {
    config
        .lines()
        .filter_map(|line| line.trim().strip_prefix("user"))
        .filter_map(|rest| rest.trim_start().strip_prefix('='))
        .map(|v| v.trim().trim_matches('"').to_string())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "greeter".into())
}

/// greetd execs this with PAM's environment alone, and the greeter account's
/// home is not writable on every distro.
fn greeter_session(l: &Layout) -> String {
    format!(
        "#!/bin/sh
# Written by `formalshell install`; `formalshell update` rewrites it.
export XDG_RUNTIME_DIR=/run/formalshell-greeter
export HOME=/var/lib/formalshell-greeter
export XDG_SESSION_TYPE=wayland
printf 'output * bg #000000 solid_color\\n' > \"$HOME/sway.conf\"
# A compositor killed with greetd leaves its socket; the wait below would take it.
rm -f \"$XDG_RUNTIME_DIR\"/wayland-*
sway --config \"$HOME/sway.conf\" &
compositor=$!
for _ in $(seq 100); do
  socket=$(find \"$XDG_RUNTIME_DIR\" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -print -quit 2>/dev/null)
  [ -n \"$socket\" ] && break
  sleep 0.1
done
export WAYLAND_DISPLAY=$(basename \"$socket\")
{}
kill \"$compositor\" 2>/dev/null
wait \"$compositor\" 2>/dev/null
",
        l.bin("formalshell-greeter").display()
    )
}

/// One system file, written with sudo after a prompt, and left alone when it
/// already says the same thing.
fn system_file(path: &str, body: &str, what: &str, opts: &Opts) -> Result<(), String> {
    if fs::read_to_string(path).is_ok_and(|old| old == body) {
        return Ok(());
    }
    if !ask(&format!("Install {what} at {path}?"), opts) {
        println!("skipped {path}");
        return Ok(());
    }
    put(path, body, "644", !am_root())
}

fn put(path: &str, body: &str, mode: &str, sudo: bool) -> Result<(), String> {
    let mut cmd = Command::new("install");
    cmd.args(["-D", "-m", mode, "/dev/stdin", path]);
    let mut cmd = if sudo { with_sudo(&cmd) } else { cmd };
    let mut child = cmd.stdin(Stdio::piped()).spawn().map_err(|e| format!("install {path}: {e}"))?;
    child.stdin.take().map(|mut s| s.write_all(body.as_bytes())).transpose().map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("writing {path} failed"));
    }
    println!("wrote {path}");
    Ok(())
}

fn ask(question: &str, opts: &Opts) -> bool {
    if opts.yes {
        return true;
    }
    let Ok(mut tty) = fs::OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        println!("note: no terminal to ask \"{question}\"; skipped (rerun with --yes to accept)");
        return false;
    };
    let _ = write!(tty, "{question} [y/N] ");
    let mut line = String::new();
    let _ = BufReader::new(&tty).read_line(&mut line);
    matches!(line.trim(), "y" | "Y" | "yes")
}

fn remove_files(files: &[PathBuf], sudo: bool) -> Result<(), String> {
    let present: Vec<&PathBuf> = files.iter().filter(|f| f.symlink_metadata().is_ok()).collect();
    if present.is_empty() {
        return Ok(());
    }
    run(Command::new("rm").arg("-f").args(present), sudo)
}

fn run(cmd: &mut Command, sudo: bool) -> Result<(), String> {
    let mut wrapped = sudo.then(|| with_sudo(cmd));
    let cmd = wrapped.as_mut().unwrap_or(cmd);
    let shown = format!("{cmd:?}");
    let status = cmd.status().map_err(|e| format!("{shown}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{shown} failed ({status})")) }
}

fn with_sudo(cmd: &Command) -> Command {
    let mut sudo = Command::new("sudo");
    sudo.arg(cmd.get_program()).args(cmd.get_args());
    sudo
}

fn systemctl(args: &[&str]) -> bool {
    Command::new("systemctl").arg("--user").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn am_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

fn writable(dir: &Path) -> bool {
    let c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).unwrap_or_default();
    // SAFETY: c is a valid NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(root: &Path) -> Layout {
        Layout { prefix: root.join("prefix"), config: root.join("config"), home: root.join("home") }
    }

    #[test]
    fn pam_follows_the_distro_stack() {
        assert!(pam_for(true, true).unwrap().contains("auth       include      common-auth"));
        assert!(pam_for(false, true).unwrap().contains("include      system-auth"));
        assert_eq!(pam_for(false, false), None);
    }

    #[test]
    fn greetd_user_comes_from_the_existing_config() {
        assert_eq!(greetd_user("[default_session]\ncommand = \"agreety\"\nuser = \"_greetd\"\n"), "_greetd");
        assert_eq!(greetd_user(""), "greeter");
    }

    #[test]
    fn include_calls_ipc_by_path() {
        let out = hypr_include("local fs_call = \"formalshell-ipc call \"\n", Path::new("/opt/x/bin/formalshell-ipc"));
        assert!(out.starts_with(MARKER));
        assert!(out.contains("\"/opt/x/bin/formalshell-ipc call \""));
    }

    #[test]
    fn own_hyprland_lua_moves_to_the_user_file_once_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let l = layout(dir.path());
        fs::create_dir_all(l.hypr("")).unwrap();
        fs::write(l.hypr("hyprland.lua"), "mine\n").unwrap();
        hypr_entry(&l).unwrap();
        assert_eq!(fs::read_to_string(l.hypr("hypr-user.lua")).unwrap(), "mine\n");
        assert!(is_owned(&l.hypr("hyprland.lua")));
        fs::write(l.hypr("hypr-user.lua"), "edited\n").unwrap();
        hypr_entry(&l).unwrap();
        assert_eq!(fs::read_to_string(l.hypr("hypr-user.lua")).unwrap(), "edited\n");
    }

    #[test]
    fn the_mango_include_calls_ipc_by_path_and_stays_owned() {
        let dir = tempfile::tempdir().unwrap();
        let l = layout(dir.path());
        fs::create_dir_all(l.share().join("examples/mango")).unwrap();
        fs::write(l.share().join("examples/mango/formalshell.conf"), "bind=SUPER,k,spawn,formalshell-ipc call menu toggle\n").unwrap();
        mango_include(&l).unwrap();
        let out = fs::read_to_string(l.mango("formalshell.conf")).unwrap();
        assert!(is_owned(&l.mango("formalshell.conf")));
        assert!(out.ends_with(&format!("spawn,{} call menu toggle\n", l.bin("formalshell-ipc").display())));
    }

    #[test]
    fn a_foreign_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("formalshell.service");
        fs::write(&path, "someone else's\n").unwrap();
        assert!(write_owned(&path, "ours\n").is_err());
        let owned = dir.path().join("x.lua");
        write_owned(&owned, &format!("{MARKER}\none\n")).unwrap();
        write_owned(&owned, &format!("{MARKER}\ntwo\n")).unwrap();
        assert!(fs::read_to_string(&owned).unwrap().ends_with("two\n"));
    }
}
