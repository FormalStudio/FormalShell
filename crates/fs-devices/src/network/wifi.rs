//! A Wi-Fi network as the shell reads it off NetworkManager, the one
//! action in flight on one, and the last one that failed.

use fs_types::{Fraction, Kbps, Megahertz};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Empty for a hidden network.
    pub ssid: String,
    pub known: bool,
    pub connected: bool,
    pub changing: bool,
    pub secured: bool,
    pub enterprise: bool,
    /// An access point broadcasts it; false only for a saved network.
    pub in_range: bool,
    pub signal: Fraction,
    /// `Security::label`'s name for its class.
    pub security: String,
    /// The device's addresses on it, `address/prefix`, while connected.
    pub addresses: Vec<String>,
    /// The access point standing for it; empty out of range.
    pub bssid: String,
    pub frequency: Option<Megahertz>,
    /// The fastest rate its access point offers.
    pub max_rate: Option<Kbps>,
    /// While connected: the negotiated rate, the gateway and the DNS
    /// servers.
    pub link_rate: Option<Kbps>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
}

/// One line of a network's details: a key unique in the list, its words,
/// its Lucide glyph and its value.
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    pub key: String,
    pub label: &'static str,
    pub icon: &'static str,
    pub value: String,
}

/// What the panel's details and the launcher's → list for a network: its
/// security and signal, the radio its access point is on, and while
/// connected the link's rate and what the device holds on it. A line with
/// nothing to say is left out.
pub fn details(r: &Row) -> Vec<Detail> {
    let mut out = Vec::new();
    let mut line = |key: String, label, icon, value: String| out.push(Detail { key, label, icon, value });
    line("security".into(), "Security", "shield", r.security.clone());
    let signal = if r.in_range { format!("{}%", r.signal.as_percent().round()) } else { "Not in range".to_owned() };
    line("signal".into(), "Signal", "signal", signal);
    if let Some(f) = r.frequency.filter(|_| r.in_range) {
        if let Some(band) = f.band() {
            line("band".into(), "Band", "radio-tower", band.to_owned());
        }
        if let Some(ch) = f.channel() {
            line("channel".into(), "Channel", "hash", ch.to_string());
        }
        line("frequency".into(), "Frequency", "radio", format!("{} MHz", f.as_mhz()));
    }
    if let Some(rate) = r.link_rate.filter(|_| r.connected) {
        line("link".into(), "Link speed", "gauge", rate_text(rate));
    }
    if let Some(rate) = r.max_rate.filter(|_| r.in_range) {
        line("max".into(), "Max speed", "gauge", rate_text(rate));
    }
    if r.in_range && !r.bssid.is_empty() {
        line("bssid".into(), "BSSID", "fingerprint", r.bssid.clone());
    }
    if r.connected {
        for (i, a) in r.addresses.iter().enumerate() {
            line(format!("address.{i}"), "IP address", "network", a.clone());
        }
        if let Some(g) = &r.gateway {
            line("gateway".into(), "Gateway", "router", g.clone());
        }
        for (i, d) in r.dns.iter().enumerate() {
            line(format!("dns.{i}"), "DNS", "globe", d.clone());
        }
    }
    out
}

fn rate_text(rate: Kbps) -> String {
    format!("{} Mbit/s", rate.as_mbps().round())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Connect,
    Disconnect,
    Forget,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Connect => "connect",
            ActionKind::Disconnect => "disconnect",
            ActionKind::Forget => "forget",
        }
    }

    /// What a row reads while the action runs on it.
    pub fn busy(self) -> &'static str {
        match self {
            ActionKind::Connect => "Connecting",
            ActionKind::Disconnect => "Disconnecting",
            ActionKind::Forget => "Forgetting",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub ssid: String,
    pub text: String,
    /// Fixed by typing the secret again rather than retrying.
    pub secret: bool,
}

/// Whether connecting to `row` asks for its secret first: a secured
/// network nobody knows, or one whose last attempt failed on the secret,
/// since a wrong password is retyped, never retried.
pub fn asks_secret(row: &Row, failure: Option<&Failure>) -> bool {
    failure.is_some_and(|f| f.ssid == row.ssid && f.secret) || (row.secured && !row.known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_asked_for_a_stranger_or_after_a_wrong_one() {
        let saved = Row { ssid: "Home".into(), known: true, secured: true, ..Row::default() };
        assert!(!asks_secret(&saved, None));
        assert!(asks_secret(&Row { known: false, ..saved.clone() }, None));
        assert!(!asks_secret(&Row { secured: false, known: false, ..saved.clone() }, None));
        let wrong = Failure { ssid: "Home".into(), text: "Wrong password".into(), secret: true };
        assert!(asks_secret(&saved, Some(&wrong)));
        assert!(!asks_secret(&saved, Some(&Failure { ssid: "Cafe".into(), ..wrong.clone() })));
        assert!(!asks_secret(&saved, Some(&Failure { secret: false, ..wrong })));
    }

    #[test]
    fn details_name_the_radio_and_the_link_while_connected() {
        let live = Row {
            ssid: "Home".into(),
            connected: true,
            in_range: true,
            signal: Fraction::from_fraction(0.8),
            security: "WPA2 Personal".into(),
            addresses: vec!["10.0.0.5/24".into()],
            bssid: "14:EB:B6:1E:A6:50".into(),
            frequency: Some(Megahertz::from_mhz(5180)),
            max_rate: Some(Kbps::from_kbps(1_170_000)),
            link_rate: Some(Kbps::from_kbps(866_700)),
            gateway: Some("10.0.0.1".into()),
            dns: vec!["10.0.0.1".into()],
            ..Row::default()
        };
        let lines: Vec<(String, String)> = details(&live).into_iter().map(|d| (d.key, d.value)).collect();
        let want = [
            ("security", "WPA2 Personal"),
            ("signal", "80%"),
            ("band", "5 GHz"),
            ("channel", "36"),
            ("frequency", "5180 MHz"),
            ("link", "867 Mbit/s"),
            ("max", "1170 Mbit/s"),
            ("bssid", "14:EB:B6:1E:A6:50"),
            ("address.0", "10.0.0.5/24"),
            ("gateway", "10.0.0.1"),
            ("dns.0", "10.0.0.1"),
        ];
        assert_eq!(lines, want.map(|(k, v)| (k.to_owned(), v.to_owned())));
        let near = Row { connected: false, ..live.clone() };
        assert!(details(&near).iter().all(|d| !["link", "address.0", "gateway", "dns.0"].contains(&d.key.as_str())));
        let away = Row { in_range: false, ..near };
        assert_eq!(details(&away).iter().map(|d| d.key.as_str()).collect::<Vec<_>>(), ["security", "signal"]);
    }
}
