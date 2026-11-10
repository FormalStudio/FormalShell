//! Pure model for the Tailscale panel and widget: parses `tailscale status
//! --json` output into the shape the panel renders directly.
//!
//! Field names verified against a real `tailscale status --json` run
//! (BackendState, Self.HostName, Self.TailscaleIPs,
//! Peer.<nodekey>.{HostName, TailscaleIPs, Online, OS}). No exit nodes, no
//! Mullvad, no multi-account switching.

use serde_json::Value as Json;

use fs_js as js;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub online: bool,
    pub ip: Option<String>,
    pub os: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub ok: bool,
    pub backend_state: Option<String>,
    pub running: bool,
    pub needs_login: bool,
    pub self_name: Option<String>,
    pub self_ips: Vec<String>,
    pub peers: Vec<Peer>,
}

fn first_ip(ips: Option<&Json>) -> Option<String> {
    match ips {
        Some(Json::Array(list)) => list.first().and_then(Json::as_str).map(str::to_string),
        _ => None,
    }
}

fn non_empty_str(v: Option<&Json>) -> Option<String> {
    v.and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Falls back to the peer's own map key (its stable nodekey) when the daemon
/// has not reported a HostName yet, never a blank row.
fn peer(id: &str, raw: &Json) -> Peer {
    Peer {
        name: non_empty_str(js::field(raw, "HostName")).unwrap_or_else(|| id.to_string()),
        online: js::field(raw, "Online") == Some(&Json::Bool(true)),
        ip: first_ip(js::field(raw, "TailscaleIPs")),
        os: non_empty_str(js::field(raw, "OS")),
    }
}

/// Online-first, then alphabetical within each group, matching the ledger sort
/// every other panel table uses.
fn sort_peers(peers: &mut [Peer]) {
    peers.sort_by(|a, b| {
        if a.online != b.online {
            return if a.online { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        js::locale_compare(&a.name, &b.name)
    });
}

/// `raw` is `tailscale status --json`'s stdout, verbatim. An honest empty
/// status for anything missing or unparsable, never a panic: a
/// daemon-unreachable run prints a plain-text error instead of JSON, and an
/// unparsable response is exactly as unusable to the caller as a process
/// failure (the panel folds both into its "NO TAILSCALE" state).
pub fn parse_status(raw: &str) -> Status {
    let text = js::trim(raw);
    let empty = Status::default();
    if text.is_empty() {
        return empty;
    }
    let Ok(data) = serde_json::from_str::<Json>(text) else { return empty };
    if !js::is_object_like(&data) {
        return empty;
    }

    let backend_state = js::field(&data, "BackendState").and_then(Json::as_str).map(str::to_string);
    let self_node = js::field(&data, "Self").unwrap_or(&Json::Null);
    let self_ips = match js::field(self_node, "TailscaleIPs") {
        Some(Json::Array(ips)) => ips.iter().filter_map(Json::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };

    let mut peers: Vec<Peer> = js::field(&data, "Peer")
        .map(|raw_peers| js::entries(raw_peers).into_iter().map(|(id, p)| peer(&id, p)).collect())
        .unwrap_or_default();
    sort_peers(&mut peers);

    Status {
        ok: true,
        running: backend_state.as_deref() == Some("Running"),
        needs_login: backend_state.as_deref() == Some("NeedsLogin"),
        backend_state,
        self_name: non_empty_str(js::field(self_node, "HostName")),
        self_ips,
        peers,
    }
}

/// First address from a parsed status's own Self entry, or `None`.
pub fn self_ip(status: &Status) -> Option<String> {
    status.self_ips.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Shapes lifted from a real `tailscale status --json` run.
    fn running_fixture() -> String {
        json!({
            "BackendState": "Running",
            "Self": { "HostName": "MyLaptop", "TailscaleIPs": ["100.64.0.1", "fd7a:115c:a1e0::1"] },
            "Peer": {
                "nodekey:aaa": { "HostName": "Server", "TailscaleIPs": ["100.64.0.2"], "Online": true, "OS": "linux" },
                "nodekey:bbb": { "HostName": "Phone", "TailscaleIPs": ["100.64.0.3"], "Online": false, "OS": "android" }
            }
        })
        .to_string()
    }

    fn stopped_fixture() -> String {
        json!({ "BackendState": "Stopped", "Self": { "HostName": "MyLaptop", "TailscaleIPs": ["100.64.0.1"] }, "Peer": {} }).to_string()
    }

    fn needs_login_fixture() -> String {
        json!({ "BackendState": "NeedsLogin", "Self": {}, "AuthURL": "https://login.tailscale.com/a/abc123" }).to_string()
    }

    // The real CLI prints a plain-text error (not JSON) when it can't reach
    // tailscaled, so this fixture is deliberately not JSON either.
    const NO_DAEMON_FIXTURE: &str = "Failed to connect to local tailscaled; (tailscaled not running?)\n";

    #[test]
    fn running_parses_backend_state_and_self() {
        let s = parse_status(&running_fixture());
        assert!(s.ok);
        assert_eq!(s.backend_state.as_deref(), Some("Running"));
        assert!(s.running);
        assert!(!s.needs_login);
        assert_eq!(s.self_name.as_deref(), Some("MyLaptop"));
        assert_eq!(s.self_ips.len(), 2);
        assert_eq!(s.self_ips[0], "100.64.0.1");
    }

    #[test]
    fn running_peers_shaped_and_sorted_online_first_then_alpha() {
        let s = parse_status(&running_fixture());
        assert_eq!(s.peers.len(), 2);
        // Server is online, Phone is offline: online sorts first regardless of name.
        assert_eq!(s.peers[0].name, "Server");
        assert!(s.peers[0].online);
        assert_eq!(s.peers[0].ip.as_deref(), Some("100.64.0.2"));
        assert_eq!(s.peers[0].os.as_deref(), Some("linux"));
        assert_eq!(s.peers[1].name, "Phone");
        assert!(!s.peers[1].online);
    }

    #[test]
    fn running_peers_sort_alphabetically_within_same_online_state() {
        let fixture = json!({
            "BackendState": "Running",
            "Self": { "HostName": "Me", "TailscaleIPs": ["100.64.0.1"] },
            "Peer": {
                "nodekey:z": { "HostName": "Zeta", "TailscaleIPs": ["100.64.0.9"], "Online": true, "OS": "linux" },
                "nodekey:a": { "HostName": "Alpha", "TailscaleIPs": ["100.64.0.8"], "Online": true, "OS": "linux" }
            }
        })
        .to_string();
        let s = parse_status(&fixture);
        assert_eq!(s.peers[0].name, "Alpha");
        assert_eq!(s.peers[1].name, "Zeta");
    }

    #[test]
    fn stopped_reports_running_false_with_empty_peers() {
        let s = parse_status(&stopped_fixture());
        assert!(s.ok);
        assert_eq!(s.backend_state.as_deref(), Some("Stopped"));
        assert!(!s.running);
        assert!(!s.needs_login);
        assert_eq!(s.peers.len(), 0);
    }

    #[test]
    fn needs_login_reports_needs_login_with_honest_none_self() {
        let s = parse_status(&needs_login_fixture());
        assert!(s.ok);
        assert_eq!(s.backend_state.as_deref(), Some("NeedsLogin"));
        assert!(s.needs_login);
        assert!(!s.running);
        assert_eq!(s.self_name, None);
        assert_eq!(s.self_ips.len(), 0);
    }

    #[test]
    fn no_daemon_plain_text_output_is_honestly_unparsable() {
        let s = parse_status(NO_DAEMON_FIXTURE);
        assert!(!s.ok);
        assert_eq!(s.backend_state, None);
        assert_eq!(s.peers.len(), 0);
    }

    #[test]
    fn empty_string_is_honestly_unparsable() {
        assert!(!parse_status("").ok);
    }

    #[test]
    fn peer_missing_hostname_falls_back_to_id_not_blank() {
        let fixture = json!({
            "BackendState": "Running",
            "Self": { "HostName": "Me", "TailscaleIPs": ["100.64.0.1"] },
            "Peer": { "nodekey:noname": { "TailscaleIPs": ["100.64.0.5"], "Online": true } }
        })
        .to_string();
        let s = parse_status(&fixture);
        assert_eq!(s.peers[0].name, "nodekey:noname");
        assert_eq!(s.peers[0].os, None);
    }

    #[test]
    fn peer_missing_ips_reports_honest_none_ip() {
        let fixture = json!({
            "BackendState": "Running",
            "Self": { "HostName": "Me", "TailscaleIPs": ["100.64.0.1"] },
            "Peer": { "nodekey:noip": { "HostName": "NoIp", "Online": false } }
        })
        .to_string();
        assert_eq!(parse_status(&fixture).peers[0].ip, None);
    }

    #[test]
    fn self_ip_returns_first_address() {
        assert_eq!(self_ip(&parse_status(&running_fixture())).as_deref(), Some("100.64.0.1"));
    }

    #[test]
    fn self_ip_honest_none_when_no_addresses() {
        assert_eq!(self_ip(&parse_status(&needs_login_fixture())), None);
    }

    #[test]
    fn self_ip_honest_none_for_unparsable_status() {
        assert_eq!(self_ip(&parse_status(NO_DAEMON_FIXTURE)), None);
    }
}
