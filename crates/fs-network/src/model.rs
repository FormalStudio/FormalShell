use zbus::zvariant::OwnedObjectPath;

/// Wi-Fi signal in percent, 0..=100.
///
/// NetworkManager's `AccessPoint.Strength` is percent and this keeps it that
/// way, so a 0..1 fraction never enters implicitly; the conversion is [`SignalStrength::as_fraction`], never implicit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct SignalStrength(u8);

impl SignalStrength {
    /// Clamps to 100.
    pub fn from_percent(value: u8) -> Self {
        Self(value.min(100))
    }

    pub fn as_percent(self) -> u8 {
        self.0
    }

    pub fn as_fraction(self) -> f64 {
        f64::from(self.0) / 100.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Wired,
    Wifi,
}

/// The state mapping: NetworkManager's 0..20
/// are unknown, 30 disconnected, 40..90 connecting, 100 connected, and 110
/// and 120 (deactivating, failed) disconnecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Unknown,
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

impl ConnectionState {
    pub(crate) fn from_device_state(wire: u32) -> Self {
        match wire {
            30 => Self::Disconnected,
            40..=90 => Self::Connecting,
            100 => Self::Connected,
            110..=120 => Self::Disconnecting,
            _ => Self::Unknown,
        }
    }

    /// `NMActiveConnectionState`: 1 activating, 2 activated, 3 deactivating.
    pub(crate) fn from_active_state(wire: u32) -> Self {
        match wire {
            1 => Self::Connecting,
            2 => Self::Connected,
            3 => Self::Disconnecting,
            _ => Self::Disconnected,
        }
    }

    /// Whether the device is mid-transition.
    pub fn is_changing(self) -> bool {
        matches!(self, Self::Connecting | Self::Disconnecting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub path: OwnedObjectPath,
    pub interface: String,
    pub kind: DeviceKind,
    pub state: ConnectionState,
    /// The raw `NMDeviceState` behind `state`; 10 is unmanaged.
    pub nm_state: u32,
}

impl Device {
    pub fn connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    pub fn managed(&self) -> bool {
        self.nm_state != 10
    }
}

/// The security classes a Wi-Fi network is shown with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Wpa3SuiteB192,
    Sae,
    Wpa2Eap,
    Wpa2Psk,
    WpaEap,
    WpaPsk,
    Wep,
    Owe,
    Open,
    Unknown,
}

impl Security {
    pub fn is_secured(self) -> bool {
        !matches!(self, Self::Open | Self::Owe)
    }

    /// Needs an identity and a secret, not a bare passphrase.
    pub fn is_enterprise(self) -> bool {
        matches!(self, Self::WpaEap | Self::Wpa2Eap | Self::Wpa3SuiteB192)
    }

    /// From an access point's `Flags`, `WpaFlags` and `RsnFlags`.
    pub(crate) fn from_ap_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        const PRIVACY: u32 = 0x1;
        const PSK: u32 = 0x100;
        const EAP: u32 = 0x200;
        const SAE: u32 = 0x400;
        const OWE: u32 = 0x800 | 0x1000;
        const SUITE_B: u32 = 0x2000;
        if rsn & SUITE_B != 0 {
            Self::Wpa3SuiteB192
        } else if rsn & EAP != 0 {
            Self::Wpa2Eap
        } else if wpa & EAP != 0 {
            Self::WpaEap
        } else if rsn & SAE != 0 {
            Self::Sae
        } else if rsn & PSK != 0 {
            Self::Wpa2Psk
        } else if wpa & PSK != 0 {
            Self::WpaPsk
        } else if rsn & OWE != 0 {
            Self::Owe
        } else if flags & PRIVACY != 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    /// The class as the network panel and the launcher name it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Wpa3SuiteB192 => "WPA3 Enterprise 192-bit",
            Self::Sae => "WPA3 Personal",
            Self::Wpa2Eap => "WPA2 Enterprise",
            Self::Wpa2Psk => "WPA2 Personal",
            Self::WpaEap => "WPA Enterprise",
            Self::WpaPsk => "WPA Personal",
            Self::Wep => "WEP",
            Self::Owe => "Enhanced Open",
            Self::Open => "Open",
            Self::Unknown => "Unknown",
        }
    }

