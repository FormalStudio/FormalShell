use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

use crate::model::Security;

/// `a{sa{sv}}` as NetworkManager returns it.
pub(crate) type Settings = HashMap<String, HashMap<String, OwnedValue>>;
/// `a{sa{sv}}` as NetworkManager takes it.
pub(crate) type NewSettings = HashMap<String, HashMap<String, Value<'static>>>;

fn section(entries: Vec<(&str, Value<'static>)>) -> HashMap<String, Value<'static>> {
    entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// Every profile the shell makes starts here. With an `owner` it is that
/// user's own (`connection.permissions`, as GNOME and plasma-nm write it), so
/// adding, changing and deleting it falls under polkit's
/// `settings.modify.own` rather than `settings.modify.system`, which distros
/// leave at auth_admin.
fn base(ssid: &str, owner: Option<&str>) -> NewSettings {
    let mut connection = section(vec![
        ("id", Value::from(ssid.to_owned())),
        ("type", Value::from("802-11-wireless".to_owned())),
        ("autoconnect", Value::from(true)),
    ]);
    if let Some(owner) = owner {
        connection.insert("permissions".to_owned(), Value::from(vec![format!("user:{owner}:")]));
    }
    NewSettings::from([
        ("connection".to_owned(), connection),
        (
            "802-11-wireless".to_owned(),
            section(vec![("ssid", Value::from(ssid.as_bytes().to_vec()))]),
        ),
    ])
}

/// A profile for a network with no security.
pub(crate) fn open_profile(ssid: &str, owner: Option<&str>) -> NewSettings {
    base(ssid, owner)
}

/// A profile for a passphrase network. SAE access points get `sae`, every
/// other secured one `wpa-psk`.
pub(crate) fn psk_profile(ssid: &str, owner: Option<&str>, security: Security, psk: &str) -> NewSettings {
    let key_mgmt = if security == Security::Sae {
        "sae"
    } else {
        "wpa-psk"
    };
    let mut settings = base(ssid, owner);
    settings.insert(
        "802-11-wireless-security".to_owned(),
        section(vec![
            ("key-mgmt", Value::from(key_mgmt.to_owned())),
            ("psk", Value::from(psk.to_owned())),
        ]),
    );
    settings
}

/// PEAP with MSCHAPv2, the same profile `dev/smoke.d/wifi.sh`'s nmcli script
/// builds, with the 8 second auth timeout it sets. No CA certificate is
/// pinned, as there.
pub(crate) fn eap_profile(ssid: &str, owner: Option<&str>, identity: &str, password: &str) -> NewSettings {
    let mut settings = base(ssid, owner);
    settings.insert(
        "802-11-wireless-security".to_owned(),
        section(vec![("key-mgmt", Value::from("wpa-eap".to_owned()))]),
    );
    settings.insert(
        "802-1x".to_owned(),
        section(vec![
            ("eap", Value::from(vec!["peap".to_owned()])),
            ("phase2-auth", Value::from("mschapv2".to_owned())),
            ("identity", Value::from(identity.to_owned())),
            ("password", Value::from(password.to_owned())),
            ("auth-timeout", Value::from(8_i32)),
        ]),
    );
    settings
}

/// What a saved profile says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Profile {
    pub ssid: String,
    pub key_mgmt: Option<String>,
    /// `802-11-wireless.hidden`.
    pub hidden: bool,
}

impl Profile {
    /// `None` for anything that is not a Wi-Fi profile.
    pub fn parse(settings: &Settings) -> Option<Self> {
        let connection = settings.get("connection")?;
        let kind: String = connection.get("type")?.try_clone().ok()?.try_into().ok()?;
        if kind != "802-11-wireless" {
            return None;
        }
        let bytes: Vec<u8> = settings
            .get("802-11-wireless")?
            .get("ssid")?
            .try_clone()
            .ok()?
            .try_into()
            .ok()?;
        let key_mgmt = settings
            .get("802-11-wireless-security")
            .and_then(|s| s.get("key-mgmt"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| String::try_from(v).ok());
        let hidden = settings
            .get("802-11-wireless")
            .and_then(|s| s.get("hidden"))
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false);
        Some(Self {
            ssid: String::from_utf8_lossy(&bytes).into_owned(),
            key_mgmt,
            hidden,
        })
    }

    pub fn security(&self) -> Security {
        self.key_mgmt
            .as_deref()
            .map_or(Security::Open, Security::from_key_mgmt)
    }
}

/// Puts a new passphrase on a saved profile, keeping everything else.
pub(crate) fn with_psk(mut saved: Settings, psk: &str) -> Settings {
    let security = saved
        .entry("802-11-wireless-security".to_owned())
        .or_default();
    if let Ok(value) = Value::from(psk.to_owned()).try_to_owned() {
        security.insert("psk".to_owned(), value);
    }
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(settings: NewSettings) -> Settings {
        settings
            .into_iter()
            .map(|(k, section)| {
                (
                    k,
                    section
                        .into_iter()
                        .map(|(k, v)| (k, v.try_to_owned().unwrap()))
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn psk_profile_reads_back_as_a_wifi_profile() {
        let p = Profile::parse(&owned(psk_profile("FORMALTEST", None, Security::Wpa2Psk, "pw")))
            .expect("wifi profile");
        assert_eq!(p.ssid, "FORMALTEST");
        assert_eq!(p.key_mgmt.as_deref(), Some("wpa-psk"));
        assert_eq!(p.security(), Security::Wpa2Psk);
    }

    #[test]
    fn sae_access_points_get_sae() {
        let s = psk_profile("x", None, Security::Sae, "pw");
        let key_mgmt: String = s["802-11-wireless-security"]["key-mgmt"]
            .try_clone()
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(key_mgmt, "sae");
    }

    #[test]
    fn eap_profile_is_peap_mschapv2() {
        let s = eap_profile("CORP", None, "user", "pw");
        let x = &s["802-1x"];
        let phase2: String = x["phase2-auth"].try_clone().unwrap().try_into().unwrap();
        assert_eq!(phase2, "mschapv2");
        let eap: Vec<String> = x["eap"].try_clone().unwrap().try_into().unwrap();
        assert_eq!(eap, ["peap"]);
        let p = Profile::parse(&owned(s)).unwrap();
        assert_eq!(p.security(), Security::Wpa2Eap);
    }

    #[test]
    fn an_owned_profile_names_its_user_alone() {
        for s in [
            open_profile("x", Some("kyan")),
            psk_profile("x", Some("kyan"), Security::Wpa2Psk, "pw"),
            eap_profile("x", Some("kyan"), "user", "pw"),
        ] {
            let permissions: Vec<String> =
                s["connection"]["permissions"].try_clone().unwrap().try_into().unwrap();
            assert_eq!(permissions, ["user:kyan:"]);
        }
        assert!(!open_profile("x", None)["connection"].contains_key("permissions"));
    }

    #[test]
    fn other_profile_types_are_not_wifi() {
        let mut s = owned(psk_profile("x", None, Security::Wpa2Psk, "pw"));
        s.get_mut("connection").unwrap().insert(
            "type".to_owned(),
            Value::from("802-3-ethernet".to_owned())
                .try_to_owned()
                .unwrap(),
        );
        assert_eq!(Profile::parse(&s), None);
    }

    #[test]
    fn with_psk_overwrites_only_the_secret() {
        let saved = owned(psk_profile("x", None, Security::Wpa2Psk, "old"));
        let updated = with_psk(saved, "new");
        let sec = &updated["802-11-wireless-security"];
        let psk: String = sec["psk"].try_clone().unwrap().try_into().unwrap();
        assert_eq!(psk, "new");
        assert!(sec.contains_key("key-mgmt"));
        assert!(updated.contains_key("802-11-wireless"));
    }
}