    /// From a saved profile's `802-11-wireless-security` key-mgmt.
    pub(crate) fn from_key_mgmt(key_mgmt: &str) -> Self {
        match key_mgmt {
            "none" | "ieee8021x" => Self::Wep,
            "wpa-psk" => Self::Wpa2Psk,
            "wpa-eap" => Self::Wpa2Eap,
            "sae" => Self::Sae,
            "wpa-eap-suite-b-192" => Self::Wpa3SuiteB192,
            "owe" => Self::Owe,
            _ => Self::Open,
        }
    }
}

/// One SSID as one row: the strongest access point stands for the SSID, and a saved network
/// out of range stays listed with `visible: false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub device: OwnedObjectPath,
    pub ssid: String,
    pub signal: SignalStrength,
    pub security: Security,
    pub known: bool,
    pub visible: bool,
    pub state: ConnectionState,
    /// The device's IPv4 addresses as `address/prefix`, on the network it
    /// is connected to; empty on every other.
    pub addresses: Vec<String>,
}

impl WifiNetwork {
    pub fn connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub wifi_enabled: bool,
    pub devices: Vec<Device>,
    /// Connected first, then the rest in range, then saved ones out of
    /// range; known before unknown in each tier, strongest first.
    pub networks: Vec<WifiNetwork>,
}

/// Why a device left activation, from `NMDeviceStateReason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    NoSecrets,
    SupplicantDisconnect,
    SupplicantFailed,
    SupplicantTimeout,
    SsidNotFound,
    DhcpFailed,
    IpConfigUnavailable,
    Other(u32),
}

impl FailReason {
    pub(crate) fn from_wire(wire: u32) -> Self {
        match wire {
            5 => Self::IpConfigUnavailable,
            7 => Self::NoSecrets,
            8 => Self::SupplicantDisconnect,
            10 => Self::SupplicantFailed,
            11 => Self::SupplicantTimeout,
            15..=17 => Self::DhcpFailed,
            53 => Self::SsidNotFound,
            other => Self::Other(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_keeps_the_wire_scale() {
        let s = SignalStrength::from_percent(72);
        assert_eq!(s.as_percent(), 72);
        assert!((s.as_fraction() - 0.72).abs() < 1e-12);
        assert_eq!(SignalStrength::from_percent(250).as_percent(), 100);
    }

    #[test]
    fn device_states_map_from_networkmanager() {
        use ConnectionState::*;
        let cases = [
            (0, Unknown),
            (20, Unknown),
            (30, Disconnected),
            (40, Connecting),
            (90, Connecting),
            (100, Connected),
            (110, Disconnecting),
            (120, Disconnecting),
        ];
        for (wire, want) in cases {
            assert_eq!(ConnectionState::from_device_state(wire), want, "{wire}");
        }
    }

    #[test]
    fn classifies_access_point_security() {
        let wpa2_psk = Security::from_ap_flags(1, 0, 0x108 | 0x100);
        assert_eq!(wpa2_psk, Security::Wpa2Psk);
        assert_eq!(Security::from_ap_flags(1, 0, 0x200), Security::Wpa2Eap);
        assert_eq!(Security::from_ap_flags(1, 0x200, 0), Security::WpaEap);
        assert_eq!(Security::from_ap_flags(1, 0, 0x400), Security::Sae);
        assert_eq!(Security::from_ap_flags(1, 0x100, 0), Security::WpaPsk);
        assert_eq!(Security::from_ap_flags(1, 0, 0), Security::Wep);
        assert_eq!(Security::from_ap_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_ap_flags(0, 0, 0x800), Security::Owe);
        assert!(!Security::Open.is_secured());
        assert!(Security::Wpa2Eap.is_enterprise());
        assert!(!Security::Wpa2Psk.is_enterprise());
    }

    #[test]
    fn failure_reasons() {
        assert_eq!(FailReason::from_wire(7), FailReason::NoSecrets);
        assert_eq!(FailReason::from_wire(11), FailReason::SupplicantTimeout);
        assert_eq!(FailReason::from_wire(16), FailReason::DhcpFailed);
        assert_eq!(FailReason::from_wire(99), FailReason::Other(99));
    }
}
